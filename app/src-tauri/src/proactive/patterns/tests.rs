use super::*;
use crate::runtime::fake::FakeRuntime;
use chrono::{NaiveDateTime, TimeZone};
use serde_json::json;
use std::sync::Mutex;

fn local(s: &str) -> NaiveDateTime {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").unwrap()
}

#[test]
fn due_window_and_once_a_day() {
    let none = |_: &str| false;
    assert_eq!(due(local("2026-11-15 21:59"), "22:00", &none), None);
    assert_eq!(due(local("2026-11-15 22:00"), "22:00", &none).as_deref(), Some("2026-11-15"));
    assert_eq!(due(local("2026-11-15 23:59"), "22:00", &none).as_deref(), Some("2026-11-15"), "late after a wake");
    assert_eq!(due(local("2026-11-16 00:01"), "22:00", &none), None, "more than 2 hours late");
    assert_eq!(due(local("2026-11-15 22:30"), "22:00", &|k| k == "2026-11-15"), None);
    assert_eq!(due(local("2026-11-15 22:30"), "bad", &none), None);
}

struct Pings(Mutex<Vec<usize>>);
impl SuggestPing for Pings {
    fn suggestions(&self, n: usize) {
        self.0.lock().unwrap().push(n);
    }
}

struct Rig {
    _d: tempfile::TempDir,
    store: Arc<Store>,
    rt: Arc<FakeRuntime>,
    mem: Arc<Memory>,
    pings: Arc<Pings>,
    paused: Arc<AtomicBool>,
    job: Arc<PatternJob>,
}

fn rig(at: &str) -> Rig {
    let d = tempfile::tempdir().unwrap();
    let paths = crate::paths::BleepPaths::from_root(d.path().to_path_buf());
    paths.ensure().unwrap();
    let store = Arc::new(Store::open(&paths.db).unwrap());
    let mem = Arc::new(Memory::new(paths, store.clone()));
    mem.ensure().unwrap();
    let rt = Arc::new(FakeRuntime::new(vec![]));
    let pings = Arc::new(Pings(Mutex::new(vec![])));
    let paused = Arc::new(AtomicBool::new(false));
    let t = chrono::Local.from_local_datetime(&local(at)).unwrap();
    let job = PatternJob::new(PatternDeps {
        store: store.clone(),
        runtime: rt.clone(),
        memory: mem.clone(),
        nightly: Arc::new(|| "22:00".into()),
        paused: paused.clone(),
        ping: pings.clone(),
        now: Arc::new(move || t),
    });
    Rig { _d: d, store, rt, mem, pings, paused, job }
}

fn reply(r: &Rig, v: serde_json::Value) {
    r.rt.one_shot_replies.lock().unwrap().push_back(Ok(v));
}

#[tokio::test]
async fn proposes_new_patterns_and_pings_once() {
    let r = rig("2026-11-15 22:05");
    r.mem.append_note(chrono::Local.from_local_datetime(&local("2026-11-15 19:10")).unwrap(), "Paid the electricity bill").unwrap();
    r.mem.add_fact(crate::paths::MemFile::Memory, "Standup is at 10:00").unwrap();
    r.store.add_proposal("Dismissed before", "nightly").unwrap();
    reply(&r, json!({"patterns": ["You pay bills the evening they are due", "standup is at 10:00.", "Dismissed before", "two\nlines"]}));
    assert_eq!(r.job.tick().await, Some(1));
    let p = r.store.pending_proposals().unwrap();
    assert_eq!(p.iter().map(|p| p.fact.as_str()).collect::<Vec<_>>(), ["Dismissed before", "You pay bills the evening they are due"]);
    assert_eq!(r.pings.0.lock().unwrap().as_slice(), [1]);
    let prompt = r.rt.one_shots.lock().unwrap()[0].clone();
    assert!(prompt.starts_with(PATTERNS_PROMPT) && prompt.contains("19:10 Paid the electricity bill"), "{prompt}");
    assert_eq!(r.store.llm_calls_since(chrono::DateTime::UNIX_EPOCH).unwrap(), 1, "counts against the background cap");
    assert_eq!(r.job.tick().await, None, "once a day");
    assert_eq!(r.rt.one_shots.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn nothing_new_no_ping() {
    let r = rig("2026-11-15 22:05");
    r.mem.append_note(chrono::Local.from_local_datetime(&local("2026-11-15 19:10")).unwrap(), "x").unwrap();
    reply(&r, json!({"patterns": []}));
    assert_eq!(r.job.tick().await, Some(0));
    assert!(r.pings.0.lock().unwrap().is_empty());
}

#[tokio::test]
async fn quiet_day_makes_no_call() {
    let r = rig("2026-11-15 22:05");
    assert_eq!(r.job.tick().await, Some(0));
    assert!(r.rt.one_shots.lock().unwrap().is_empty());
    assert_eq!(r.job.tick().await, None, "the day counts as done");
}

#[tokio::test]
async fn paused_or_over_cap_waits() {
    let r = rig("2026-11-15 22:05");
    r.mem.append_note(chrono::Local.from_local_datetime(&local("2026-11-15 19:10")).unwrap(), "x").unwrap();
    r.paused.store(true, Ordering::SeqCst);
    assert_eq!(r.job.tick().await, None);
    r.paused.store(false, Ordering::SeqCst);
    let t = chrono::Local.from_local_datetime(&local("2026-11-15 21:50")).unwrap().with_timezone(&chrono::Utc);
    for _ in 0..crate::mail::triage::MAX_MODEL_CALLS_PER_HOUR {
        r.store.log_llm_at("triage", t).unwrap();
    }
    assert_eq!(r.job.tick().await, None);
    assert!(r.rt.one_shots.lock().unwrap().is_empty(), "no call over the cap");
}

#[tokio::test]
async fn failed_call_does_not_retry_all_night() {
    let r = rig("2026-11-15 22:05");
    r.mem.append_note(chrono::Local.from_local_datetime(&local("2026-11-15 19:10")).unwrap(), "x").unwrap();
    r.rt.one_shot_replies.lock().unwrap().push_back(Err("limit".into()));
    assert_eq!(r.job.tick().await, None);
    assert_eq!(r.job.tick().await, None);
    assert_eq!(r.rt.one_shots.lock().unwrap().len(), 1);
}

#[test]
fn prompt_has_no_em_dash() {
    assert!(!PATTERNS_PROMPT.contains('\u{2014}'));
}
