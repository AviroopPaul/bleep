//! Gmail connector. Read tools run at once, a draft is safe, a send waits for a card.
//! Text other people wrote comes back with an `_untrusted` key.
pub mod mime;

use super::{Connector, ConnectorStatus, Risk, ToolSpec};
use crate::google::GoogleAuth;
use crate::store::Store;
use async_trait::async_trait;
use chrono::{Local, TimeZone, Utc};
use serde_json::{json, Value};
use std::sync::Arc;

pub const MAX_SENDS_PER_DAY: usize = 10;

pub struct GmailConnector {
    auth: Arc<GoogleAuth>,
    http: reqwest::Client,
    /// `https://gmail.googleapis.com/gmail/v1`
    base: String,
    store: Arc<Store>,
}

impl GmailConnector {
    pub fn new(auth: Arc<GoogleAuth>, http: reqwest::Client, base: String, store: Arc<Store>) -> Arc<GmailConnector> {
        Arc::new(GmailConnector { auth, http, base, store })
    }

    /// Sends with a fresh access token; on 401 drops the token and tries once more.
    async fn send(&self, build: impl Fn() -> reqwest::RequestBuilder) -> Result<Value, String> {
        for attempt in 0..2 {
            let token = self.auth.access_token().await.map_err(|e| e.to_string())?;
            let resp = build().bearer_auth(&token).send().await.map_err(|e| format!("Could not reach Gmail: {e}"))?;
            let status = resp.status();
            if status == reqwest::StatusCode::UNAUTHORIZED && attempt == 0 {
                self.auth.invalidate().await;
                continue;
            }
            let text = resp.text().await.unwrap_or_default();
            if status.is_success() {
                return if text.trim().is_empty() { Ok(Value::Null) } else { serde_json::from_str(&text).map_err(|e| format!("Gmail sent a reply bleep could not read: {e}")) };
            }
            let msg = serde_json::from_str::<Value>(&text).ok().and_then(|v| v["error"]["message"].as_str().map(String::from)).unwrap_or(text);
            return Err(format!("Gmail error {}: {msg}", status.as_u16()));
        }
        Err("Gmail refused the sign-in twice.".into())
    }

    fn url(&self, path: &str) -> String {
        format!("{}/users/me/{path}", self.base)
    }

    async fn thread(&self, id: &str, query: &[(&str, &str)]) -> Result<Value, String> {
        let url = self.url(&format!("threads/{id}"));
        self.send(|| self.http.get(&url).query(query)).await
    }

    async fn search(&self, args: &Value) -> Result<Value, String> {
        let q = req_str(args, "query")?;
        let max = args["max"].as_u64().unwrap_or(10).clamp(1, 20).to_string();
        let url = self.url("threads");
        let list = self.send(|| self.http.get(&url).query(&[("q", q.as_str()), ("maxResults", max.as_str())])).await?;
        let mut out = vec![];
        for t in list["threads"].as_array().into_iter().flatten() {
            let Some(id) = t["id"].as_str() else { continue };
            let meta = self.thread(id, &[("format", "metadata"), ("metadataHeaders", "From"), ("metadataHeaders", "Subject"), ("metadataHeaders", "Date")]).await?;
            let msgs = meta["messages"].as_array().cloned().unwrap_or_default();
            let last = msgs.last().cloned().unwrap_or(Value::Null);
            let p = &last["payload"];
            out.push(json!({
                "thread_id": id,
                "from_untrusted": mime::header(p, "From"),
                "subject_untrusted": mime::header(p, "Subject"),
                "date": mime::header(p, "Date"),
                "snippet_untrusted": t["snippet"],
                "unread": msgs.iter().any(|m| has_label(m, "UNREAD")),
                "message_count": msgs.len(),
            }));
        }
        Ok(json!({"threads": out}))
    }

