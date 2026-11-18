//! The calendar watcher and the meeting bar clock. Every 30 s: refresh the calendar cache
//! when it is 5 minutes old or the Mac just woke, then show or hide the bar. No model calls.
use super::cues::{bar_worthy, bubble_text, cue_key, due_cue, BarCue, CueKind};
use crate::connectors::calendar::CalendarConnector;
use crate::google::GoogleAuth;
use crate::store::Store;
use chrono::{DateTime, Duration, Utc};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub const TICK: std::time::Duration = std::time::Duration::from_secs(30);
pub const PING_KIND: &str = "meeting_bar";

/// The windows the bar and bubble live in. Tauri in the app, a recorder in tests.
pub trait BarUi: Send + Sync {
    fn show(&self, cue: &BarCue);
    fn hide(&self);
    fn bubble(&self, text: &str);
    /// A mail ping: a bubble that can be marked "Not useful".
    fn ping(&self, text: &str, _ping_id: i64) {
        self.bubble(text);
    }
}

pub type Clock = Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>;

pub struct SchedulerDeps {
    pub store: Arc<Store>,
    pub calendar: Arc<CalendarConnector>,
    pub auth: Arc<GoogleAuth>,
    pub paused: Arc<AtomicBool>,
    pub ui: Arc<dyn BarUi>,
    pub now: Clock,
}

#[derive(Default)]
struct State {
    current: Option<BarCue>,
    last_refresh: Option<DateTime<Utc>>,
    last_tick: Option<DateTime<Utc>>,
    refresh_requested: bool,
}

pub struct Scheduler {
    deps: SchedulerDeps,
    state: tokio::sync::Mutex<State>,
}

impl Scheduler {
    pub fn new(deps: SchedulerDeps) -> Arc<Scheduler> {
        Arc::new(Scheduler { deps, state: Default::default() })
    }

    /// The 30 s loop. Spawn it on the app's async runtime.
    pub async fn run(self: Arc<Self>) {
        let mut every = tokio::time::interval(TICK);
        every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            every.tick().await;
            self.tick().await;
        }
    }

    /// The calendar refresh runs without holding the state lock, so the bar's buttons
    /// never wait on Google.
    pub async fn tick(&self) {
        let now = (self.deps.now)();
        let refresh = {
            let mut st = self.state.lock().await;
            let woke = st.last_tick.is_some_and(|t| now - t > Duration::seconds(2 * TICK.as_secs() as i64));
            let stale = st.last_refresh.is_none_or(|t| now - t >= Duration::minutes(REFRESH_MINUTES));
            let due = st.refresh_requested || stale || woke;
            if due {
                st.refresh_requested = false;
                st.last_refresh = Some(now);
            }
            st.last_tick = Some(now);
            due
        };
        if refresh {
            self.refresh(now).await;
        }
        let mut st = self.state.lock().await;
        self.show_or_hide(&mut st, now);
    }

    async fn refresh(&self, now: DateTime<Utc>) {
        let d = &self.deps;
        // Calendar off in Settings > Connectors, or not signed in: no polling.
        if !d.auth.ready("calendar") {
            return;
        }
        let (from, to) = (now - Duration::hours(1), now + Duration::hours(36));
        match d.calendar.list_window(from, to).await {
            Ok(evs) => {
                if let Err(e) = d.store.replace_cal_window(from, to, &evs) {
                    eprintln!("bleep: calendar cache: {e}");
                }
            }
            Err(e) => eprintln!("bleep: calendar refresh: {e}"),
        }
    }

    fn show_or_hide(&self, st: &mut State, now: DateTime<Utc>) {
        let d = &self.deps;
        let paused = d.paused.load(Ordering::SeqCst);
        let events = d.store.cal_upcoming(now, now + Duration::minutes(61)).unwrap_or_default();
        if let Some(c) = &st.current {
            // Started, paused, or no longer on the calendar as shown (moved, cancelled, declined).
            let still = events.iter().any(|e| e.event_id == c.event_id && e.start == c.start && bar_worthy(e));
            if c.start <= now || paused || !still {
                st.current = None;
                d.ui.hide();
            }
        }
        if paused {
            return;
        }
        let seen = |k: &str| d.store.has_ping(PING_KIND, k).unwrap_or(false);
        let Some(cue) = due_cue(&events, now, &seen) else { return };
        if st.current.as_ref().is_some_and(|c| c.key == cue.key) {
            return;
        }
        let _ = d.store.record_ping(PING_KIND, &cue.key);
        d.ui.show(&cue);
        d.ui.bubble(&bubble_text(&cue, now));
        st.current = Some(cue);
    }

    pub async fn current(&self) -> Option<BarCue> {
        self.state.lock().await.current.clone()
    }

    /// Hides the bar until the 10 minute cue.
    pub async fn snooze(&self) {
        let mut st = self.state.lock().await;
        if st.current.take().is_some() {
            self.deps.ui.hide();
        }
    }

    /// Hides the bar and skips the rest of this meeting's cues.
    pub async fn dismiss(&self) {
        let mut st = self.state.lock().await;
        if let Some(c) = st.current.take() {
            let _ = self.deps.store.record_ping(PING_KIND, &cue_key(&c.event_id, c.start, CueKind::TenMin));
            self.deps.ui.hide();
        }
    }

    /// Refresh the cache now (after sign-in), then tick.
    pub async fn refresh_now(&self) {
        self.state.lock().await.refresh_requested = true;
        self.tick().await;
    }
}

