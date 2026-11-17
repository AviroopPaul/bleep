//! Moonshine base (int8 ONNX) run directly, so the wake phrase can be scored, not just read.
//!
//! Reading the best guess fails for a made-up word: "hey bleep" comes out as "I believe"
//! or "Ablea". Scoring asks the decoder how likely the phrase's own tokens are for this
//! audio, next to how likely its best guess was. A small gap means the phrase was said.
use anyhow::{anyhow, Context, Result};
use ndarray::{Array2, ArrayD, IxDyn};
use ort::session::Session;
use std::borrow::Cow;
use std::collections::HashMap;
use std::path::Path;

const START: i64 = 1;
const EOS: i64 = 2;
const LAYERS: usize = 8;
const HEADS: usize = 8;
const HEAD_DIM: usize = 52;
/// Moonshine emits about 6 tokens a second of speech; a little headroom on top.
const TOKENS_PER_SEC: f32 = 6.5;

/// Llama-style BPE (SentencePiece with byte fallback), enough to encode a short phrase.
pub struct Tokenizer {
    vocab: Vec<String>,
    ids: HashMap<String, i64>,
    ranks: HashMap<(String, String), usize>,
    special: Vec<i64>,
}

impl Tokenizer {
    pub fn load(path: &Path) -> Result<Self> {
        let json: serde_json::Value = serde_json::from_reader(std::io::BufReader::new(std::fs::File::open(path)?))?;
        let model = &json["model"];
        let v = model["vocab"].as_object().context("tokenizer vocab")?;
        let mut vocab = vec![String::new(); v.len() + 64];
        let mut ids = HashMap::new();
        for (tok, id) in v {
            let id = id.as_i64().context("token id")?;
            if id as usize >= vocab.len() {
                vocab.resize(id as usize + 1, String::new());
            }
            vocab[id as usize] = tok.clone();
            ids.insert(tok.clone(), id);
        }
        let mut ranks = HashMap::new();
        for (i, m) in model["merges"].as_array().context("tokenizer merges")?.iter().enumerate() {
            let pair = match m {
                serde_json::Value::Array(a) => (a[0].as_str().unwrap_or("").to_string(), a[1].as_str().unwrap_or("").to_string()),
                serde_json::Value::String(s) => match s.split_once(' ') {
                    Some((a, b)) => (a.to_string(), b.to_string()),
                    None => continue,
                },
                _ => continue,
            };
            ranks.insert(pair, i);
        }
        let special = json["added_tokens"].as_array().map(|a| a.iter().filter(|t| t["special"].as_bool() == Some(true)).filter_map(|t| t["id"].as_i64()).collect()).unwrap_or_default();
        Ok(Self { vocab, ids, ranks, special })
    }

    pub fn encode(&self, text: &str) -> Vec<i64> {
        let norm = format!("\u{2581}{}", text.replace(' ', "\u{2581}"));
        let mut parts: Vec<String> = norm.chars().map(String::from).collect();
        loop {
            let best = parts.windows(2).enumerate().filter_map(|(i, w)| self.ranks.get(&(w[0].clone(), w[1].clone())).map(|r| (*r, i))).min();
            let Some((_, i)) = best else { break };
            let merged = format!("{}{}", parts[i], parts[i + 1]);
            parts.splice(i..i + 2, [merged]);
        }
        let mut out = vec![];
        for p in parts {
            match self.ids.get(&p) {
                Some(id) => out.push(*id),
                None => out.extend(p.bytes().filter_map(|b| self.ids.get(&format!("<0x{b:02X}>")).copied())),
            }
        }
        out
    }

    pub fn decode(&self, ids: &[i64]) -> String {
        let mut bytes = vec![];
        for &id in ids {
            if self.special.contains(&id) {
                continue;
            }
            let Some(tok) = self.vocab.get(id as usize) else { continue };
            if let Some(hex) = tok.strip_prefix("<0x").and_then(|t| t.strip_suffix('>')) {
                if let Ok(b) = u8::from_str_radix(hex, 16) {
                    bytes.push(b);
                    continue;
                }
            }
            bytes.extend(tok.replace('\u{2581}', " ").as_bytes());
        }
        let s = String::from_utf8_lossy(&bytes);
        s.strip_prefix(' ').unwrap_or(&s).trim().to_string()
    }
}

type Cache = HashMap<String, ArrayD<f32>>;

