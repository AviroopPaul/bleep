use super::*;
use crate::config::MemoryConfig;
use crate::paths::{BleepPaths, MemFile};

fn rig() -> (tempfile::TempDir, Arc<Store>, Memory) {
    let d = tempfile::tempdir().unwrap();
    let paths = BleepPaths::from_root(d.path().to_path_buf());
    paths.ensure().unwrap();
    let store = Arc::new(Store::open(&paths.db).unwrap());
    let mem = Memory::new(paths, store.clone());
    mem.ensure().unwrap();
    (d, store, mem)
}

#[test]
fn view_names_files_and_budget() {
    let (_d, _s, mem) = rig();
    mem.add_fact(MemFile::Memory, "Sync means my 1:1 with Priya").unwrap();
    let v = view(&mem);
    assert_eq!(v.files.iter().map(|f| f.file).collect::<Vec<_>>(), ["USER.md", "RULES.md", "MEMORY.md"]);
    assert_eq!(v.files[2].facts, ["Sync means my 1:1 with Priya"]);
    assert_eq!(v.budget, crate::memory::PROMPT_BUDGET);
    assert!(v.size > 0);
}

#[test]
fn forget_by_file_name() {
    let (_d, _s, mem) = rig();
    mem.add_fact(MemFile::User, "My name is Aviroop").unwrap();
    let v = forget(&mem, "USER.md", "My name is Aviroop").unwrap();
    assert!(v.files[0].facts.is_empty());
    assert!(forget(&mem, "SECRETS.md", "x").is_err());
    assert!(forget(&mem, "USER.md", "not there").is_ok(), "already gone is fine");
}

#[test]
fn decide_add_writes_memory_and_dismiss_does_not() {
    let (_d, store, mem) = rig();
    let a = store.add_proposal("You pay bills the evening they are due", "nightly").unwrap().unwrap();
    let b = store.add_proposal("Priya leads the design team", "compaction").unwrap().unwrap();
    let left = decide(&store, &mem, a, true).unwrap();
    assert_eq!(left.len(), 1);
    assert!(mem.is_known("You pay bills the evening they are due"));
    decide(&store, &mem, b, false).unwrap();
    assert!(!mem.is_known("Priya leads the design team"));
    assert!(decide(&store, &mem, b, true).unwrap().is_empty(), "a decided one stays decided");
    assert!(!mem.is_known("Priya leads the design team"));
}

#[test]
fn decide_add_when_full_keeps_the_suggestion() {
    let (_d, store, mem) = rig();
    let long: String = (0..1500).map(|i| format!("- fact number {i}\n")).collect();
    std::fs::write(mem.paths.memory_file(MemFile::User), long).unwrap();
    let id = store.add_proposal("One more", "nightly").unwrap().unwrap();
    assert_eq!(decide(&store, &mem, id, true).unwrap_err(), crate::memory::FULL);
    assert_eq!(store.pending_proposals().unwrap().len(), 1);
}

#[test]
fn settings_view_and_clamp() {
    let c = MemoryConfig { compact_at: 300_000, nightly: "22:00".into(), context_window: Some(200_000) };
    let v = settings_view(&c);
    assert_eq!((v.compact_at, v.threshold, v.window, v.nightly.as_str()), (300_000, 160_000, Some(200_000), "22:00"));
    assert_eq!(clamp_compact_at(5), crate::config::MIN_COMPACT_AT);
    assert_eq!(clamp_compact_at(250_000), 250_000);
    assert_eq!(clamp_compact_at(5_000_000), 1_000_000);
}
