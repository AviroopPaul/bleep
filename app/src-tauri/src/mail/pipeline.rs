//! One mail tick: every 2 minutes (or at once after the Mac wakes) fetch new mail, sort it,
//! and ping for what matters. No model call unless rules cannot tell.
use super::triage::Triager;
use super::watch::MailWatcher;
use crate::proactive::notifier::Notifier;
use crate::store::Store;
use chrono::{DateTime, Duration, Utc};
use std::sync::Arc;

pub const EVERY_MINUTES: i64 = 2;

#[derive(Default)]
struct State {
    last_sync: Option<DateTime<Utc>>,
    last_tick: Option<DateTime<Utc>>,
}

pub struct MailPipeline {
    watcher: Arc<MailWatcher>,
    triager: Arc<Triager>,
    notifier: Arc<Notifier>,
    store: Arc<Store>,
    state: tokio::sync::Mutex<State>,
}

impl MailPipeline {
    pub fn new(watcher: Arc<MailWatcher>, triager: Arc<Triager>, notifier: Arc<Notifier>, store: Arc<Store>) -> Arc<MailPipeline> {
        Arc::new(MailPipeline { watcher, triager, notifier, store, state: Default::default() })
    }

    /// Returns the ping texts sent this tick.
    pub async fn tick(&self, now: DateTime<Utc>) -> Vec<String> {
        let due = {
            let mut st = self.state.lock().await;
            let woke = st.last_tick.is_some_and(|t| now - t > Duration::seconds(60));
            let stale = st.last_sync.is_none_or(|t| now - t >= Duration::minutes(EVERY_MINUTES));
            st.last_tick = Some(now);
            if woke || stale {
                st.last_sync = Some(now);
            }
            woke || stale
        };
        if !due {
            return vec![];
        }
        let fresh = match self.watcher.sync().await {
            Ok(r) => r.new,
            Err(e) => {
                eprintln!("bleep: mail sync: {e}");
                return vec![];
            }
        };
        self.triager.run(now).await;
        if fresh.is_empty() {
            return vec![];
        }
        let rows = self.store.mail_rows(&fresh).unwrap_or_default();
        self.notifier.after_triage(&rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{GoogleConfig, MailConfig};
    use crate::google::mock::{fixture, MockGoogle};
    use crate::google::secrets::{MemorySecrets, Secrets};
    use crate::google::{GoogleAuth, ACCOUNT_KEY, REFRESH_KEY};
    use crate::opener::RecordingOpener;
    use crate::proactive::scheduler::BarUi;
    use crate::runtime::fake::FakeRuntime;
    use crate::store::tests::utc;
    use serde_json::json;
    use std::sync::atomic::AtomicBool;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Ui(Mutex<Vec<String>>);
    impl BarUi for Ui {
        fn show(&self, _c: &crate::proactive::cues::BarCue) {}
        fn hide(&self) {}
        fn bubble(&self, t: &str) {
            self.0.lock().unwrap().push(t.into());
        }
    }

    /// The bill fixture is due 14 Nov; "now" is 13 Nov, so it is due tomorrow.
    fn now() -> DateTime<Utc> {
        utc("2026-11-13T10:00:00Z")
    }

    #[tokio::test]
    async fn new_bill_pings_once_and_seeded_mail_never() {
        let d = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::open(&d.path().join("b.db")).unwrap());
        let mock = MockGoogle::start().await;
        mock.on("POST /token", vec![(200, fixture("token_refresh.json"))]);
        mock.on("GET /gmail/v1/users/me/profile", vec![(200, fixture("gmail_profile.json"))]);
        mock.on_query("GET /gmail/v1/users/me/messages", "in%3Ainbox", (200, fixture("gmail_seed_list.json")));
        for (id, f) in [("m1", "gmail_msg_bill.json"), ("m2", "gmail_msg_promo.json"), ("m3", "gmail_msg_personal.json"), ("m4", "gmail_msg_sent.json")] {
            mock.on(&format!("GET /gmail/v1/users/me/messages/{id}"), vec![(200, fixture(f))]);
        }
        let secrets = Arc::new(MemorySecrets::default());
        secrets.set(REFRESH_KEY, "rt").unwrap();
        secrets.set(ACCOUNT_KEY, "me@example.com").unwrap();
        let auth = GoogleAuth::new(GoogleConfig { client_id: "c".into(), client_secret: "s".into() }, mock.endpoints(), secrets, Arc::new(RecordingOpener::default()));
        let watcher = MailWatcher::new(auth, reqwest::Client::new(), format!("{}/gmail/v1", mock.base), store.clone());
        let rt = Arc::new(FakeRuntime::new(vec![]));
        let triager = Triager::new(rt.clone(), store.clone(), Arc::new(MailConfig::default));
        let ui = Arc::new(Ui::default());
        let notifier = Notifier::new(store.clone(), ui.clone(), Arc::new(AtomicBool::new(false)), Arc::new(MailConfig::default), Arc::new(now));
        let p = MailPipeline::new(watcher, triager, notifier, store.clone());

        // First tick seeds: the bill (m1) is sorted but never pings.
        assert!(p.tick(now()).await.is_empty());
        assert_eq!(store.thread_rows("t1").unwrap()[0].triage, Some(crate::store::Label::Bill));
        // One minute later: not due yet, nothing fetched.
        let hits = mock.hits.lock().unwrap().len();
        assert!(p.tick(now() + Duration::minutes(1)).await.is_empty());
        assert_eq!(mock.hits.lock().unwrap().len(), hits);

        // New mail arrives: the same bill as a new message in a new thread.
        let mut bill: serde_json::Value = serde_json::from_str(&fixture("gmail_msg_bill.json")).unwrap();
        bill["id"] = json!("m9");
        bill["threadId"] = json!("t9");
        mock.on("GET /gmail/v1/users/me/messages/m9", vec![(200, bill.to_string())]);
        mock.on("GET /gmail/v1/users/me/history", vec![(200, json!({"history": [{"messagesAdded": [{"message": {"id": "m9"}}]}], "historyId": "110"}).to_string())]);
        rt.one_shot_replies.lock().unwrap().push_back(Ok(json!({"items": []})));
        let sent = p.tick(now() + Duration::minutes(3)).await;
        assert_eq!(sent, vec!["Bill due tomorrow: City Power ₹2,340".to_string()]);
        assert_eq!(ui.0.lock().unwrap().len(), 1);
    }
}
