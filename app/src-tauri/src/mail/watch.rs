//! Polls Gmail `history.list` from a stored historyId and fills `mail_cache`.
//! First run and an expired historyId seed from the last 2 days; seeded mail never pings.
use crate::connectors::gmail::mime;
use crate::google::GoogleAuth;
use crate::store::{MailRow, Store};
use serde_json::Value;
use std::sync::Arc;

pub const CURSOR: &str = "gmail";

#[derive(Debug, Default, PartialEq)]
pub struct SyncResult {
    /// New message ids since the last sync (empty after a seed).
    pub new: Vec<String>,
    pub seeded: bool,
}

pub struct MailWatcher {
    auth: Arc<GoogleAuth>,
    http: reqwest::Client,
    base: String,
    store: Arc<Store>,
}

/// A cached row from a Gmail message in `metadata` format. `me` is the signed-in address.
pub fn to_row(msg: &Value, me: &str, seeded: bool) -> Option<MailRow> {
    let p = &msg["payload"];
    let (from_name, from_addr) = parse_addr(mime::header(p, "From").unwrap_or(""));
    let me = me.to_lowercase();
    let mut labels: Vec<String> = msg["labelIds"].as_array().into_iter().flatten().filter_map(|v| v.as_str().map(String::from)).collect();
    if mime::header(p, "List-Unsubscribe").is_some() {
        labels.push("X-LIST".into());
    }
    let to_me = !me.is_empty() && mime::header(p, "To").unwrap_or("").split(',').any(|a| parse_addr(a).1 == me);
    let from_me = labels.iter().any(|l| l == "SENT") || (!me.is_empty() && from_addr == me);
    let received = msg["internalDate"].as_str().and_then(|s| s.parse::<i64>().ok()).and_then(chrono::DateTime::from_timestamp_millis).unwrap_or_else(chrono::Utc::now);
    Some(MailRow {
        message_id: msg["id"].as_str()?.to_string(),
        thread_id: msg["threadId"].as_str()?.to_string(),
        from_addr,
        from_name,
        to_me,
        from_me,
        subject: mime::header(p, "Subject").unwrap_or("(no subject)").to_string(),
        snippet: msg["snippet"].as_str().unwrap_or("").to_string(),
        labels,
        received,
        triage: None,
        amount: None,
        due: None,
        reason: None,
        seeded,
        paid: false,
    })
}

/// `"Name" <addr>` or `addr` into (name, lowercased addr).
fn parse_addr(s: &str) -> (Option<String>, String) {
    let s = s.trim();
    match (s.rfind('<'), s.rfind('>')) {
        (Some(a), Some(b)) if a < b => {
            let name = s[..a].trim().trim_matches('"').trim();
            ((!name.is_empty()).then(|| name.to_string()), s[a + 1..b].trim().to_lowercase())
        }
        _ => (None, s.to_lowercase()),
    }
}

const SEED_QUERY: &str = "in:inbox newer_than:2d";
const MAX_NEW: usize = 100;

impl MailWatcher {
    pub fn new(auth: Arc<GoogleAuth>, http: reqwest::Client, base: String, store: Arc<Store>) -> Arc<MailWatcher> {
        Arc::new(MailWatcher { auth, http, base, store })
    }

    fn url(&self, path: &str) -> String {
        format!("{}/users/me/{path}", self.base)
    }

    /// Fetches new mail into the cache. Skips quietly when Google is not connected.
    pub async fn sync(&self) -> Result<SyncResult, String> {
        let Some(me) = self.auth.account() else { return Ok(SyncResult::default()) };
        match self.store.get_cursor(CURSOR).map_err(|e| e.to_string())? {
            None => self.seed(&me).await,
            Some(c) => match self.history(&c, &me).await {
                Err(e) if e.status == Some(404) => {
                    self.store.set_cursor(CURSOR, None).map_err(|e| e.to_string())?;
                    self.seed(&me).await
                }
                Err(e) => Err(format!("Gmail: {}", e.message)),
                Ok(r) => Ok(r),
            },
        }
    }

    async fn seed(&self, me: &str) -> Result<SyncResult, String> {
        let url = self.url("profile");
        let profile = self.auth.send_json(|| self.http.get(&url)).await.map_err(|e| e.message)?;
        let start = profile["historyId"].as_str().map(String::from).ok_or("Gmail sent no historyId.")?;
        let url = self.url("messages");
        let list = self.auth.send_json(|| self.http.get(&url).query(&[("q", SEED_QUERY), ("maxResults", "50")])).await.map_err(|e| e.message)?;
        for m in list["messages"].as_array().into_iter().flatten() {
            if let Some(id) = m["id"].as_str() {
                self.fetch(id, me, true).await?;
            }
        }
        self.store.set_cursor(CURSOR, Some(&start)).map_err(|e| e.to_string())?;
        Ok(SyncResult { new: vec![], seeded: true })
    }

