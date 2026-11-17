//! Speech to text for the listener: Moonshine on the Mac.
use super::listener::Stt;
use super::moonshine::Moonshine;
use anyhow::Result;

/// Moonshine refuses clips under 0.1 s or over 64 s.
const MIN: usize = 16_000 / 10;
const MAX: usize = 16_000 * 60;

impl Stt for Moonshine {
    fn transcribe(&mut self, audio: &[f32]) -> Result<String> {
        if audio.len() < MIN {
            return Ok(String::new());
        }
        Moonshine::transcribe(self, &audio[..audio.len().min(MAX)])
    }

    fn spot(&mut self, audio: &[f32], phrase: &str, max_gap: f32) -> Result<(String, Option<String>)> {
        if audio.len() < MIN {
            return Ok((String::new(), None));
        }
        let audio = &audio[..audio.len().min(MAX)];
        let s = Moonshine::spot(self, audio, phrase, max_gap)?;
        Ok((s.heard, s.rest))
    }
}

#[cfg(test)]
mod tests {
    use crate::voice::listener::{max_gap, Action, Listener, Stt};
    use crate::voice::moonshine::tests::{model, say};
    use crate::voice::vad::Vad;

    /// The real model on real speech, end to end through the VAD and the listener.
    /// Needs the model on disk: `cargo test --lib voice::stt -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn real_model_wakes_and_takes_the_request() {
        let Some(mut m) = model() else {
            eprintln!("no speech model on this Mac; skipped");
            return;
        };
        let gap = || vec![0.0f32; 16_000];
        for (voice, sens) in [("", "normal"), ("Daniel", "normal"), ("Karen", "normal"), ("Rishi", "normal"), ("Moira", "normal"), ("Samantha", "loose")] {
            let mut audio = gap();
            audio.extend(say(voice, "Hey bleep."));
            audio.extend(gap());
            audio.extend(say(voice, "What is on my calendar today?"));
            audio.extend(gap());
            audio.extend(gap());
            let (mut vad, mut l) = (Vad::new(), Listener::new("hey bleep", max_gap(sens)));
            let mut got = vec![];
            let t = std::time::Instant::now();
            for chunk in audio.chunks(512) {
                vad.set_pause_ms(l.pause_ms());
                for ev in vad.push(chunk) {
                    got.extend(l.on_event(ev, &mut m));
                }
            }
            let s = m.spot(&say(voice, "Hey bleep."), "hey bleep", -1.0).unwrap();
            eprintln!("{voice:>10}: {got:?} in {:?} (gap {:.2}, heard {:?})", t.elapsed(), s.gap, s.heard);
            assert_eq!(got.first(), Some(&Action::Wake), "{voice}");
            match got.get(1) {
                Some(Action::Heard(t)) => assert!(t.to_lowercase().contains("calendar"), "{t}"),
                other => panic!("{voice}: {other:?}"),
            }
        }
        // One breath.
        let (heard, hit) = Stt::spot(&mut m, &say("Daniel", "Hey bleep, what time is it?"), "hey bleep", max_gap("normal")).unwrap();
        eprintln!("one breath: {heard:?} -> {hit:?}");
        assert!(hit.unwrap().to_lowercase().contains("what time"));
        for other in ["Let me check the build first.", "I believe so.", "I need some sleep tonight."] {
            let (heard, hit) = Stt::spot(&mut m, &say("Samantha", other), "hey bleep", max_gap("normal")).unwrap();
            assert!(hit.is_none(), "{other} heard as {heard}");
        }
    }
}
