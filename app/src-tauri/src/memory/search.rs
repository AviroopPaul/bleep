//! memory_search: daily-note lines and old chat through SQLite FTS5. Every word of the
//! query is quoted, so FTS5 syntax in it (`"`, `*`, `-`, `NEAR`, `OR`) is plain text.
use crate::store::{Role, Store};
use serde::Serialize;

pub const MAX_HITS: usize = 10;
pub const HIT_CHARS: usize = 200;
const MAX_WORDS: usize = 12;

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Hit {
    pub date: String,
    pub time: String,
    /// "note" or "chat".
    pub source: &'static str,
    /// For chat hits: "user" or "bleep".
    pub who: Option<&'static str>,
    /// Notes and old chat can quote email, so the model treats this as data.
    #[serde(rename = "text_untrusted")]
    pub text: String,
}

fn words(q: &str) -> Vec<String> {
    q.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).take(MAX_WORDS).map(|w| w.to_lowercase()).collect()
}

/// `"a" "b"` (all words), or `None` when the query has no words.
pub fn fts_query(q: &str) -> Option<String> {
    let w = words(q);
    (!w.is_empty()).then(|| w.iter().map(|w| format!("\"{w}\"")).collect::<Vec<_>>().join(" "))
}

fn any_query(q: &str) -> Option<String> {
    let w = words(q);
    (!w.is_empty()).then(|| w.iter().map(|w| format!("\"{w}\"")).collect::<Vec<_>>().join(" OR "))
}

fn cut(s: &str) -> String {
    let one = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() > HIT_CHARS {
        format!("{}…", one.chars().take(HIT_CHARS).collect::<String>())
    } else {
        one
    }
}

fn run(store: &Store, fts: &str) -> anyhow::Result<Vec<Hit>> {
    let notes = store.search_notes(fts, MAX_HITS as u32)?;
    let chat = store.search_messages(fts, MAX_HITS as u32)?;
    let mut notes = notes.into_iter().map(|n| Hit { date: n.date, time: n.time, source: "note", who: None, text: cut(&n.text) });
    let mut chat = chat.into_iter().map(|m| Hit {
        date: m.created_at.get(..10).unwrap_or("").to_string(),
        time: m.created_at.get(11..16).unwrap_or("").to_string(),
        source: "chat",
        who: Some(if m.role == Role::User { "user" } else { "bleep" }),
        text: cut(&m.content),
    });
    // Best note, best chat line, next note, ...: notes are what bleep chose to keep.
    let mut out = vec![];
    loop {
        let (a, b) = (notes.next(), chat.next());
        if a.is_none() && b.is_none() {
            break;
        }
        out.extend(a);
        out.extend(b);
    }
    out.truncate(MAX_HITS);
    Ok(out)
}

/// All words first; when that finds nothing, any word.
pub fn search(store: &Store, q: &str) -> anyhow::Result<Vec<Hit>> {
    let Some(all) = fts_query(q) else { return Ok(vec![]) };
    let hits = run(store, &all)?;
    if !hits.is_empty() {
        return Ok(hits);
    }
    match any_query(q) {
        Some(any) if any != all => run(store, &any),
        _ => Ok(hits),
    }
}

#[cfg(test)]
#[path = "search_tests.rs"]
mod tests;
