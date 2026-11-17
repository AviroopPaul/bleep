//! Cuts the mic stream into utterances by loudness, so speech to text only runs
//! when someone talks. 16 kHz mono in, whole utterances out. Time is counted in
//! samples, so tests need no clock.

pub const RATE: usize = 16_000;
/// 30 ms frames.
pub const FRAME: usize = RATE * 30 / 1000;
/// Kept from before speech starts, so the first sound is not cut.
const PREROLL_FRAMES: usize = 10;
/// Loud frames in a row that start an utterance (90 ms).
const START_FRAMES: usize = 3;
/// The quietest level that can count as speech, whatever the room noise.
const MIN_LEVEL: f32 = 0.006;
/// Speech must be this many times louder than the room.
const OVER_NOISE: f32 = 3.0;
/// An utterance is cut here even if the talking goes on.
pub const MAX_SAMPLES: usize = RATE * 15;
/// Shorter utterances are clicks and bumps, not words.
const MIN_VOICED_FRAMES: usize = 8;

#[derive(Debug, PartialEq)]
pub enum VadEvent {
    /// Speech started, `at` samples into the stream.
    Start { at: u64 },
    /// An utterance ended: its audio, including a short pre-roll.
    End { at: u64, audio: Vec<f32> },
}

pub struct Vad {
    pending: Vec<f32>,
    preroll: std::collections::VecDeque<Vec<f32>>,
    noise: f32,
    loud_run: usize,
    quiet_run: usize,
    voiced: usize,
    speech: Option<Vec<f32>>,
    /// Quiet frames that end an utterance.
    hangover: usize,
    clock: u64,
}

pub fn rms(frame: &[f32]) -> f32 {
    (frame.iter().map(|s| s * s).sum::<f32>() / frame.len().max(1) as f32).sqrt()
}

impl Default for Vad {
    fn default() -> Self {
        Self::new()
    }
}

impl Vad {
    pub fn new() -> Self {
        Self { pending: vec![], preroll: Default::default(), noise: MIN_LEVEL / OVER_NOISE, loud_run: 0, quiet_run: 0, voiced: 0, speech: None, hangover: hangover_frames(500), clock: 0 }
    }

    /// How long a pause ends an utterance. Longer after the wake phrase, when people stop to think.
    pub fn set_pause_ms(&mut self, ms: usize) {
        self.hangover = hangover_frames(ms);
    }

    pub fn in_speech(&self) -> bool {
        self.speech.is_some()
    }

    /// Samples seen so far.
    pub fn now(&self) -> u64 {
        self.clock
    }

    /// Drops any utterance in progress (while bleep itself is talking).
    pub fn reset(&mut self) {
        self.speech = None;
        self.loud_run = 0;
        self.quiet_run = 0;
        self.voiced = 0;
        self.preroll.clear();
    }

    fn threshold(&self) -> f32 {
        (self.noise * OVER_NOISE).max(MIN_LEVEL)
    }

    pub fn push(&mut self, samples: &[f32]) -> Vec<VadEvent> {
        self.pending.extend_from_slice(samples);
        let mut out = vec![];
        let mut at = 0;
        while self.pending.len() - at >= FRAME {
            let frame = self.pending[at..at + FRAME].to_vec();
            at += FRAME;
            self.clock += FRAME as u64;
            if let Some(ev) = self.frame(frame) {
                out.push(ev);
            }
        }
        self.pending.drain(..at);
        out
    }

    fn frame(&mut self, frame: Vec<f32>) -> Option<VadEvent> {
        let level = rms(&frame);
        let loud = level > self.threshold();
        match self.speech.as_mut() {
            None => {
                if loud {
                    self.loud_run += 1;
                } else {
                    self.loud_run = 0;
                    // The room level follows quiet frames only.
                    self.noise = self.noise * 0.95 + level * 0.05;
                }
                self.preroll.push_back(frame);
                while self.preroll.len() > PREROLL_FRAMES {
                    self.preroll.pop_front();
                }
                if self.loud_run >= START_FRAMES {
                    let buf: Vec<f32> = self.preroll.drain(..).flatten().collect();
                    self.speech = Some(buf);
                    self.voiced = self.loud_run;
                    self.quiet_run = 0;
                    self.loud_run = 0;
                    return Some(VadEvent::Start { at: self.clock });
                }
                None
            }
            Some(buf) => {
                buf.extend_from_slice(&frame);
                if loud {
                    self.voiced += 1;
                    self.quiet_run = 0;
                } else {
                    self.quiet_run += 1;
                }
                if self.quiet_run >= self.hangover || buf.len() >= MAX_SAMPLES {
                    let audio = self.speech.take().unwrap();
                    let voiced = self.voiced;
                    self.quiet_run = 0;
                    self.voiced = 0;
                    if voiced < MIN_VOICED_FRAMES {
                        return None;
                    }
                    return Some(VadEvent::End { at: self.clock, audio });
                }
                None
            }
        }
    }
}

