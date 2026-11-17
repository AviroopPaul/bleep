//! Speaks replies with macOS's own voices (`say`): on the Mac, free, no download.
//! `say -a` picks the speaker, `say -v` the voice.
use serde::Serialize;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Voice {
    pub name: String,
    pub locale: String,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct OutputDevice {
    pub id: String,
    pub name: String,
}

/// Parses `say -v '?'`: "Daniel (English (UK)) en_GB    # Hello! ...". Duplicates dropped.
pub fn parse_voices(out: &str) -> Vec<Voice> {
    let re = regex::Regex::new(r"^(.+?)\s+([a-z]{2,3}_[A-Z0-9]{2,3})\s+#").unwrap();
    let mut v: Vec<Voice> = vec![];
    for line in out.lines() {
        if let Some(c) = re.captures(line) {
            let voice = Voice { name: c[1].trim().to_string(), locale: c[2].to_string() };
            if !v.contains(&voice) {
                v.push(voice);
            }
        }
    }
    v
}

/// Parses `say -a '?'`: "   81 MacBook Air Speakers".
pub fn parse_devices(out: &str) -> Vec<OutputDevice> {
    out.lines()
        .filter_map(|l| {
            let (id, name) = l.trim().split_once(char::is_whitespace)?;
            id.chars().all(|c| c.is_ascii_digit()).then(|| OutputDevice { id: id.into(), name: name.trim().into() })
        })
        .collect()
}

fn run(args: &[&str]) -> String {
    Command::new("say").args(args).output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default()
}

/// English voices first, the novelty ones (Bells, Bahh) left out.
pub fn voices() -> Vec<Voice> {
    const NOVELTY: &[&str] = &["Bad News", "Bahh", "Bells", "Boing", "Bubbles", "Cellos", "Good News", "Jester", "Organ", "Superstar", "Trinoids", "Whisper", "Wobble", "Zarvox", "Albert", "Fred", "Junior", "Kathy", "Ralph"];
    let mut v: Vec<Voice> = parse_voices(&run(&["-v", "?"])).into_iter().filter(|v| v.locale.starts_with("en_") && !NOVELTY.contains(&v.name.as_str())).collect();
    v.sort_by(|a, b| a.locale.cmp(&b.locale).then(a.name.cmp(&b.name)));
    v
}

pub fn output_devices() -> Vec<OutputDevice> {
    parse_devices(&run(&["-a", "?"]))
}

/// Markdown and links read aloud badly. Keep the words, drop the marks, and stop at ~600 chars.
pub fn speakable(md: &str) -> String {
    let code = regex::Regex::new(r"(?s)```.*?```").unwrap();
    let link = regex::Regex::new(r"\[([^\]]+)\]\([^)]*\)").unwrap();
    let url = regex::Regex::new(r"https?://\S+").unwrap();
    let marks = regex::Regex::new(r"[*_`#>|~]+").unwrap();
    let bullet = regex::Regex::new(r"(?m)^\s*(?:[-+]|\d+\.)\s+").unwrap();
    let s = code.replace_all(md, " ");
    let s = link.replace_all(&s, "$1");
    let s = url.replace_all(&s, "a link");
    let s = bullet.replace_all(&s, "");
    let s = marks.replace_all(&s, "");
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() <= 600 {
        return s;
    }
    // Cut at the last sentence end before the limit.
    let head: String = s.chars().take(600).collect();
    match head.rfind(['.', '!', '?']) {
        Some(i) if i > 200 => head[..=i].to_string(),
        _ => format!("{head}..."),
    }
}

/// The `say` arguments for one reply. Empty voice or device means the system's own.
pub fn args(text: &str, voice: &str, device: &str) -> Vec<String> {
    let mut a = vec![];
    if !voice.is_empty() {
        a.extend(["-v".into(), voice.to_string()]);
    }
    if !device.is_empty() {
        a.extend(["-a".into(), device.to_string()]);
    }
    a.push("--".into());
    a.push(text.to_string());
    a
}

/// The speaker to use: the chosen one while it is plugged in, else the system default ("").
pub fn pick_output(chosen: &str, available: &[OutputDevice]) -> String {
    if available.iter().any(|d| d.name == chosen) { chosen.to_string() } else { String::new() }
}

/// One voice at a time. A new reply, or the user talking, stops the old one.
#[derive(Clone, Default)]
pub struct Speaker {
    child: Arc<Mutex<Option<Child>>>,
    speaking: Arc<AtomicBool>,
}

impl Speaker {
    pub fn speak(&self, text: &str, voice: &str, device: &str) -> anyhow::Result<()> {
        self.stop();
        let text = speakable(text);
        if text.is_empty() {
            return Ok(());
        }
        let device = if device.is_empty() { String::new() } else { pick_output(device, &output_devices()) };
        let child = Command::new("say").args(args(&text, voice, &device)).stdout(Stdio::null()).stderr(Stdio::null()).spawn()?;
        *self.child.lock().unwrap() = Some(child);
        self.speaking.store(true, Ordering::SeqCst);
        Ok(())
    }

    pub fn stop(&self) {
        if let Some(mut c) = self.child.lock().unwrap().take() {
            let _ = c.kill();
            let _ = c.wait();
        }
        self.speaking.store(false, Ordering::SeqCst);
    }

    /// True while `say` runs. The listener ignores the mic then, so bleep does not hear itself.
    pub fn is_speaking(&self) -> bool {
        let mut slot = self.child.lock().unwrap();
        let done = match slot.as_mut() {
            Some(c) => !matches!(c.try_wait(), Ok(None)),
            None => true,
        };
        if done {
            slot.take();
            self.speaking.store(false, Ordering::SeqCst);
        }
        !done
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_voice_list() {
        let out = "Albert              en_US    # Hello! My name is Albert.\nAmélie              fr_CA    # Bonjour!\nDaniel (English (UK)) en_GB    # Hello! My name is Daniel.\nDaniel (English (UK)) en_GB    # Hello! My name is Daniel.\njunk line\n";
        let v = parse_voices(out);
        assert_eq!(v, vec![
            Voice { name: "Albert".into(), locale: "en_US".into() },
            Voice { name: "Amélie".into(), locale: "fr_CA".into() },
            Voice { name: "Daniel (English (UK))".into(), locale: "en_GB".into() },
        ]);
    }

    #[test]
    fn parses_device_list() {
        let d = parse_devices("   81 MacBook Air Speakers\n   73 EShareAudio\n\nnot a device\n");
        assert_eq!(d, vec![OutputDevice { id: "81".into(), name: "MacBook Air Speakers".into() }, OutputDevice { id: "73".into(), name: "EShareAudio".into() }]);
    }

    #[test]
    fn speakable_drops_markdown_and_links() {
        let md = "## Today\n- **10:00** Standup with [Priya](https://cal.example/x)\n- Lunch\n\n```\ncode here\n```\nSee https://example.com/a?b=c for more.";
        assert_eq!(speakable(md), "Today 10:00 Standup with Priya Lunch See a link for more.");
    }

    #[test]
    fn speakable_cuts_long_replies_at_a_sentence() {
        let long = "This is one sentence that goes on. ".repeat(40);
        let s = speakable(&long);
        assert!(s.len() <= 600 && s.ends_with('.'), "{s}");
    }

    #[test]
    fn args_use_system_defaults_when_empty() {
        assert_eq!(args("hi", "", ""), vec!["--", "hi"]);
        assert_eq!(args("-rf", "Samantha", "81"), vec!["-v", "Samantha", "-a", "81", "--", "-rf"]);
    }

    #[test]
    fn unplugged_speaker_falls_back_to_the_default() {
        let have = parse_devices("   81 MacBook Air Speakers\n   90 AirPods Pro\n");
        assert_eq!(pick_output("AirPods Pro", &have), "AirPods Pro");
        assert_eq!(pick_output("Studio Display", &have), "");
        assert_eq!(pick_output("", &have), "");
    }

    #[test]
    fn speaker_is_idle_until_it_speaks() {
        let s = Speaker::default();
        assert!(!s.is_speaking());
        s.stop();
        assert!(!s.is_speaking());
    }
}