    async fn read_thread(&self, args: &Value) -> Result<Value, String> {
        let id = mail_id(args, "thread_id")?;
        let t = self.thread(&id, &[("format", "full")]).await?;
        let msgs = t["messages"].as_array().cloned().unwrap_or_default();
        let skip = msgs.len().saturating_sub(10);
        let out: Vec<Value> = msgs
            .iter()
            .skip(skip)
            .map(|m| {
                let p = &m["payload"];
                json!({
                    "message_id": m["id"],
                    "from_untrusted": mime::header(p, "From"),
                    "to_untrusted": mime::header(p, "To"),
                    "date": mime::header(p, "Date"),
                    "subject_untrusted": mime::header(p, "Subject"),
                    "body_untrusted": mime::body_text(p).chars().take(4000).collect::<String>(),
                    "sent_by_me": has_label(m, "SENT"),
                })
            })
            .collect();
        Ok(json!({"thread_id": id, "messages": out}))
    }

    /// Who, what and, for a reply, the headers that keep it in the thread.
    async fn compose(&self, args: &Value) -> Result<Value, String> {
        let to = addrs(args, "to")?;
        let cc = addrs(args, "cc")?;
        if to.is_empty() {
            return Err("to is required: at least one email address.".into());
        }
        if to.len() + cc.len() > 10 {
            return Err("At most 10 recipients.".into());
        }
        let body = req_str(args, "body")?;
        if body.chars().count() > 20_000 {
            return Err("The email is too long (20000 characters at most).".into());
        }
        let mut subject = opt_str(args, "subject");
        let (mut thread_id, mut in_reply_to, mut references) = (Value::Null, Value::Null, Value::Null);
        if args["reply_to_thread_id"].is_string() {
            let id = mail_id(args, "reply_to_thread_id")?;
            let meta = self.thread(&id, &[("format", "metadata"), ("metadataHeaders", "Subject"), ("metadataHeaders", "Message-ID"), ("metadataHeaders", "References")]).await?;
            let last = meta["messages"].as_array().and_then(|m| m.last()).cloned().unwrap_or(Value::Null);
            let p = &last["payload"];
            if let Some(mid) = mime::header(p, "Message-ID") {
                in_reply_to = json!(mid);
                let refs = format!("{} {mid}", mime::header(p, "References").unwrap_or("")).trim().to_string();
                references = json!(refs);
            }
            if subject.is_none() {
                let orig = mime::header(p, "Subject").unwrap_or("");
                subject = Some(if orig.to_ascii_lowercase().starts_with("re:") { orig.to_string() } else { format!("Re: {orig}") });
            }
            thread_id = json!(id);
        }
        let subject = subject.ok_or("subject is required for a new email.")?;
        if subject.chars().count() > 250 {
            return Err("The subject is too long (250 characters at most).".into());
        }
        Ok(json!({"to": to, "cc": cc, "subject": subject, "body": body, "thread_id": thread_id, "in_reply_to": in_reply_to, "references": references}))
    }

    async fn draft(&self, args: &Value) -> Result<Value, String> {
        let c = self.compose(args).await?;
        let mut message = json!({"raw": mime::build_raw(&outgoing(&c))?});
        if c["thread_id"].is_string() {
            message["threadId"] = c["thread_id"].clone();
        }
        let url = self.url("drafts");
        let body = json!({"message": message});
        let v = self.send(|| self.http.post(&url).json(&body)).await?;
        Ok(json!({"draft_id": v["id"], "saved": "Saved to Gmail drafts. Nothing was sent."}))
    }

    /// Addresses I have never sent mail to. The card warns about them.
    async fn new_recipients(&self, all: &[String]) -> Result<Vec<String>, String> {
        let url = self.url("messages");
        let mut out = vec![];
        for a in all {
            let q = format!("in:sent to:{a}");
            let v = self.send(|| self.http.get(&url).query(&[("q", q.as_str()), ("maxResults", "1")])).await?;
            if v["messages"].as_array().is_none_or(|m| m.is_empty()) {
                out.push(a.clone());
            }
        }
        Ok(out)
    }

