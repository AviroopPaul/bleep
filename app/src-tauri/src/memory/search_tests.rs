use super::*;
use crate::store::{Kind, MsgStatus, NewMessage, Role, Store};

fn store() -> (tempfile::TempDir, Store) {
    let d = tempfile::tempdir().unwrap();
    let s = Store::open(&d.path().join("b.db")).unwrap();
    (d, s)
}

fn say(s: &Store, role: Role, text: &str) {
    s.insert_message(NewMessage { role, kind: Kind::Chat, content: text.into(), status: MsgStatus::Done, session_id: None }).unwrap();
}

#[test]
fn query_quotes_every_word() {
    assert_eq!(fts_query("sync priya").unwrap(), r#""sync" "priya""#);
    assert_eq!(fts_query("1:1 with Priya's").unwrap(), r#""1" "1" "with" "priya" "s""#);
    assert_eq!(fts_query("café über").unwrap(), r#""café" "über""#);
}

#[test]
fn query_syntax_is_plain_text() {
    for q in [r#"a" OR "b"#, "NEAR(a b)", "-mango *", "x AND NOT y", "col:val", "^start"] {
        let f = fts_query(q).unwrap();
        assert!(f.split(' ').all(|w| w.starts_with('"') && w.ends_with('"')), "{q} -> {f}");
    }
    assert_eq!(fts_query(""), None);
    assert_eq!(fts_query("  *** -- \"\" "), None);
}

#[test]
fn search_never_errors_on_odd_input() {
    let (_d, s) = store();
    say(&s, Role::User, "Sync means my 1:1 with Priya");
    for q in [r#"a" OR "b"#, "NEAR(", "-", "*", "OR", "\"", "", "''", "a\nb", "a\0b"] {
        assert!(search(&s, q).is_ok(), "{q:?}");
    }
    assert!(search(&s, "*").unwrap().is_empty(), "punctuation never matches everything");
}

#[test]
fn search_merges_notes_and_chat() {
    let (_d, s) = store();
    say(&s, Role::User, "Sync means my 1:1 with Priya");
    say(&s, Role::Assistant, "Got it, sync is your 1:1 with Priya.");
    s.index_note("2026-11-15", "10:02", "Sync is the Priya 1:1 on Tuesdays").unwrap();
    let h = search(&s, "sync priya").unwrap();
    assert_eq!(h.len(), 3);
    assert_eq!((h[0].source, h[0].date.as_str(), h[0].time.as_str()), ("note", "2026-11-15", "10:02"));
    assert_eq!(h[1].source, "chat");
    assert!(h.iter().any(|x| x.who == Some("user")) && h.iter().any(|x| x.who == Some("bleep")));
}

#[test]
fn all_words_first_then_any_word() {
    let (_d, s) = store();
    say(&s, Role::User, "Sync means my 1:1 with Priya");
    say(&s, Role::User, "The kiwi got ripe");
    let h = search(&s, "when is my sync with priya").unwrap();
    assert_eq!(h.len(), 1, "AND finds nothing (\"when\"), OR finds the sync line: {h:?}");
    assert!(h[0].text.contains("Priya"));
}

#[test]
fn search_caps_hits_and_text() {
    let (_d, s) = store();
    for i in 0..30 {
        say(&s, Role::User, &format!("mango {i} {}", "x".repeat(400)));
    }
    let h = search(&s, "mango").unwrap();
    assert_eq!(h.len(), MAX_HITS);
    assert!(h.iter().all(|x| x.text.chars().count() <= HIT_CHARS + 1));
}