fn empty_cache() -> Cache {
    let mut c = HashMap::new();
    for i in 0..LAYERS {
        for a in ["decoder", "encoder"] {
            for kv in ["key", "value"] {
                c.insert(format!("past_key_values.{i}.{a}.{kv}"), ArrayD::<f32>::zeros(IxDyn(&[0, HEADS, 1, HEAD_DIM])));
            }
        }
    }
    c
}

fn log_softmax_at(logits: &[f32], id: usize) -> f32 {
    let max = logits.iter().cloned().fold(f32::MIN, f32::max);
    let sum: f32 = logits.iter().map(|l| (l - max).exp()).sum();
    logits[id] - max - sum.ln()
}

fn argmax(logits: &[f32]) -> usize {
    logits.iter().enumerate().fold((0, f32::MIN), |b, (i, &l)| if l > b.1 { (i, l) } else { b }).0
}

/// A decoder run over one utterance: the encoder output and the running cache.
struct Run {
    hidden: ArrayD<f32>,
    mask: Array2<i64>,
    cache: Cache,
    steps: usize,
}

/// What scoring found.
#[derive(Debug, Clone, PartialEq)]
pub struct Spot {
    /// The best guess at what was said.
    pub heard: String,
    /// How far, per token, the phrase falls behind the best guess (0 when it is the best guess).
    pub gap: f32,
    /// What follows the phrase, decoded with the phrase forced in. Only filled when asked.
    pub rest: Option<String>,
}

pub struct Moonshine {
    enc: Session,
    dec: Session,
    enc_mask: bool,
    dec_mask: bool,
    pub tok: Tokenizer,
}

impl Moonshine {
    pub fn load(dir: &Path) -> Result<Self> {
        let enc = transcribe_rs::onnx::session::create_session(&dir.join("encoder_model.int8.onnx")).map_err(|e| anyhow!("speech encoder: {e}"))?;
        let dec = transcribe_rs::onnx::session::create_session(&dir.join("decoder_model_merged.int8.onnx")).map_err(|e| anyhow!("speech decoder: {e}"))?;
        let enc_mask = enc.inputs().iter().any(|i| i.name() == "attention_mask");
        let dec_mask = dec.inputs().iter().any(|i| i.name() == "encoder_attention_mask");
        Ok(Self { enc, dec, enc_mask, dec_mask, tok: Tokenizer::load(&dir.join("tokenizer.json"))? })
    }