    async fn prepare_send(&self, args: &Value) -> Result<Value, String> {
        let midnight = Local::now().date_naive().and_hms_opt(0, 0, 0).and_then(|n| Local.from_local_datetime(&n).earliest()).map(|d| d.with_timezone(&Utc)).unwrap_or_else(Utc::now);
        if self.store.count_done_since("gmail_send", midnight).map_err(|e| e.to_string())? >= MAX_SENDS_PER_DAY {
            return Err(format!("bleep has sent {MAX_SENDS_PER_DAY} emails today, the daily limit. Send this one from Gmail, or try tomorrow."));
        }
        let mut c = self.compose(args).await?;
        let everyone: Vec<String> = c["to"].as_array().into_iter().chain(c["cc"].as_array()).flatten().filter_map(|v| v.as_str().map(String::from)).collect();
        c["new_recipients"] = json!(self.new_recipients(&everyone).await?);
        Ok(c)
    }

    async fn execute_send(&self, p: &Value) -> Result<Value, String> {
        let mut body = json!({"raw": mime::build_raw(&outgoing(p))?});
        if p["thread_id"].is_string() {
            body["threadId"] = p["thread_id"].clone();
        }
        let url = self.url("messages/send");
        let v = self.send(|| self.http.post(&url).json(&body)).await?;
        Ok(json!({"sent": true, "id": v["id"], "thread_id": v["threadId"]}))
    }
}

fn outgoing(c: &Value) -> mime::Outgoing {
    let list = |k: &str| c[k].as_array().into_iter().flatten().filter_map(|v| v.as_str().map(String::from)).collect();
    mime::Outgoing {
        to: list("to"),
        cc: list("cc"),
        subject: c["subject"].as_str().unwrap_or("").into(),
        body: c["body"].as_str().unwrap_or("").into(),
        in_reply_to: c["in_reply_to"].as_str().map(String::from),
        references: c["references"].as_str().map(String::from),
    }
}

fn has_label(m: &Value, l: &str) -> bool {
    m["labelIds"].as_array().is_some_and(|ls| ls.iter().any(|x| x == l))
}

fn opt_str(args: &Value, k: &str) -> Option<String> {
    args[k].as_str().map(str::trim).filter(|s| !s.is_empty()).map(String::from)
}

fn req_str(args: &Value, k: &str) -> Result<String, String> {
    opt_str(args, k).ok_or_else(|| format!("{k} is required."))
}

/// Gmail ids are hex. Anything else never reaches a URL.
fn mail_id(args: &Value, k: &str) -> Result<String, String> {
    let id = req_str(args, k)?;
    if id.len() > 64 || !id.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err("That is not a Gmail thread id.".into());
    }
    Ok(id)
}

/// One address or a list, checked and lowercased.
fn addrs(args: &Value, k: &str) -> Result<Vec<String>, String> {
    let items: Vec<&Value> = match &args[k] {
        Value::Null => return Ok(vec![]),
        Value::String(_) => vec![&args[k]],
        Value::Array(a) => a.iter().collect(),
        _ => return Err(format!("{k} must be email addresses.")),
    };
    items
        .into_iter()
        .map(|v| {
            let e = v.as_str().map(str::trim).unwrap_or_default();
            let ok = e.len() <= 254
                && !e.contains(|c: char| c.is_whitespace() || c == ',' || c == '<' || c == '>')
                && matches!(e.split_once('@'), Some((l, d)) if !l.is_empty() && d.contains('.') && !d.starts_with('.') && !d.ends_with('.') && !d.contains('@'));
            if ok { Ok(e.to_lowercase()) } else { Err(format!("\"{e}\" is not an email address.")) }
        })
        .collect()
}

