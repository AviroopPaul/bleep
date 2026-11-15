//! bleep's memory on disk (build plan 8.7): USER.md, RULES.md and MEMORY.md go into the
//! system prompt; daily notes are only searched. The files are mine to edit, so bleep reads
//! them fresh, changes only the line it adds or removes, and writes atomically.

use crate::paths::{BleepPaths, MemFile};
use crate::store::{norm_fact, Store};
use chrono::{DateTime, Local, NaiveDate};
use serde::Serialize;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;

/// About 3K tokens for the three files together.
pub const PROMPT_BUDGET: usize = 12_000;
pub const MAX_FACT: usize = 300;
pub const CUT_NOTE: &str = "(Memory is longer than this; the rest was cut.)";
pub const FULL: &str = "Memory is full. Ask the user to forget something in Settings.";

#[derive(Clone, Debug, PartialEq)]
pub enum Added {
    Yes,
    Duplicate,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Forget {
    Removed,
    Gone,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct FileFacts {
    pub file: MemFile,
    pub facts: Vec<String>,
}

pub struct Memory {
    pub paths: BleepPaths,
    pub store: Arc<Store>,
}

fn starter(f: MemFile) -> &'static str {
    match f {
        MemFile::User => "# About me\n\n<!-- Who you are and the people who matter. One fact per line, starting with \"- \". -->\n",
        MemFile::Rules => "# My rules\n\n<!-- How bleep should behave. One rule per line, starting with \"- \". -->\n",
        MemFile::Memory => "# What bleep has learned\n\n<!-- Facts you approved. One per line, starting with \"- \". Edit freely. -->\n",
    }
}

/// The fact on a bullet line ("- x" or "* x"), if it is one.
fn bullet(line: &str) -> Option<&str> {
    let t = line.trim();
    let rest = t.strip_prefix("- ").or_else(|| t.strip_prefix("* "))?;
    let rest = rest.trim();
    (!rest.is_empty()).then_some(rest)
}

fn is_comment(line: &str) -> bool {
    let t = line.trim();
    t.starts_with("<!--") && t.ends_with("-->")
}

/// One line, trimmed, whitespace folded, at most `MAX_FACT` chars.
fn one_line(s: &str, what: &str) -> Result<String, String> {
    let t = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.is_empty() {
        return Err(format!("The {what} is empty."));
    }
    if t.chars().count() > MAX_FACT {
        return Err(format!("The {what} is longer than {MAX_FACT} characters. Make it shorter."));
    }
    Ok(t)
}

/// Writes through a symlink to its target, via a temp file and rename.
fn write_atomic(path: &std::path::Path, body: &str) -> io::Result<()> {
    let target = match std::fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => std::fs::canonicalize(path)?,
        _ => path.to_path_buf(),
    };
    let name = target.file_name().and_then(|n| n.to_str()).unwrap_or("memory");
    let tmp = target.with_file_name(format!(".{name}.bleep-tmp"));
    std::fs::write(&tmp, body)?;
    std::fs::rename(&tmp, &target)
}

impl Memory {
    pub fn new(paths: BleepPaths, store: Arc<Store>) -> Memory {
        Memory { paths, store }
    }

    fn path(&self, f: MemFile) -> PathBuf {
        self.paths.memory_file(f)
    }

    fn read(&self, f: MemFile) -> String {
        std::fs::read_to_string(self.path(f)).unwrap_or_default()
    }

    /// Creates each file with a heading and a hint, only when it is missing.
    pub fn ensure(&self) -> io::Result<()> {
        std::fs::create_dir_all(&self.paths.daily)?;
        for f in MemFile::ALL {
            let p = self.path(f);
            if std::fs::symlink_metadata(&p).is_err() {
                std::fs::write(&p, starter(f))?;
            }
        }
        Ok(())
    }