    fn start(&mut self, audio: &[f32]) -> Result<Run> {
        let input = Array2::from_shape_vec((1, audio.len()), audio.to_vec())?;
        let mask = Array2::<i64>::ones((1, audio.len()));
        let mut inputs: Vec<(Cow<'_, str>, ort::value::DynValue)> = vec![("input_values".into(), ort::value::Value::from_array(input.into_dyn())?.into_dyn())];
        if self.enc_mask {
            inputs.push(("attention_mask".into(), ort::value::Value::from_array(mask.clone().into_dyn())?.into_dyn()));
        }
        let out = self.enc.run(inputs)?;
        let hidden = out.get("last_hidden_state").context("encoder output")?.try_extract_array::<f32>()?.to_owned();
        Ok(Run { hidden, mask, cache: empty_cache(), steps: 0 })
    }

    /// Feeds one token, returns the logits for the next.
    fn step(&mut self, run: &mut Run, token: i64) -> Result<Vec<f32>> {
        let cached = run.steps > 0;
        let mut inputs: Vec<(Cow<'_, str>, ort::value::DynValue)> = vec![
            ("input_ids".into(), ort::value::Value::from_array(Array2::from_shape_vec((1, 1), vec![token])?.into_dyn())?.into_dyn()),
            ("encoder_hidden_states".into(), ort::value::Value::from_array(run.hidden.clone())?.into_dyn()),
            ("use_cache_branch".into(), ort::value::Value::from_array(ndarray::arr1(&[cached]).into_dyn())?.into_dyn()),
        ];
        if self.dec_mask {
            inputs.push(("encoder_attention_mask".into(), ort::value::Value::from_array(run.mask.clone().into_dyn())?.into_dyn()));
        }
        for (k, v) in &run.cache {
            inputs.push((k.clone().into(), ort::value::Value::from_array(v.clone())?.into_dyn()));
        }
        let out = self.dec.run(inputs)?;
        let logits = out.get("logits").context("decoder logits")?.try_extract_array::<f32>()?;
        let last = logits.shape()[1] - 1;
        let row = logits.slice(ndarray::s![0, last, ..]).to_vec();
        for i in 0..LAYERS {
            for a in ["decoder", "encoder"] {
                // The cross-attention cache is fixed after the first step.
                if cached && a == "encoder" {
                    continue;
                }
                for kv in ["key", "value"] {
                    if let Some(t) = out.get(&format!("present.{i}.{a}.{kv}")) {
                        run.cache.insert(format!("past_key_values.{i}.{a}.{kv}"), t.try_extract_array::<f32>()?.to_owned());
                    }
                }
            }
        }
        run.steps += 1;
        Ok(row)
    }

    /// Greedy decoding from `token`, up to `max` new tokens. Returns them with their log-probabilities.
    fn greedy(&mut self, run: &mut Run, mut token: i64, max: usize) -> Result<(Vec<i64>, Vec<f32>)> {
        let (mut ids, mut lps) = (vec![], vec![]);
        let mut repeats = 0;
        for _ in 0..max {
            let logits = self.step(run, token)?;
            let next = argmax(&logits);
            if next as i64 == EOS {
                break;
            }
            repeats = if ids.last() == Some(&(next as i64)) { repeats + 1 } else { 0 };
            if repeats > 6 {
                break;
            }
            lps.push(log_softmax_at(&logits, next));
            ids.push(next as i64);
            token = next as i64;
        }
        Ok((ids, lps))
    }

    fn max_tokens(audio: &[f32]) -> usize {
        ((audio.len() as f32 / 16_000.0) * TOKENS_PER_SEC).ceil() as usize + 4
    }

    pub fn transcribe(&mut self, audio: &[f32]) -> Result<String> {
        let mut run = self.start(audio)?;
        let (ids, _) = self.greedy(&mut run, START, Self::max_tokens(audio))?;
        Ok(self.tok.decode(&ids))
    }

    /// Ways the decoder may write the phrase: "Hey bleep", "Hey, bleep", "hey bleep".
    pub fn spellings(phrase: &str) -> Vec<String> {
        let words: Vec<&str> = phrase.split_whitespace().collect();
        if words.is_empty() {
            return vec![];
        }
        let cap = |w: &str| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        };
        let mut out = vec![];
        let plain = std::iter::once(cap(words[0])).chain(words[1..].iter().map(|w| w.to_string())).collect::<Vec<_>>().join(" ");
        out.push(plain.clone());
        if words.len() > 1 {
            out.push(std::iter::once(format!("{},", cap(words[0]))).chain(words[1..].iter().map(|w| w.to_string())).collect::<Vec<_>>().join(" "));
            out.push(words.iter().map(|w| cap(w)).collect::<Vec<_>>().join(" "));
        }
        out.push(words.join(" "));
        out.dedup();
        out
    }