#[async_trait]
impl Connector for GmailConnector {
    fn id(&self) -> &'static str {
        "gmail"
    }

    fn tools(&self) -> Vec<ToolSpec> {
        let to = json!({"type": "array", "items": {"type": "string"}, "description": "Email addresses."});
        let compose = json!({"type": "object", "properties": {
            "to": to, "cc": to, "subject": {"type": "string", "description": "Leave out for a reply to keep Re: and the thread subject."},
            "body": {"type": "string", "description": "Plain text."},
            "reply_to_thread_id": {"type": "string", "description": "thread_id from gmail_search, to reply in that thread."}
        }, "required": ["to", "body"]});
        vec![
            ToolSpec { name: "gmail_search", description: "Search the user's Gmail with Gmail search syntax (for example is:unread newer_than:2d, from:priya). Returns threads with sender, subject and snippet.", schema: json!({"type": "object", "properties": {"query": {"type": "string"}, "max": {"type": "integer", "minimum": 1, "maximum": 20}}, "required": ["query"]}), risk: Risk::Read },
            ToolSpec { name: "gmail_read_thread", description: "Read the messages of one Gmail thread (last 10).", schema: json!({"type": "object", "properties": {"thread_id": {"type": "string"}}, "required": ["thread_id"]}), risk: Risk::Read },
            ToolSpec { name: "gmail_draft", description: "Save an email as a Gmail draft. Nothing is sent. Use this by default when the user wants a reply written.", schema: compose.clone(), risk: Risk::Safe },
            ToolSpec { name: "gmail_send", description: "Send an email or a reply. The user sees a card with the exact email and must approve. Only when the user asks to send.", schema: compose, risk: Risk::Write },
        ]
    }

    fn status(&self) -> ConnectorStatus {
        self.auth.status()
    }

    async fn call(&self, tool: &str, args: Value) -> Result<Value, String> {
        match tool {
            "gmail_search" => self.search(&args).await,
            "gmail_read_thread" => self.read_thread(&args).await,
            "gmail_draft" => self.draft(&args).await,
            _ => Err(format!("Unknown tool: {tool}")),
        }
    }

    async fn prepare(&self, tool: &str, args: Value) -> Result<Value, String> {
        // No card for an email bleep cannot send.
        if !matches!(self.auth.status(), ConnectorStatus::Connected { .. }) {
            return Err(crate::google::NOT_CONNECTED.into());
        }
        match tool {
            "gmail_send" => self.prepare_send(&args).await,
            _ => Err(format!("Unknown tool: {tool}")),
        }
    }

    async fn execute(&self, tool: &str, payload: Value) -> Result<Value, String> {
        match tool {
            "gmail_send" => self.execute_send(&payload).await,
            _ => Err(format!("Unknown tool: {tool}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::GoogleConfig;
    use crate::google::mock::{fixture, MockGoogle};
    use crate::google::secrets::{MemorySecrets, Secrets};
    use crate::google::{NOT_CONNECTED, REFRESH_KEY};
    use crate::opener::RecordingOpener;
    use crate::store::ApprovalStatus;
    use base64::Engine;
    use serde_json::json;

    const THREADS: &str = "GET /gmail/v1/users/me/threads";
    const MESSAGES: &str = "GET /gmail/v1/users/me/messages";
    const DRAFTS: &str = "POST /gmail/v1/users/me/drafts";
    const SEND: &str = "POST /gmail/v1/users/me/messages/send";

    struct Rig {
        _d: tempfile::TempDir,
        mock: MockGoogle,
        gm: Arc<GmailConnector>,
        store: Arc<Store>,
    }

    async fn rig(signed_in: bool) -> Rig {
        let d = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::open(&d.path().join("b.db")).unwrap());
        let mock = MockGoogle::start().await;
        mock.on("POST /token", vec![(200, fixture("token_refresh.json"))]);
        mock.on(THREADS, vec![(200, fixture("gmail_threads_list.json"))]);
        mock.on_query("GET /gmail/v1/users/me/threads/t1", "format=metadata", (200, fixture("gmail_thread_t1_meta.json")));
        mock.on_query("GET /gmail/v1/users/me/threads/t2", "metadataHeaders=Message-ID", (200, fixture("gmail_thread_t2_reply_meta.json")));
        mock.on_query("GET /gmail/v1/users/me/threads/t2", "format=metadata", (200, fixture("gmail_thread_t2_meta.json")));
        mock.on_query("GET /gmail/v1/users/me/threads/t2", "format=full", (200, fixture("gmail_thread_t2_full.json")));
        mock.on_query(MESSAGES, "priya%40example.com", (200, fixture("gmail_search_hit.json")));
        mock.on(MESSAGES, vec![(200, fixture("gmail_search_none.json"))]);
        mock.on(DRAFTS, vec![(200, fixture("gmail_draft_created.json"))]);
        mock.on(SEND, vec![(200, fixture("gmail_message_sent.json"))]);
        let secrets = Arc::new(MemorySecrets::default());
        if signed_in {
            secrets.set(REFRESH_KEY, "rt").unwrap();
        }
        let auth = GoogleAuth::new(GoogleConfig { client_id: "c".into(), client_secret: "s".into() }, mock.endpoints(), secrets, Arc::new(RecordingOpener::default()));
        let gm = GmailConnector::new(auth, reqwest::Client::new(), format!("{}/gmail/v1", mock.base), store.clone());
        Rig { _d: d, mock, gm, store }
    }

    fn raw_of(body: &str) -> String {
        let v: Value = serde_json::from_str(body).unwrap();
        let raw = v["raw"].as_str().or(v["message"]["raw"].as_str()).unwrap();
        String::from_utf8(base64::engine::general_purpose::URL_SAFE.decode(raw).unwrap()).unwrap()
    }

    #[tokio::test]
    async fn search_lists_threads_with_untrusted_fields() {
        let r = rig(true).await;
        let out = r.gm.call("gmail_search", json!({"query": "is:unread newer_than:2d"})).await.unwrap();
        let t = out["threads"].as_array().unwrap();
        assert_eq!(t.len(), 2);
        assert_eq!(t[0]["thread_id"], "t1");
        assert_eq!(t[0]["from_untrusted"], "City Power <bills@citypower.example>");
        assert_eq!(t[0]["subject_untrusted"], "Bill due today");
        assert_eq!(t[0]["snippet_untrusted"], "Your bill of ₹2,340 is due today");
        assert_eq!(t[0]["unread"], true);
        assert_eq!(t[1]["unread"], false);
        assert!(t[0].get("from").is_none() && t[0].get("subject").is_none());
        let h = &r.mock.hits_for(THREADS)[0];
        assert!(h.query.contains("q=is%3Aunread+newer_than%3A2d") || h.query.contains("q=is%3Aunread%20newer_than%3A2d"), "{}", h.query);
        assert!(h.query.contains("maxResults=10"), "{}", h.query);
        assert_eq!(h.auth.as_deref(), Some("Bearer at-refresh"));
    }

    #[tokio::test]
    async fn read_thread_decodes_bodies() {
        let r = rig(true).await;
        let out = r.gm.call("gmail_read_thread", json!({"thread_id": "t2"})).await.unwrap();
        let m = out["messages"].as_array().unwrap();
        assert_eq!(m.len(), 2);
        assert_eq!(m[0]["from_untrusted"], "Priya <priya@example.com>");
        assert!(m[0]["body_untrusted"].as_str().unwrap().starts_with("Can you review the deck by Friday?"));
        assert_eq!(m[1]["body_untrusted"], "Sure, tomorrow.\nThanks & regards");
        assert_eq!(m[1]["sent_by_me"], true);
        assert!(m[0].get("body").is_none());
    }

    #[tokio::test]
    async fn draft_saves_at_once_with_reply_headers() {
        let r = rig(true).await;
        let out = r.gm.call("gmail_draft", json!({"to": ["priya@example.com"], "body": "Done, see comments.", "reply_to_thread_id": "t2"})).await.unwrap();
        assert_eq!(out["draft_id"], "r-draft-1");
        let h = &r.mock.hits_for(DRAFTS)[0];
        let v: Value = serde_json::from_str(&h.body).unwrap();
        assert_eq!(v["message"]["threadId"], "t2");
        let text = raw_of(&h.body);
        assert!(text.contains("Subject: Re: Deck review\r\n"), "{text}");
        assert!(text.contains("In-Reply-To: <deck-2@example.com>\r\n"), "{text}");
        assert!(text.contains("References: <deck-1@example.com> <deck-2@example.com>\r\n"), "{text}");
        assert!(r.mock.hits_for(SEND).is_empty(), "a draft never sends");
    }

    #[tokio::test]
    async fn send_prepare_flags_new_recipients() {
        let r = rig(true).await;
        let p = r.gm.prepare("gmail_send", json!({"to": "Priya@Example.com", "cc": ["stranger@other.example"], "subject": "Deck", "body": "Looks good."})).await.unwrap();
        assert_eq!(p["to"], json!(["priya@example.com"]));
        assert_eq!(p["cc"], json!(["stranger@other.example"]));
        assert_eq!(p["subject"], "Deck");
        assert_eq!(p["body"], "Looks good.");
        assert_eq!(p["new_recipients"], json!(["stranger@other.example"]));
        assert!(r.mock.hits_for(SEND).is_empty(), "prepare must not send");
        let e = r.gm.prepare("gmail_send", json!({"to": ["not-an-email"], "subject": "x", "body": "y"})).await.unwrap_err();
        assert!(e.contains("email"), "{e}");
        assert!(r.gm.prepare("gmail_send", json!({"to": ["a@b.co"], "subject": "x", "body": "  "})).await.is_err());
    }

    #[tokio::test]
    async fn send_execute_posts_exact_payload() {
        let r = rig(true).await;
        let p = r.gm.prepare("gmail_send", json!({"to": ["priya@example.com"], "body": "Done.", "reply_to_thread_id": "t2"})).await.unwrap();
        assert_eq!(p["thread_id"], "t2");
        assert_eq!(p["subject"], "Re: Deck review");
        let out = r.gm.execute("gmail_send", p).await.unwrap();
        assert_eq!(out["sent"], true);
        let h = &r.mock.hits_for(SEND)[0];
        let v: Value = serde_json::from_str(&h.body).unwrap();
        assert_eq!(v["threadId"], "t2");
        let text = raw_of(&h.body);
        assert!(text.contains("To: priya@example.com\r\n") && text.contains("Subject: Re: Deck review\r\n") && text.contains("In-Reply-To: <deck-2@example.com>\r\n"), "{text}");
    }

    #[tokio::test]
    async fn send_daily_limit_refuses_eleventh() {
        let r = rig(true).await;
        for _ in 0..MAX_SENDS_PER_DAY {
            let id = r.store.insert_approval("gmail_send", &json!({}), None).unwrap();
            r.store.set_approval_status(id, ApprovalStatus::Approved, None).unwrap();
            r.store.set_approval_status(id, ApprovalStatus::Done, Some("ok")).unwrap();
        }
        let e = r.gm.prepare("gmail_send", json!({"to": ["priya@example.com"], "subject": "x", "body": "y"})).await.unwrap_err();
        assert!(e.contains("10 emails today"), "{e}");
    }

    #[tokio::test]
    async fn not_connected_says_so() {
        let r = rig(false).await;
        assert_eq!(r.gm.call("gmail_search", json!({"query": "x"})).await, Err(NOT_CONNECTED.to_string()));
        assert_eq!(r.gm.call("gmail_draft", json!({"to": ["a@b.co"], "subject": "x", "body": "y"})).await, Err(NOT_CONNECTED.to_string()));
        assert_eq!(r.gm.prepare("gmail_send", json!({"to": ["a@b.co"], "subject": "x", "body": "y"})).await, Err(NOT_CONNECTED.to_string()));
    }

    #[tokio::test]
    async fn bad_thread_id_is_refused() {
        let r = rig(true).await;
        assert!(r.gm.call("gmail_read_thread", json!({"thread_id": "../drafts"})).await.is_err());
        assert!(r.mock.hits.lock().unwrap().is_empty());
    }

    #[test]
    fn tools_have_the_planned_names_and_risks() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let r = rt.block_on(rig(false));
        let got: Vec<(&str, Risk)> = r.gm.tools().iter().map(|t| (t.name, t.risk)).collect();
        assert_eq!(got, vec![("gmail_search", Risk::Read), ("gmail_read_thread", Risk::Read), ("gmail_draft", Risk::Safe), ("gmail_send", Risk::Write)]);
    }
}