    /// The files for the system prompt: comments and empty files left out, cut at the budget.
    pub fn load_for_prompt(&self) -> String {
        let mut out = String::new();
        for f in MemFile::ALL {
            let raw = self.read(f);
            let body: Vec<&str> = raw.lines().filter(|l| !is_comment(l)).map(str::trim_end).collect();
            let has_content = body.iter().any(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'));
            if !has_content {
                continue;
            }
            let text = body.join("\n");
            let text = text.trim();
            if !out.is_empty() {
                out.push_str("\n\n");
            }
            out.push_str(&format!("## {}\n{}", f.file_name(), text));
        }
        if out.chars().count() > PROMPT_BUDGET {
            let cut: String = out.chars().take(PROMPT_BUDGET).collect();
            let cut = match cut.rfind('\n') {
                Some(i) => cut[..i].to_string(),
                None => cut,
            };
            return format!("{cut}\n{CUT_NOTE}");
        }
        out
    }

    pub fn size(&self) -> usize {
        self.load_for_prompt().chars().count()
    }

    pub fn facts(&self) -> Vec<FileFacts> {
        MemFile::ALL
            .into_iter()
            .map(|f| FileFacts { file: f, facts: self.read(f).lines().filter_map(bullet).map(String::from).collect() })
            .collect()
    }

    /// A fact as it would be saved, or why it cannot be.
    pub fn check_fact(&self, fact: &str) -> Result<String, String> {
        let t = fact.trim();
        let t = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")).unwrap_or(t);
        if t.contains(['\n', '\r']) {
            return Err("A fact must be one line.".into());
        }
        one_line(t, "fact")
    }

    pub fn is_known(&self, fact: &str) -> bool {
        let n = norm_fact(fact);
        self.facts().iter().flat_map(|f| f.facts.iter()).any(|x| norm_fact(x) == n)
    }

    /// Checks a fact for `file` without saving it: cleaned text, or why it cannot be saved.
    pub fn can_add(&self, fact: &str) -> Result<Added, String> {
        let t = self.check_fact(fact)?;
        if self.is_known(&t) {
            return Ok(Added::Duplicate);
        }
        if self.size() + t.chars().count() + 3 > PROMPT_BUDGET {
            return Err(FULL.into());
        }
        Ok(Added::Yes)
    }

    pub fn add_fact(&self, file: MemFile, fact: &str) -> Result<Added, String> {
        if self.can_add(fact)? == Added::Duplicate {
            return Ok(Added::Duplicate);
        }
        let t = self.check_fact(fact)?;
        let mut body = self.read(file);
        if !body.is_empty() && !body.ends_with('\n') {
            body.push('\n');
        }
        body.push_str(&format!("- {t}\n"));
        write_atomic(&self.path(file), &body).map_err(|e| format!("Could not save memory: {e}"))?;
        Ok(Added::Yes)
    }

    /// Removes the first bullet line whose fact is exactly `fact`. Every other byte stays.
    pub fn forget_fact(&self, file: MemFile, fact: &str) -> io::Result<Forget> {
        let body = match std::fs::read_to_string(self.path(file)) {
            Ok(b) => b,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Forget::Gone),
            Err(e) => return Err(e),
        };
        let want = fact.trim();
        let mut out = String::with_capacity(body.len());
        let mut removed = false;
        for line in body.split_inclusive('\n') {
            if !removed && bullet(line) == Some(want) {
                removed = true;
                continue;
            }
            out.push_str(line);
        }
        if !removed {
            return Ok(Forget::Gone);
        }
        write_atomic(&self.path(file), &out)?;
        Ok(Forget::Removed)
    }

    fn daily_path(&self, date: NaiveDate) -> PathBuf {
        self.paths.daily.join(format!("{}.md", date.format("%Y-%m-%d")))
    }

    pub fn daily_note(&self, date: NaiveDate) -> String {
        std::fs::read_to_string(self.daily_path(date)).unwrap_or_default()
    }

    /// Appends "- HH:MM text" to the day's note and indexes it.
    pub fn append_note(&self, at: DateTime<Local>, text: &str) -> Result<(), String> {
        use std::io::Write;
        let t = one_line(text, "note")?;
        let date = at.date_naive();
        let (d, hm) = (date.format("%Y-%m-%d").to_string(), at.format("%H:%M").to_string());
        let p = self.daily_path(date);
        let new = !p.exists();
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&p).map_err(|e| format!("Could not save the note: {e}"))?;
        let head = if new { format!("# {d}\n\n") } else { String::new() };
        f.write_all(format!("{head}- {hm} {t}\n").as_bytes()).map_err(|e| format!("Could not save the note: {e}"))?;
        self.store.index_note(&d, &hm, &t).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Rebuilds the note index from the daily files (I may have edited them). Returns lines indexed.
    pub fn reindex_notes(&self) -> anyhow::Result<usize> {
        self.store.clear_note_index()?;
        let mut n = 0;
        let Ok(dir) = std::fs::read_dir(&self.paths.daily) else { return Ok(0) };
        for e in dir.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let Some(stem) = name.strip_suffix(".md") else { continue };
            if NaiveDate::parse_from_str(stem, "%Y-%m-%d").is_err() {
                continue;
            }
            let body = std::fs::read_to_string(e.path()).unwrap_or_default();
            for line in body.lines() {
                let Some(b) = bullet(line) else { continue };
                let (time, text) = match b.split_once(' ') {
                    Some((t, rest)) if t.len() == 5 && chrono::NaiveTime::parse_from_str(t, "%H:%M").is_ok() => (t, rest),
                    _ => ("", b),
                };
                self.store.index_note(stem, time, text)?;
                n += 1;
            }
        }
        Ok(n)
    }
}

#[cfg(test)]
mod tests;
