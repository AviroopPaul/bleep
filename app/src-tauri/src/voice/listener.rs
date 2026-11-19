//! The wake word state machine. Waits for the phrase, then takes the next thing
//! said as the request. "Hey bleep, what's on today?" in one breath works too.
use super::vad::{VadEvent, RATE};
use super::wake;

/// Speech to text. Moonshine in the app, a script in tests.
pub trait Stt: Send {
    fn transcribe(&mut self, audio: &[f32]) -> anyhow::Result<String>;
    /// What was heard, and on a wake phrase hit what follows it. `max_gap` is how far
    /// behind the model's best guess the phrase may score (see `moonshine::Spot`).
    fn spot(&mut self, audio: &[f32], phrase: &str, max_gap: f32) -> anyhow::Result<(String, Option<String>)>;
}

/// Only the start of a long utterance is checked for the phrase, to save work.
const HEAD: usize = RATE * 3;
/// After the phrase, how long bleep waits for the request to start.
pub const WAIT: u64 = RATE as u64 * 7;
pub const IDLE_PAUSE_MS: usize = 500;
pub const AWAKE_PAUSE_MS: usize = 1200;

/// How far behind the best guess the phrase may score, per token. Calibrated on 13 macOS
/// voices: the phrase scores 0 to 1 most of the time, everyday speech 4 to 7, and
/// near misses like "Hey, Bree" or "Hey, please" about 2.5.
pub fn max_gap(sensitivity: &str) -> f32 {
    match sensitivity {
        "strict" => 1.2,
        "loose" => 2.4,
        _ => 2.0,
    }
}

#[derive(Debug, PartialEq, Clone)]
pub enum Action {
    /// The phrase was heard. Open the chat and show that bleep listens.
    Wake,
    /// The request, as text.
    Heard(String),
    /// Woke, but nothing usable came after.
    GaveUp,
}

#[derive(Debug, PartialEq, Clone, Copy)]
enum State {
    Idle,
    Awake { since: u64, talking: bool },
}

pub struct Listener {
    phrase: String,
    max_gap: f32,
    state: State,
    /// The last thing transcribed, for the settings page.
    pub last_heard: Option<String>,
}

fn has_words(s: &str) -> bool {
    s.split_whitespace().any(|w| w.chars().filter(|c| c.is_alphanumeric()).count() >= 2)
}