    async fn history(&self, start: &str, me: &str) -> Result<SyncResult, crate::google::ApiError> {
        let url = self.url("history");
        let mut ids: Vec<String> = vec![];
        let mut page: Option<String> = None;
        let mut latest = start.to_string();
        for _ in 0..10 {
            let mut q = vec![("startHistoryId", start.to_string()), ("historyTypes", "messageAdded".into())];
            if let Some(p) = &page {
                q.push(("pageToken", p.clone()));
            }
            let v = self.auth.send_json(|| self.http.get(&url).query(&q)).await?;
            for h in v["history"].as_array().into_iter().flatten() {
                for added in h["messagesAdded"].as_array().into_iter().flatten() {
                    if let Some(id) = added["message"]["id"].as_str() {
                        if !ids.iter().any(|x| x == id) && ids.len() < MAX_NEW {
                            ids.push(id.to_string());
                        }
                    }
                }
            }
            if let Some(h) = v["historyId"].as_str() {
                latest = h.to_string();
            }
            page = v["nextPageToken"].as_str().map(String::from);
            if page.is_none() {
                break;
            }
        }
        let mut new = vec![];
        for id in &ids {
            if self.fetch(id, me, false).await.map_err(|m| crate::google::ApiError { status: None, message: m })? {
                new.push(id.clone());
            }
        }
        let _ = self.store.set_cursor(CURSOR, Some(&latest));
        Ok(SyncResult { new, seeded: false })
    }

    /// Stores one message. False when Gmail no longer has it (deleted since).
    async fn fetch(&self, id: &str, me: &str, seeded: bool) -> Result<bool, String> {
        let url = self.url(&format!("messages/{id}"));
        let q = [("format", "metadata"), ("metadataHeaders", "From"), ("metadataHeaders", "To"), ("metadataHeaders", "Cc"), ("metadataHeaders", "Subject"), ("metadataHeaders", "List-Unsubscribe")];
        let msg = match self.auth.send_json(|| self.http.get(&url).query(&q)).await {
            Ok(m) => m,
            Err(e) if e.status == Some(404) => return Ok(false),
            Err(e) => return Err(format!("Gmail: {}", e.message)),
        };
        let Some(row) = to_row(&msg, me, seeded) else { return Ok(false) };
        self.store.upsert_mail(&row).map_err(|e| e.to_string())?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::GoogleConfig;
    use crate::google::mock::{fixture, MockGoogle};
    use crate::google::secrets::{MemorySecrets, Secrets};
    use crate::google::{ACCOUNT_KEY, REFRESH_KEY};
    use crate::opener::RecordingOpener;

    const HISTORY: &str = "GET /gmail/v1/users/me/history";

    struct Rig {
        _d: tempfile::TempDir,
        mock: MockGoogle,
        w: Arc<MailWatcher>,
        store: Arc<Store>,
    }

    async fn rig(signed_in: bool) -> Rig {
        let d = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::open(&d.path().join("b.db")).unwrap());
        let mock = MockGoogle::start().await;
        mock.on("POST /token", vec![(200, fixture("token_refresh.json"))]);
        mock.on("GET /gmail/v1/users/me/profile", vec![(200, fixture("gmail_profile.json"))]);
        mock.on_query("GET /gmail/v1/users/me/messages", "in%3Ainbox", (200, fixture("gmail_seed_list.json")));
        for (id, f) in [("m1", "gmail_msg_bill.json"), ("m2", "gmail_msg_promo.json"), ("m3", "gmail_msg_personal.json"), ("m4", "gmail_msg_sent.json")] {
            mock.on(&format!("GET /gmail/v1/users/me/messages/{id}"), vec![(200, fixture(f))]);
        }
        mock.on(HISTORY, vec![(200, fixture("gmail_history.json"))]);
        let secrets = Arc::new(MemorySecrets::default());
        if signed_in {
            secrets.set(REFRESH_KEY, "rt").unwrap();
            secrets.set(ACCOUNT_KEY, "me@example.com").unwrap();
        }
        let auth = GoogleAuth::new(GoogleConfig { client_id: "c".into(), client_secret: "s".into() }, mock.endpoints(), secrets, Arc::new(RecordingOpener::default()));
        let w = MailWatcher::new(auth, reqwest::Client::new(), format!("{}/gmail/v1", mock.base), store.clone());
        Rig { _d: d, mock, w, store }
    }