    /// Scores the phrase against the audio. When the gap is within `max_gap`, also decodes
    /// what follows the phrase (`rest` is then Some, maybe empty).
    pub fn spot(&mut self, audio: &[f32], phrase: &str, max_gap: f32) -> Result<Spot> {
        let max = Self::max_tokens(audio);
        let mut run = self.start(audio)?;
        let (ids, lps) = self.greedy(&mut run, START, max)?;
        let heard = self.tok.decode(&ids);
        // (gap, phrase tokens, the run with all but the last phrase token fed)
        let mut best: Option<(f32, Vec<i64>, Run)> = None;
        for sp in Self::spellings(phrase) {
            let want = self.tok.encode(&sp);
            if want.is_empty() {
                continue;
            }
            // Fresh cache; the encoder output is reused.
            let mut forced = Run { hidden: run.hidden.clone(), mask: run.mask.clone(), cache: empty_cache(), steps: 0 };
            let mut lp = 0.0;
            let mut prev = START;
            for &t in &want {
                lp += log_softmax_at(&self.step(&mut forced, prev)?, t as usize);
                prev = t;
            }
            let n = want.len();
            // The best guess over the same number of tokens is the yardstick.
            let guess: f32 = lps.iter().take(n).sum::<f32>() + n.saturating_sub(lps.len()) as f32 * -1.0;
            let gap = ((guess - lp) / n as f32).max(0.0);
            if best.as_ref().is_none_or(|(g, ..)| gap < *g) {
                best = Some((gap, want, forced));
            }
        }
        let Some((gap, want, mut forced)) = best else { return Ok(Spot { heard, gap: f32::MAX, rest: None }) };
        let rest = if gap <= max_gap {
            let (more, _) = self.greedy(&mut forced, *want.last().unwrap(), max)?;
            let mut all = want.clone();
            all.extend(more);
            let (full, head) = (self.tok.decode(&all), self.tok.decode(&want));
            Some(full.strip_prefix(&head).unwrap_or(&full).trim().to_string())
        } else {
            None
        };
        Ok(Spot { heard, gap, rest })
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub fn say(voice: &str, text: &str) -> Vec<f32> {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("s.wav");
        let mut c = std::process::Command::new("say");
        if !voice.is_empty() {
            c.args(["-v", voice]);
        }
        assert!(c.args(["-o", out.to_str().unwrap(), "--data-format=LEF32@16000", text]).status().unwrap().success());
        let bytes = std::fs::read(&out).unwrap();
        let at = bytes.windows(4).position(|w| w == b"data").unwrap() + 8;
        let mut audio = vec![0.0; 4000];
        audio.extend(bytes[at..].chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])));
        audio.extend(vec![0.0; 4000]);
        audio
    }

    pub fn model() -> Option<Moonshine> {
        let root = crate::paths::BleepPaths::from_env().root.join("models");
        let dir = crate::voice::model::find(&root, &crate::voice::model::whisprcatch_dir().into_iter().collect::<Vec<_>>())?;
        Some(Moonshine::load(&dir).unwrap())
    }

    pub const VOICES: &[&str] = &["", "Samantha", "Daniel", "Karen", "Moira", "Rishi", "Tessa", "Aman", "Fred", "Reed (English (US))", "Sandy (English (US))", "Shelley (English (US))", "Flo (English (US))"];

    /// Needs the model: `cargo test --lib voice::moonshine -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn tokenizer_round_trips_real_transcripts() {
        let Some(mut m) = model() else { return };
        for text in ["Hey bleep", "What is on my calendar today?", "Draft a reply to Priya, saying yes."] {
            let ids = m.tok.encode(text);
            assert_eq!(m.tok.decode(&ids), text);
            let heard = m.transcribe(&say("Samantha", text)).unwrap();
            eprintln!("{text:?} -> {ids:?}, heard {heard:?}");
        }
    }

    /// Prints the gap for the phrase said in many voices, and for other speech.
    #[test]
    #[ignore]
    fn calibrate() {
        let Some(mut m) = model() else { return };
        let phrase = std::env::var("PHRASE").unwrap_or("hey bleep".into());
        let pos: Vec<String> = std::env::var("POS").unwrap_or("Hey bleep|Hey, bleep!|Hey bleep, what time is it?".into()).split('|').map(String::from).collect();
        let neg = ["I believe", "Hey, please.", "Hey there", "Please", "I need some sleep", "Let me check the build", "Hey, how are you?", "Beep beep", "Hey Siri", "OK, sounds good", "Hey, look at this", "Able to help", "The blip on the radar", "Hey, Bree"];
        let mut worst_pos = 0f32;
        let mut best_neg = f32::MAX;
        for v in VOICES {
            for p in &pos {
                let s = m.spot(&say(v, p), &phrase, f32::MAX).unwrap();
                worst_pos = worst_pos.max(s.gap);
                eprintln!("POS {v:>22} | {p:<28} | gap {:.2} | heard {:?} rest {:?}", s.gap, s.heard, s.rest);
            }
        }
        for v in ["", "Samantha", "Daniel", "Rishi", "Karen"] {
            for n in neg {
                let s = m.spot(&say(v, n), &phrase, -1.0).unwrap();
                best_neg = best_neg.min(s.gap);
                eprintln!("NEG {v:>22} | {n:<28} | gap {:.2} | heard {:?}", s.gap, s.heard);
            }
        }
        eprintln!("worst positive gap {worst_pos:.2}, best negative gap {best_neg:.2}");
    }

    #[test]
    fn spellings_cover_case_and_comma() {
        assert_eq!(Moonshine::spellings("hey bleep"), vec!["Hey bleep", "Hey, bleep", "Hey Bleep", "hey bleep"]);
        assert_eq!(Moonshine::spellings("jarvis"), vec!["Jarvis", "jarvis"]);
    }

    #[test]
    fn log_softmax_and_argmax() {
        let l = [1.0, 3.0, 2.0];
        assert_eq!(argmax(&l), 1);
        let p: f32 = (0..3).map(|i| log_softmax_at(&l, i).exp()).sum();
        assert!((p - 1.0).abs() < 1e-5);
    }
}
