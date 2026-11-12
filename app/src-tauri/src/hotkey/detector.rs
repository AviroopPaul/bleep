//! Double tap of Option, as a pure state machine (build plan 8.8).

/// A tap is Option down then up within this, with no other key between.
pub const TAP_MAX_MS: u64 = 300;
/// Two taps whose releases are at most this far apart make a double tap.
pub const DOUBLE_MAX_MS: u64 = 400;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum KeyEv {
    OptionDown,
    OptionUp,
    OtherKey,
}

#[derive(Default)]
pub struct OptionTapDetector {
    down_at: Option<u64>,
    dirty: bool,
    last_tap_up: Option<u64>,
}

impl OptionTapDetector {
    /// Feed one key event; true when a double tap completes. State resets after a hit.
    pub fn on(&mut self, ev: KeyEv, now_ms: u64) -> bool {
        match ev {
            KeyEv::OptionDown => {
                self.down_at = Some(now_ms);
                self.dirty = false;
                false
            }
            KeyEv::OtherKey => {
                self.dirty = true;
                self.last_tap_up = None;
                false
            }
            KeyEv::OptionUp => {
                let Some(down) = self.down_at.take() else { return false };
                if self.dirty || now_ms.saturating_sub(down) > TAP_MAX_MS {
                    self.last_tap_up = None;
                    return false;
                }
                match self.last_tap_up {
                    Some(prev) if now_ms.saturating_sub(prev) <= DOUBLE_MAX_MS => {
                        self.last_tap_up = None;
                        true
                    }
                    _ => {
                        self.last_tap_up = Some(now_ms);
                        false
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use KeyEv::*;

    fn run(seq: &[(KeyEv, u64)]) -> Vec<bool> {
        let mut d = OptionTapDetector::default();
        seq.iter().map(|&(e, t)| d.on(e, t)).collect()
    }

    #[test]
    fn double_tap_within_400ms_hits() {
        assert_eq!(run(&[(OptionDown, 0), (OptionUp, 80), (OptionDown, 200), (OptionUp, 280)]), [false, false, false, true]);
    }

    #[test]
    fn slow_second_tap_misses() {
        assert_eq!(run(&[(OptionDown, 0), (OptionUp, 80), (OptionDown, 520), (OptionUp, 600)]).last(), Some(&false));
    }

    #[test]
    fn other_key_between_misses() {
        assert_eq!(run(&[(OptionDown, 0), (OptionUp, 80), (OtherKey, 120), (OptionDown, 200), (OptionUp, 280)]).last(), Some(&false));
    }

    #[test]
    fn option_used_as_modifier_misses() {
        // Option held while another key is pressed (for example Option+E) is not a tap.
        assert_eq!(run(&[(OptionDown, 0), (OtherKey, 40), (OptionUp, 80), (OptionDown, 200), (OptionUp, 280)]).last(), Some(&false));
    }

    #[test]
    fn held_option_is_not_a_tap() {
        assert_eq!(run(&[(OptionDown, 0), (OptionUp, 500), (OptionDown, 600), (OptionUp, 650)]).last(), Some(&false));
    }

    #[test]
    fn resets_after_hit() {
        let r = run(&[(OptionDown, 0), (OptionUp, 80), (OptionDown, 200), (OptionUp, 280), (OptionDown, 380), (OptionUp, 440)]);
        assert_eq!(r[3], true);
        assert_eq!(r[5], false, "the third tap alone must not hit again");
    }

    #[test]
    fn triple_tap_then_fourth_hits_once_more() {
        let r = run(&[
            (OptionDown, 0), (OptionUp, 80), (OptionDown, 200), (OptionUp, 280),
            (OptionDown, 380), (OptionUp, 440), (OptionDown, 520), (OptionUp, 600),
        ]);
        assert_eq!(r.iter().filter(|&&h| h).count(), 2);
    }
}