/// Punctuation left over where the phrase was cut off: ", what time is it?" gives "What time is it?".
fn tidy(rest: &str) -> String {
    let t = rest.trim_start_matches(|c: char| c.is_whitespace() || matches!(c, ',' | '.' | '!' | '?' | ':' | ';' | '-')).trim();
    let mut c = t.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

impl Listener {
    pub fn new(phrase: &str, max_gap: f32) -> Self {
        Self { phrase: phrase.to_string(), max_gap, state: State::Idle, last_heard: None }
    }

    pub fn set_phrase(&mut self, phrase: &str, max_gap: f32) {
        self.phrase = phrase.to_string();
        self.max_gap = max_gap;
    }

    pub fn awake(&self) -> bool {
        matches!(self.state, State::Awake { .. })
    }

    /// The pause that ends an utterance in this state.
    pub fn pause_ms(&self) -> usize {
        if self.awake() { AWAKE_PAUSE_MS } else { IDLE_PAUSE_MS }
    }

    fn note(&mut self, text: &str) {
        if !text.trim().is_empty() {
            self.last_heard = Some(text.to_string());
        }
    }

    fn spot(&mut self, stt: &mut dyn Stt, audio: &[f32]) -> Option<String> {
        match stt.spot(audio, &self.phrase, self.max_gap) {
            Ok((heard, hit)) => {
                self.note(&heard);
                hit
            }
            Err(e) => {
                eprintln!("bleep: speech to text: {e}");
                None
            }
        }
    }

    pub fn on_event(&mut self, ev: VadEvent, stt: &mut dyn Stt) -> Vec<Action> {
        match (ev, self.state) {
            (VadEvent::Start { .. }, State::Awake { since, .. }) => {
                self.state = State::Awake { since, talking: true };
                vec![]
            }
            (VadEvent::Start { .. }, State::Idle) => vec![],
            (VadEvent::End { at, audio }, State::Idle) => {
                let long = audio.len() > HEAD + RATE / 2;
                let Some(mut rest) = self.spot(stt, if long { &audio[..HEAD] } else { &audio }) else { return vec![] };
                if long {
                    rest = self.spot(stt, &audio).unwrap_or_default();
                }
                let rest = tidy(&rest);
                if has_words(&rest) {
                    vec![Action::Wake, Action::Heard(rest)]
                } else {
                    self.state = State::Awake { since: at, talking: false };
                    vec![Action::Wake]
                }
            }
            (VadEvent::End { audio, .. }, State::Awake { .. }) => {
                self.state = State::Idle;
                let text = stt.transcribe(&audio).unwrap_or_else(|e| {
                    eprintln!("bleep: speech to text: {e}");
                    String::new()
                });
                self.note(&text);
                let text = wake::strip_leading(&text, &self.phrase);
                if has_words(&text) { vec![Action::Heard(text)] } else { vec![Action::GaveUp] }
            }
        }
    }

    /// Gives up when nothing was said after the phrase.
    pub fn tick(&mut self, now: u64) -> Option<Action> {
        match self.state {
            State::Awake { since, talking: false } if now.saturating_sub(since) > WAIT => {
                self.state = State::Idle;
                Some(Action::GaveUp)
            }
            _ => None,
        }
    }

    /// Awake without the phrase: the chat's mic button. The next utterance is the request.
    pub fn wake_now(&mut self, at: u64) {
        self.state = State::Awake { since: at, talking: false };
    }

    /// Back to waiting for the phrase (bleep started talking, or the mic changed).
    pub fn reset(&mut self) {
        self.state = State::Idle;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Returns the scripted lines in order and records the audio lengths it got.
    /// A line is a hit when its text holds the phrase.
    struct Script(Vec<&'static str>, Vec<usize>);
    impl Stt for Script {
        fn transcribe(&mut self, audio: &[f32]) -> anyhow::Result<String> {
            self.1.push(audio.len());
            Ok(if self.0.is_empty() { String::new() } else { self.0.remove(0).to_string() })
        }
        fn spot(&mut self, audio: &[f32], phrase: &str, _: f32) -> anyhow::Result<(String, Option<String>)> {
            let t = self.transcribe(audio)?;
            let hit = wake::find(&t, phrase).map(|end| t[end..].to_string());
            Ok((t, hit))
        }
    }

    fn end(secs: f32, at: u64) -> VadEvent {
        VadEvent::End { at, audio: vec![0.1; (secs * RATE as f32) as usize] }
    }

    fn l() -> Listener {
        Listener::new("hey bleep", 2.0)
    }

    #[test]
    fn sensitivity_maps_to_a_gap() {
        assert!(max_gap("strict") < max_gap("normal") && max_gap("normal") < max_gap("loose"));
        assert_eq!(max_gap(""), max_gap("normal"));
    }

    #[test]
    fn tidy_drops_leading_punctuation() {
        assert_eq!(tidy(", what time is it?"), "What time is it?");
        assert_eq!(tidy("! What time"), "What time");
        assert_eq!(tidy("."), "");
    }

    #[test]
    fn other_talk_does_nothing() {
        let mut l = l();
        let mut s = Script(vec!["so anyway the build passed"], vec![]);
        assert!(l.on_event(end(2.0, 100), &mut s).is_empty());
        assert!(!l.awake());
        assert_eq!(l.last_heard.as_deref(), Some("so anyway the build passed"));
    }

    #[test]
    fn phrase_then_request() {
        let mut l = l();
        let mut s = Script(vec!["Hey bleep.", "What's on my calendar today?"], vec![]);
        assert_eq!(l.on_event(end(1.0, 16_000), &mut s), vec![Action::Wake]);
        assert!(l.awake());
        assert_eq!(l.pause_ms(), AWAKE_PAUSE_MS);
        assert!(l.on_event(VadEvent::Start { at: 20_000 }, &mut s).is_empty());
        assert_eq!(l.tick(16_000 + WAIT + 1), None, "no timeout once the user talks");
        assert_eq!(l.on_event(end(2.0, 60_000), &mut s), vec![Action::Heard("What's on my calendar today?".into())]);
        assert!(!l.awake());
    }

    #[test]
    fn mic_button_wakes_without_the_phrase() {
        let mut l = l();
        let mut s = Script(vec!["What's on my calendar today?"], vec![]);
        l.wake_now(1_000);
        assert!(l.awake());
        assert_eq!(l.on_event(end(2.0, 40_000), &mut s), vec![Action::Heard("What's on my calendar today?".into())]);
        assert!(!l.awake());
        l.wake_now(50_000);
        assert_eq!(l.tick(50_000 + WAIT + 1), Some(Action::GaveUp), "gives up when nothing is said");
    }

    #[test]
    fn phrase_and_request_in_one_breath() {
        let mut l = l();
        // Short enough to check once.
        let mut s = Script(vec!["Hey bleep, what time is it?"], vec![]);
        assert_eq!(l.on_event(end(2.5, 1), &mut s), vec![Action::Wake, Action::Heard("What time is it?".into())]);
        assert!(!l.awake());
        assert_eq!(s.1.len(), 1);
    }

    #[test]
    fn long_utterance_checks_the_head_first() {
        let mut l = l();
        let mut s = Script(vec!["Hey bleep, draft a", "Hey bleep, draft a reply to Priya saying yes"], vec![]);
        let got = l.on_event(end(6.0, 1), &mut s);
        assert_eq!(got, vec![Action::Wake, Action::Heard("Draft a reply to Priya saying yes".into())]);
        assert_eq!(s.1, vec![HEAD, 6 * RATE]);
        // Long talk without the phrase costs one short pass.
        let mut s = Script(vec!["we should ship this on"], vec![]);
        assert!(l.on_event(end(8.0, 1), &mut s).is_empty());
        assert_eq!(s.1, vec![HEAD]);
    }

    #[test]
    fn silence_after_the_phrase_gives_up() {
        let mut l = l();
        let mut s = Script(vec!["hey bleep"], vec![]);
        l.on_event(end(1.0, 1000), &mut s);
        assert_eq!(l.tick(1000 + WAIT - 1), None);
        assert_eq!(l.tick(1000 + WAIT + 1), Some(Action::GaveUp));
        assert!(!l.awake());
    }

    #[test]
    fn noise_after_the_phrase_gives_up() {
        let mut l = l();
        let mut s = Script(vec!["hey bleep", "."], vec![]);
        l.on_event(end(1.0, 1), &mut s);
        assert_eq!(l.on_event(end(1.0, 2), &mut s), vec![Action::GaveUp]);
    }

    #[test]
    fn repeated_phrase_is_stripped_from_the_request() {
        let mut l = l();
        let mut s = Script(vec!["hey bleep", "Hey bleep, open my mail"], vec![]);
        l.on_event(end(1.0, 1), &mut s);
        assert_eq!(l.on_event(end(2.0, 2), &mut s), vec![Action::Heard("open my mail".into())]);
    }

    #[test]
    fn custom_phrase_and_reset() {
        let mut l = l();
        l.set_phrase("jarvis", 2.0);
        let mut s = Script(vec!["hey bleep", "Jarvis", "Jarvis, check mail"], vec![]);
        assert!(l.on_event(end(1.0, 1), &mut s).is_empty(), "the old phrase no longer works");
        assert_eq!(l.on_event(end(1.0, 2), &mut s), vec![Action::Wake]);
        l.reset();
        assert!(!l.awake());
        assert_eq!(l.on_event(end(2.0, 6), &mut s), vec![Action::Wake, Action::Heard("Check mail".into())]);
    }
}
