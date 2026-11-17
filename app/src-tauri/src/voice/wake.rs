//! Finds the wake phrase in a transcript. Speech to text spells a made-up word like
//! "bleep" in many ways ("blip", "bleep.", "Hey, Blip!"), so the match is fuzzy:
//! an edit distance on the letters, or the same sound shape (vowel runs folded).

pub const DEFAULT_PHRASE: &str = "hey bleep";
const MAX_PHRASE_CHARS: usize = 40;

/// A word of the transcript: its normalized letters and where it ends in the original.
#[derive(Debug, PartialEq)]
struct Word {
    norm: String,
    end: usize,
}

fn words(text: &str) -> Vec<Word> {
    let mut out = vec![];
    let mut cur = String::new();
    let mut end = 0;
    for (i, c) in text.char_indices() {
        if c.is_alphanumeric() || c == '\'' {
            for l in c.to_lowercase() {
                if l != '\'' {
                    cur.push(l);
                }
            }
            end = i + c.len_utf8();
        } else if !cur.is_empty() {
            out.push(Word { norm: std::mem::take(&mut cur), end });
        }
    }
    if !cur.is_empty() {
        out.push(Word { norm: cur, end });
    }
    out
}

/// Lowercase words joined by single spaces. What Settings saves and shows.
pub fn normalize(phrase: &str) -> String {
    words(phrase).into_iter().map(|w| w.norm).collect::<Vec<_>>().join(" ")
}

/// A wake phrase the listener can use: one to four words, letters only, not too short.
pub fn validate(phrase: &str) -> Result<String, String> {
    let p = normalize(phrase);
    let n = p.split(' ').filter(|w| !w.is_empty()).count();
    if p.chars().filter(|c| c.is_alphabetic()).count() < 4 {
        return Err("Use a phrase with at least 4 letters.".into());
    }
    if n > 4 || p.len() > MAX_PHRASE_CHARS {
        return Err("Keep the wake phrase to 4 words or fewer.".into());
    }
    Ok(p)
}

fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1; b.len() + 1];
        for (j, cb) in b.iter().enumerate() {
            cur[j + 1] = (prev[j] + usize::from(ca != cb)).min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        prev = cur;
    }
    prev[b.len()]
}

/// Sound shape: each run of vowels becomes one "a", doubled consonants fold.
/// "bleep", "blip" and "blep" all become "blap".
fn sound(s: &str) -> String {
    let mut out = String::new();
    let mut last: Option<char> = None;
    for (i, c) in s.chars().enumerate() {
        let vowel = matches!(c, 'a' | 'e' | 'i' | 'o' | 'u') || (c == 'y' && i > 0);
        let c = if vowel { 'a' } else { c };
        if last != Some(c) {
            out.push(c);
        }
        last = Some(c);
    }
    out
}

/// Allowed letter edits: one per four letters, at least one.
fn tolerance(chars: usize) -> usize {
    (chars / 4).max(1)
}

/// Where the wake phrase ends in `transcript` (a byte index), or None.
/// The phrase may sit anywhere ("um, hey bleep"). Spaces do not count ("heybleep").
pub fn find(transcript: &str, phrase: &str) -> Option<usize> {
    let want: String = normalize(phrase).split(' ').collect();
    if want.is_empty() {
        return None;
    }
    let n = normalize(phrase).split(' ').count();
    let ws = words(transcript);
    let tol = tolerance(want.chars().count());
    let want_sound = sound(&want);
    let mut best: Option<(usize, usize)> = None; // (distance, end)
    for i in 0..ws.len() {
        for len in [n, n + 1, n.saturating_sub(1)] {
            if len == 0 || i + len > ws.len() {
                continue;
            }
            let got: String = ws[i..i + len].iter().map(|w| w.norm.as_str()).collect();
            let d = levenshtein(&got, &want);
            let hit = if d <= tol { Some(d) } else if sound(&got) == want_sound { Some(tol) } else { None };
            if let Some(d) = hit {
                let end = ws[i + len - 1].end;
                if best.is_none_or(|(bd, _)| d < bd) {
                    best = Some((d, end));
                }
            }
        }
        if best.is_some_and(|(d, _)| d == 0) {
            break;
        }
    }
    best.map(|(_, end)| end)
}