pub const REFRESH_MINUTES: i64 = 5;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::GoogleConfig;
    use crate::google::mock::{fixture, MockGoogle};
    use crate::google::secrets::{MemorySecrets, Secrets};
    use crate::google::REFRESH_KEY;
    use crate::opener::RecordingOpener;
    use serde_json::json;
    use std::sync::Mutex;

    const EVENTS: &str = "GET /calendar/v3/calendars/primary/events";

    #[derive(Default)]
    struct FakeUi(Mutex<Vec<String>>);
    impl BarUi for FakeUi {
        fn show(&self, cue: &BarCue) {
            self.0.lock().unwrap().push(format!("show:{}", cue.key));
        }
        fn hide(&self) {
            self.0.lock().unwrap().push("hide".into());
        }
        fn bubble(&self, text: &str) {
            self.0.lock().unwrap().push(format!("bubble:{text}"));
        }
    }

    struct Rig {
        _d: tempfile::TempDir,
        mock: MockGoogle,
        ui: Arc<FakeUi>,
        clock: Arc<Mutex<DateTime<Utc>>>,
        paused: Arc<AtomicBool>,
        s: Arc<Scheduler>,
    }

    fn t0() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-11-12T09:00:00Z").unwrap().with_timezone(&Utc)
    }

    fn at(mins: i64) -> String {
        (t0() + Duration::minutes(mins)).to_rfc3339()
    }

    /// One meeting starting `start` minutes after t0, 30 minutes long.
    fn events(start: i64) -> String {
        json!({"items": [{"id": "m1", "summary": "Design review", "start": {"dateTime": at(start)}, "end": {"dateTime": at(start + 30)}, "hangoutLink": "https://meet.google.com/x"}]}).to_string()
    }

    async fn rig(signed_in: bool, meeting_at: i64) -> Rig {
        let d = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::open(&d.path().join("b.db")).unwrap());
        let mock = MockGoogle::start().await;
        mock.on("POST /token", vec![(200, fixture("token_refresh.json"))]);
        mock.on(EVENTS, vec![(200, events(meeting_at))]);
        let secrets = Arc::new(MemorySecrets::default());
        if signed_in {
            secrets.set(REFRESH_KEY, "rt").unwrap();
        }
        let auth = GoogleAuth::new(GoogleConfig { client_id: "c".into(), client_secret: "s".into() }, mock.endpoints(), secrets, Arc::new(RecordingOpener::default()));
        auth.set_on(["calendar".to_string()]);
        let calendar = CalendarConnector::new(auth.clone(), reqwest::Client::new(), format!("{}/calendar/v3", mock.base));
        let ui = Arc::new(FakeUi::default());
        let clock = Arc::new(Mutex::new(t0()));
        let c = clock.clone();
        let paused = Arc::new(AtomicBool::new(false));
        let s = Scheduler::new(SchedulerDeps { store, calendar, auth, paused: paused.clone(), ui: ui.clone(), now: Arc::new(move || *c.lock().unwrap()) });
        Rig { _d: d, mock, ui, clock, paused, s }
    }

    impl Rig {
        fn set(&self, mins: i64) {
            *self.clock.lock().unwrap() = t0() + Duration::minutes(mins);
        }
        fn log(&self) -> Vec<String> {
            self.ui.0.lock().unwrap().clone()
        }
    }

    #[tokio::test]
    async fn tick_shows_bar_once_and_records_ping() {
        let r = rig(true, 50).await;
        r.s.tick().await;
        let log = r.log();
        assert_eq!(log.len(), 2, "{log:?}");
        assert!(log[0].starts_with("show:m1@") && log[0].ends_with(":hour"), "{log:?}");
        assert_eq!(log[1], "bubble:Design review in 50 min");
        assert_eq!(r.s.current().await.unwrap().kind, CueKind::Hour);
        r.set(1);
        r.s.tick().await;
        assert_eq!(r.log().len(), 2, "the same cue showed twice: {:?}", r.log());
    }

    #[tokio::test]
    async fn paused_shows_nothing() {
        let r = rig(true, 50).await;
        r.paused.store(true, Ordering::SeqCst);
        r.s.tick().await;
        assert!(r.log().is_empty(), "{:?}", r.log());
        r.paused.store(false, Ordering::SeqCst);
        r.set(1);
        r.s.tick().await;
        assert!(r.log()[0].starts_with("show:"), "{:?}", r.log());
    }

    #[tokio::test]
    async fn bar_hides_when_the_meeting_starts() {
        let r = rig(true, 8).await;
        r.s.tick().await;
        assert!(r.log()[0].ends_with(":ten"), "{:?}", r.log());
        r.set(8);
        r.s.tick().await;
        assert_eq!(r.log().last().unwrap(), "hide");
        assert!(r.s.current().await.is_none());
    }

    #[tokio::test]
    async fn snooze_hides_until_ten_min_cue() {
        let r = rig(true, 50).await;
        r.s.tick().await;
        r.s.snooze().await;
        assert_eq!(r.log().last().unwrap(), "hide");
        r.set(20);
        r.s.tick().await;
        assert_eq!(r.log().len(), 3, "{:?}", r.log());
        r.set(41);
        r.s.tick().await;
        let log = r.log();
        assert!(log[3].ends_with(":ten"), "{log:?}");
        assert_eq!(log[4], "bubble:Design review in 9 min");
    }

    #[tokio::test]
    async fn dismiss_suppresses_ten_min_cue() {
        let r = rig(true, 50).await;
        r.s.tick().await;
        r.s.dismiss().await;
        r.set(42);
        r.s.tick().await;
        assert_eq!(r.log().len(), 3, "{:?}", r.log());
    }

    #[tokio::test]
    async fn snooze_does_not_wait_for_a_slow_refresh() {
        let r = rig(true, 50).await;
        r.s.tick().await;
        r.mock.slow(EVENTS, std::time::Duration::from_millis(1500));
        r.set(6);
        let s = r.s.clone();
        let refresh = tokio::spawn(async move { s.tick().await });
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        let quick = tokio::time::timeout(std::time::Duration::from_millis(400), r.s.snooze()).await;
        assert!(quick.is_ok(), "Snooze waited for the calendar refresh");
        refresh.await.unwrap();
    }

    #[tokio::test]
    async fn bar_hides_when_the_meeting_moves_away_or_is_declined() {
        let r = rig(true, 50).await;
        r.s.tick().await;
        assert!(r.log()[0].starts_with("show:"));
        // Moved to 2 hours out: the next refresh must take the bar down.
        r.mock.on(EVENTS, vec![(200, events(120))]);
        r.set(6);
        r.s.tick().await;
        assert_eq!(r.log().last().unwrap(), "hide", "{:?}", r.log());
        assert!(r.s.current().await.is_none());

        let r = rig(true, 50).await;
        r.s.tick().await;
        let declined = json!({"items": [{"id": "m1", "summary": "Design review", "start": {"dateTime": at(50)}, "end": {"dateTime": at(80)}, "hangoutLink": "https://meet.google.com/x", "attendees": [{"email": "me@example.com", "self": true, "responseStatus": "declined"}]}]}).to_string();
        r.mock.on(EVENTS, vec![(200, declined)]);
        r.set(6);
        r.s.tick().await;
        assert_eq!(r.log().last().unwrap(), "hide", "{:?}", r.log());
    }

    #[tokio::test]
    async fn wake_gap_triggers_refresh() {
        let r = rig(true, 500).await;
        r.s.tick().await;
        assert_eq!(r.mock.hits_for(EVENTS).len(), 1);
        r.set(1);
        r.s.tick().await;
        assert_eq!(r.mock.hits_for(EVENTS).len(), 1, "refreshed too soon");
        r.set(3 * 60);
        r.s.tick().await;
        assert_eq!(r.mock.hits_for(EVENTS).len(), 2, "no refresh after the Mac woke");
        let h = &r.mock.hits_for(EVENTS)[1];
        assert!(h.query.contains("timeMin=2026-11-12T11%3A00%3A00Z") && h.query.contains("timeMax=2026-11-14T00%3A00%3A00Z"), "{}", h.query);
        r.set(3 * 60 + 5);
        r.s.tick().await;
        assert_eq!(r.mock.hits_for(EVENTS).len(), 3, "no refresh after 5 minutes");
    }

    #[tokio::test]
    async fn not_connected_skips_quietly() {
        let r = rig(false, 50).await;
        r.s.tick().await;
        assert!(r.mock.hits.lock().unwrap().is_empty());
        assert!(r.log().is_empty());
    }

    #[tokio::test]
    async fn refresh_now_fetches_at_once() {
        let r = rig(true, 500).await;
        r.s.tick().await;
        r.set(1);
        r.s.refresh_now().await;
        assert_eq!(r.mock.hits_for(EVENTS).len(), 2);
    }
}