fn hangover_frames(ms: usize) -> usize {
    (ms * RATE / 1000).div_ceil(FRAME).max(1)
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub fn tone(ms: usize, amp: f32) -> Vec<f32> {
        (0..ms * RATE / 1000).map(|i| amp * (i as f32 * 2.0 * std::f32::consts::PI * 220.0 / RATE as f32).sin()).collect()
    }

    pub fn hush(ms: usize) -> Vec<f32> {
        // A little room noise, not digital silence.
        (0..ms * RATE / 1000).map(|i| 0.001 * ((i * 7919 % 13) as f32 / 6.5 - 1.0)).collect()
    }

    fn ends(ev: &[VadEvent]) -> Vec<usize> {
        ev.iter().filter_map(|e| if let VadEvent::End { audio, .. } = e { Some(audio.len()) } else { None }).collect()
    }

    #[test]
    fn silence_gives_nothing() {
        let mut v = Vad::new();
        assert!(v.push(&hush(3000)).is_empty());
        assert!(!v.in_speech());
    }

    #[test]
    fn one_word_gives_one_utterance_with_preroll() {
        let mut v = Vad::new();
        let mut ev = v.push(&hush(1000));
        ev.extend(v.push(&tone(600, 0.2)));
        assert!(v.in_speech());
        ev.extend(v.push(&hush(800)));
        assert!(matches!(ev[0], VadEvent::Start { .. }));
        let lens = ends(&ev);
        assert_eq!(lens.len(), 1);
        // 600 ms of speech, the pre-roll before it and the pause after it.
        assert!(lens[0] >= 600 * RATE / 1000 && lens[0] <= (600 + 300 + 600) * RATE / 1000, "{}", lens[0]);
    }

    #[test]
    fn a_short_pause_does_not_split_but_a_long_one_does() {
        let mut v = Vad::new();
        let mut ev = v.push(&hush(500));
        ev.extend(v.push(&tone(400, 0.2)));
        ev.extend(v.push(&hush(200)));
        ev.extend(v.push(&tone(400, 0.2)));
        ev.extend(v.push(&hush(700)));
        ev.extend(v.push(&tone(400, 0.2)));
        ev.extend(v.push(&hush(700)));
        assert_eq!(ends(&ev).len(), 2);
    }

    #[test]
    fn longer_pause_setting_keeps_thinking_gaps_together() {
        let mut v = Vad::new();
        v.set_pause_ms(1200);
        let mut ev = v.push(&hush(500));
        ev.extend(v.push(&tone(400, 0.2)));
        ev.extend(v.push(&hush(800)));
        ev.extend(v.push(&tone(400, 0.2)));
        ev.extend(v.push(&hush(1500)));
        assert_eq!(ends(&ev).len(), 1);
    }

    #[test]
    fn a_click_is_dropped() {
        let mut v = Vad::new();
        let mut ev = v.push(&hush(500));
        ev.extend(v.push(&tone(120, 0.3)));
        ev.extend(v.push(&hush(800)));
        assert!(ends(&ev).is_empty());
    }

    #[test]
    fn endless_talk_is_cut_at_the_cap() {
        let mut v = Vad::new();
        let ev = v.push(&tone(20_000, 0.2));
        let lens = ends(&ev);
        assert!(!lens.is_empty() && lens[0] <= MAX_SAMPLES + FRAME);
    }

    #[test]
    fn steady_loud_room_raises_the_bar() {
        let mut v = Vad::new();
        // A fan at 0.004 RMS should not count as speech.
        let fan: Vec<f32> = (0..RATE * 3).map(|i| if i % 2 == 0 { 0.004 } else { -0.004 }).collect();
        assert!(ends(&v.push(&fan)).is_empty());
    }

    #[test]
    fn odd_sized_chunks_are_fine() {
        let mut v = Vad::new();
        let mut audio = hush(500);
        audio.extend(tone(600, 0.2));
        audio.extend(hush(800));
        let mut ev = vec![];
        for c in audio.chunks(333) {
            ev.extend(v.push(c));
        }
        assert_eq!(ends(&ev).len(), 1);
        assert_eq!(v.now() as usize, audio.len() / FRAME * FRAME);
    }
}