/// What follows the wake phrase, trimmed of joining punctuation ("Hey bleep, what's up?" gives "what's up?").
pub fn remainder(transcript: &str, end: usize) -> String {
    transcript[end..].trim_start_matches(|c: char| c.is_whitespace() || matches!(c, ',' | '.' | '!' | '?' | ':' | ';' | '-')).trim().to_string()
}

/// Strips a wake phrase at the start of a command, for when the user says it twice.
pub fn strip_leading(text: &str, phrase: &str) -> String {
    match find(text, phrase) {
        Some(end) if words(&text[..end]).len() <= normalize(phrase).split(' ').count() + 1 => remainder(text, end),
        _ => text.trim().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(t: &str) -> bool {
        find(t, DEFAULT_PHRASE).is_some()
    }

    #[test]
    fn exact_and_punctuated_forms_match() {
        for t in ["hey bleep", "Hey, bleep!", "Hey Bleep.", "HEY BLEEP", "heybleep", "Hey bleep what's on my calendar"] {
            assert!(hit(t), "{t}");
        }
    }

    #[test]
    fn common_mishearings_match() {
        for t in ["Hey blip.", "Hey, bleeps.", "Hey Blip!", "Hey bleed", "hey blep", "Hay bleep", "Hey, Bleep!"] {
            assert!(hit(t), "{t}");
        }
    }

    #[test]
    fn phrase_inside_a_sentence_matches() {
        assert!(hit("Um, hey bleep, are you there?"));
    }

    #[test]
    fn other_speech_does_not_match() {
        for t in ["", "hello", "hey there", "I need some sleep tonight", "hey black", "Let me check the build", "beep beep", "keep it up", "hey, how are you?"] {
            assert!(!hit(t), "{t}");
        }
    }

    #[test]
    fn remainder_after_the_phrase() {
        let t = "Hey bleep, what's on my calendar today?";
        let end = find(t, DEFAULT_PHRASE).unwrap();
        assert_eq!(remainder(t, end), "what's on my calendar today?");
        let t = "Hey blip.";
        assert_eq!(remainder(t, find(t, DEFAULT_PHRASE).unwrap()), "");
    }

    #[test]
    fn custom_phrases_work() {
        assert!(find("Okay computer, play music", "okay computer").is_some());
        assert!(find("OK computer", "okay computer").is_some(), "ok vs okay is within the edit budget");
        assert!(find("hello world", "okay computer").is_none());
        let t = "Jarvis. Open my mail";
        assert_eq!(remainder(t, find(t, "jarvis").unwrap()), "Open my mail");
        assert!(find("Jarvus", "jarvis").is_some(), "one letter off still counts");
        assert!(find("service", "jarvis").is_none());
    }

    #[test]
    fn strip_leading_removes_a_repeated_phrase_only_at_the_start() {
        assert_eq!(strip_leading("Hey bleep, what time is it?", DEFAULT_PHRASE), "what time is it?");
        assert_eq!(strip_leading("what time is it?", DEFAULT_PHRASE), "what time is it?");
        assert_eq!(strip_leading("tell my team I said hey bleep", DEFAULT_PHRASE), "tell my team I said hey bleep");
    }

    #[test]
    fn validate_cleans_and_bounds_the_phrase() {
        assert_eq!(validate("  Hey,  Bleep! ").unwrap(), "hey bleep");
        assert!(validate("hi").is_err());
        assert!(validate("one two three four five").is_err());
        assert_eq!(validate("Jarvis").unwrap(), "jarvis");
    }

    #[test]
    fn sound_folds_vowel_runs() {
        assert_eq!(sound("bleep"), "blap");
        assert_eq!(sound("blip"), "blap");
        assert_eq!(sound("black"), "black");
    }
}
