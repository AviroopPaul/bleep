use super::*;
use crate::paths::MemFile::{Memory as Mem, Rules, User};
use chrono::TimeZone;

fn mem() -> (tempfile::TempDir, Memory) {
    let d = tempfile::tempdir().unwrap();
    let paths = BleepPaths::from_root(d.path().to_path_buf());
    paths.ensure().unwrap();
    let store = Arc::new(Store::open(&paths.db).unwrap());
    let m = Memory::new(paths, store);
    m.ensure().unwrap();
    (d, m)
}

fn read(m: &Memory, f: MemFile) -> String {
    std::fs::read_to_string(m.paths.memory_file(f)).unwrap()
}

fn write(m: &Memory, f: MemFile, s: &str) {
    std::fs::write(m.paths.memory_file(f), s).unwrap();
}

const MINE: &str = "# What bleep has learned\n\n<!-- my own comment -->\n- Sync means my 1:1 with Priya\n\n## Work\n- I pay bills the evening they are due\n- Sync means my 1:1 with Priya\nfree text I wrote\n";

#[test]
fn ensure_creates_files_once() {
    let (_d, m) = mem();
    for f in MemFile::ALL {
        let s = read(&m, f);
        assert!(s.starts_with("# ") && s.contains("<!--"), "{s}");
    }
    write(&m, Mem, "mine\n");
    m.ensure().unwrap();
    assert_eq!(read(&m, Mem), "mine\n", "never overwrites my file");
}

#[test]
fn prompt_skips_empty_files_and_comments() {
    let (_d, m) = mem();
    assert_eq!(m.load_for_prompt(), "", "fresh files say nothing");
    write(&m, Mem, MINE);
    let p = m.load_for_prompt();
    assert!(p.starts_with("## MEMORY.md\n"), "{p}");
    assert!(p.contains("- I pay bills the evening they are due") && p.contains("free text I wrote"));
    assert!(!p.contains("my own comment") && !p.contains("USER.md"), "{p}");
}

#[test]
fn prompt_is_cut_at_budget_with_note() {
    let (_d, m) = mem();
    let long: String = (0..2000).map(|i| format!("- fact number {i}\n")).collect();
    write(&m, User, &long);
    let p = m.load_for_prompt();
    assert!(p.chars().count() <= PROMPT_BUDGET + CUT_NOTE.len() + 2, "{}", p.len());
    assert!(p.ends_with(CUT_NOTE));
}

#[test]
fn facts_lists_bullets_per_file() {
    let (_d, m) = mem();
    write(&m, Mem, MINE);
    write(&m, Rules, "# Rules\n* Never ping me before 9\n");
    let f = m.facts();
    let mem = f.iter().find(|x| x.file == Mem).unwrap();
    assert_eq!(mem.facts, ["Sync means my 1:1 with Priya", "I pay bills the evening they are due", "Sync means my 1:1 with Priya"]);
    assert_eq!(f.iter().find(|x| x.file == Rules).unwrap().facts, ["Never ping me before 9"]);
    assert!(f.iter().find(|x| x.file == User).unwrap().facts.is_empty());
}

#[test]
fn add_fact_appends_and_keeps_my_bytes() {
    let (_d, m) = mem();
    write(&m, Mem, "# Mine\nno newline at end");
    assert_eq!(m.add_fact(Mem, "  Standup is at 10:00  "), Ok(Added::Yes));
    assert_eq!(read(&m, Mem), "# Mine\nno newline at end\n- Standup is at 10:00\n");
    assert_eq!(m.add_fact(Mem, "standup is at 10:00."), Ok(Added::Duplicate));
    assert_eq!(m.add_fact(User, "Standup is at 10:00"), Ok(Added::Duplicate), "dedupe across files");
}

#[test]
fn add_fact_refuses_bad_input() {
    let (_d, m) = mem();
    assert!(m.add_fact(Mem, "").is_err());
    assert!(m.add_fact(Mem, "two\nlines").is_err());
    assert!(m.add_fact(Mem, &"x".repeat(MAX_FACT + 1)).is_err());
    assert_eq!(m.check_fact("- leading dash"), Ok("leading dash".into()));
}

#[test]
fn add_fact_refuses_when_full() {
    let (_d, m) = mem();
    let long: String = (0..1500).map(|i| format!("- fact number {i}\n")).collect();
    write(&m, User, &long);
    assert_eq!(m.add_fact(Mem, "one more"), Err(FULL.to_string()));
}

#[test]
fn forget_removes_one_line_by_text() {
    let (_d, m) = mem();
    write(&m, Mem, MINE);
    assert_eq!(m.forget_fact(Mem, "Sync means my 1:1 with Priya").unwrap(), Forget::Removed);
    assert_eq!(read(&m, Mem), MINE.replacen("- Sync means my 1:1 with Priya\n", "", 1));
    assert_eq!(m.forget_fact(Mem, "Sync means my 1:1 with Priya").unwrap(), Forget::Removed, "the duplicate goes next");
    assert_eq!(m.forget_fact(Mem, "Sync means my 1:1 with Priya").unwrap(), Forget::Gone);
    assert_eq!(m.forget_fact(Mem, "free text I wrote").unwrap(), Forget::Gone, "only bullet lines are facts");
}

#[test]
fn forget_on_missing_file_is_gone() {
    let (_d, m) = mem();
    std::fs::remove_file(m.paths.memory_file(Rules)).unwrap();
    assert_eq!(m.forget_fact(Rules, "x").unwrap(), Forget::Gone);
}

#[test]
fn writes_follow_a_symlink() {
    let (d, m) = mem();
    let real = d.path().join("dotfiles-MEMORY.md");
    std::fs::write(&real, "# Mine\n").unwrap();
    let link = m.paths.memory_file(Mem);
    std::fs::remove_file(&link).unwrap();
    std::os::unix::fs::symlink(&real, &link).unwrap();
    m.add_fact(Mem, "Linked fact").unwrap();
    assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
    assert_eq!(std::fs::read_to_string(&real).unwrap(), "# Mine\n- Linked fact\n");
}

#[test]
fn notes_append_index_and_reindex() {
    let (_d, m) = mem();
    let at = chrono::Local.with_ymd_and_hms(2026, 11, 15, 10, 2, 0).unwrap();
    m.append_note(at, "Sync means my 1:1 with Priya").unwrap();
    m.append_note(at, "  second\nline  ").unwrap();
    let f = std::fs::read_to_string(m.paths.daily.join("2026-11-15.md")).unwrap();
    assert_eq!(f, "# 2026-11-15\n\n- 10:02 Sync means my 1:1 with Priya\n- 10:02 second line\n");
    assert_eq!(m.store.search_notes("\"priya\"", 5).unwrap()[0].time, "10:02");
    assert!(m.append_note(at, &"x".repeat(MAX_FACT + 1)).is_err());
    assert!(m.append_note(at, "  ").is_err());
    std::fs::write(m.paths.daily.join("2026-11-14.md"), "# 2026-11-14\n- 09:00 mango day\n- no time here\n").unwrap();
    std::fs::write(m.paths.daily.join("notes.md"), "- 09:00 not a daily file\n").unwrap();
    assert_eq!(m.reindex_notes().unwrap(), 4);
    let h = m.store.search_notes("\"time\"", 5).unwrap();
    assert_eq!((h[0].date.as_str(), h[0].time.as_str(), h[0].text.as_str()), ("2026-11-14", "", "no time here"));
    assert!(m.store.search_notes("\"daily\"", 5).unwrap().is_empty());
    assert_eq!(m.daily_note(at.date_naive()), f);
}