    fn all(store: &Store) -> Vec<MailRow> {
        store.mail_since(chrono::DateTime::UNIX_EPOCH).unwrap()
    }

    #[test]
    fn to_row_reads_from_to_me_and_labels() {
        let bill: Value = serde_json::from_str(&fixture("gmail_msg_bill.json")).unwrap();
        let r = to_row(&bill, "me@example.com", false).unwrap();
        assert_eq!((r.message_id.as_str(), r.thread_id.as_str()), ("m1", "t1"));
        assert_eq!((r.from_addr.as_str(), r.from_name.as_deref()), ("bills@citypower.example", Some("City Power")));
        assert!(r.to_me && !r.from_me && !r.seeded);
        assert_eq!(r.subject, "Your bill of ₹2,340 is due on 14 Nov");
        assert!(r.snippet.starts_with("Amount due"));
        assert_eq!(r.received.timestamp_millis(), 1794470400000);
        let promo: Value = serde_json::from_str(&fixture("gmail_msg_promo.json")).unwrap();
        assert!(to_row(&promo, "me@example.com", true).unwrap().labels.contains(&"X-LIST".to_string()));
        let personal: Value = serde_json::from_str(&fixture("gmail_msg_personal.json")).unwrap();
        let p = to_row(&personal, "me@example.com", false).unwrap();
        assert_eq!((p.from_addr.as_str(), p.from_name.as_deref(), p.to_me), ("priya@example.com", Some("Priya S"), true));
        let sent: Value = serde_json::from_str(&fixture("gmail_msg_sent.json")).unwrap();
        assert!(to_row(&sent, "me@example.com", false).unwrap().from_me);
    }

    #[tokio::test]
    async fn first_sync_seeds_without_new() {
        let r = rig(true).await;
        let res = r.w.sync().await.unwrap();
        assert_eq!(res, SyncResult { new: vec![], seeded: true });
        let rows = all(&r.store);
        assert_eq!(rows.iter().map(|m| m.message_id.as_str()).collect::<Vec<_>>(), vec!["m1", "m2"]);
        assert!(rows.iter().all(|m| m.seeded));
        assert_eq!(r.store.get_cursor(CURSOR).unwrap().as_deref(), Some("100"));
        let q = &r.mock.hits_for("GET /gmail/v1/users/me/messages")[0].query;
        assert!(q.contains("newer_than%3A2d") && q.contains("maxResults=50"), "{q}");
        assert!(r.mock.hits_for(HISTORY).is_empty());
    }

    #[tokio::test]
    async fn history_adds_new_inbox_and_sent() {
        let r = rig(true).await;
        r.store.set_cursor(CURSOR, Some("100")).unwrap();
        let res = r.w.sync().await.unwrap();
        assert_eq!(res, SyncResult { new: vec!["m3".into(), "m4".into()], seeded: false });
        let rows = all(&r.store);
        let m3 = rows.iter().find(|m| m.message_id == "m3").unwrap();
        assert!(!m3.seeded && m3.to_me && !m3.from_me);
        assert!(rows.iter().find(|m| m.message_id == "m4").unwrap().from_me);
        assert_eq!(r.store.get_cursor(CURSOR).unwrap().as_deref(), Some("105"));
        let q = &r.mock.hits_for(HISTORY)[0].query;
        assert!(q.contains("startHistoryId=100") && q.contains("historyTypes=messageAdded"), "{q}");
        assert_eq!(r.mock.hits_for("GET /gmail/v1/users/me/messages/m3").len(), 1, "duplicate ids are fetched once");
    }

    #[tokio::test]
    async fn expired_history_reseeds() {
        let r = rig(true).await;
        r.store.set_cursor(CURSOR, Some("1")).unwrap();
        r.mock.on(HISTORY, vec![(404, r#"{"error":{"code":404,"message":"Requested entity was not found."}}"#.into())]);
        let res = r.w.sync().await.unwrap();
        assert_eq!(res, SyncResult { new: vec![], seeded: true });
        assert_eq!(r.store.get_cursor(CURSOR).unwrap().as_deref(), Some("100"));
        assert!(all(&r.store).iter().all(|m| m.seeded));
    }

    #[tokio::test]
    async fn not_connected_skips() {
        let r = rig(false).await;
        assert_eq!(r.w.sync().await.unwrap(), SyncResult::default());
        assert!(r.mock.hits.lock().unwrap().is_empty());
    }
}
