//! SQLite store: one chat history and the runtime sessions behind it.
use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;
use std::sync::Mutex;
use chrono::{DateTime, Utc};
use crate::connectors::calendar::CalEvent;

macro_rules! str_enum {
    ($name:ident { $($v:ident = $s:literal),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(rename_all = "lowercase")]
        pub enum $name { $($v),+ }
        impl $name {
            pub fn as_str(self) -> &'static str { match self { $(Self::$v => $s),+ } }
            fn parse(s: &str) -> Self { match s { $($s => Self::$v,)+ _ => panic!("bad {} in db: {s}", stringify!($name)) } }
        }
    };
}

str_enum!(Role { User = "user", Assistant = "assistant", System = "system" });
str_enum!(Kind { Chat = "chat", Scheduled = "scheduled", System = "system", Approval = "approval", Divider = "divider" });
str_enum!(MsgStatus { Streaming = "streaming", Done = "done", Error = "error", Interrupted = "interrupted" });
str_enum!(ApprovalStatus { Pending = "pending", Approved = "approved", Declined = "declined", Expired = "expired", Done = "done", Failed = "failed" });

impl ApprovalStatus {
    /// Statuses only move forward: a decided card never becomes pending or approved again.
    pub fn can_become(self, to: ApprovalStatus) -> bool {
        use ApprovalStatus::*;
        matches!((self, to), (Pending, Approved | Declined | Expired) | (Approved, Done | Failed))
    }
}

/// One approval card: the exact payload that runs if the user approves.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Approval {
    pub id: i64,
    pub tool: String,
    pub payload: Value,
    pub status: ApprovalStatus,
    pub turn_message_id: Option<i64>,
    pub result: Option<String>,
    pub created_at: String,
    pub decided_at: Option<String>,
}

pub struct NewMessage {
    pub role: Role,
    pub kind: Kind,
    pub content: String,
    pub status: MsgStatus,
    pub session_id: Option<i64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Message {
    pub id: i64,
    pub role: Role,
    pub kind: Kind,
    pub content: String,
    pub tool_steps: Value,
    pub tokens: Option<i64>,
    pub status: MsgStatus,
    pub session_id: Option<i64>,
    pub created_at: String,
    /// Approval cards asked for during this turn.
    pub approvals: Vec<Approval>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Session {
    pub id: i64,
    pub runtime: String,
    pub runtime_session_id: Option<String>,
    pub started_at: String,
    pub ended_at: Option<String>,
}

const MIGRATIONS: &[&str] = &[r#"
CREATE TABLE sessions (
  id INTEGER PRIMARY KEY,
  runtime TEXT NOT NULL,
  runtime_session_id TEXT,
  started_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  ended_at TEXT,
  summary TEXT,
  tokens_at_end INTEGER
);
CREATE TABLE messages (
  id INTEGER PRIMARY KEY,
  role TEXT NOT NULL,
  kind TEXT NOT NULL,
  content TEXT NOT NULL,
  tool_steps TEXT NOT NULL DEFAULT '[]',
  tokens INTEGER,
  status TEXT NOT NULL,
  session_id INTEGER REFERENCES sessions(id),
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
"#, r#"
CREATE TABLE approvals (
  id INTEGER PRIMARY KEY,
  tool TEXT NOT NULL,
  payload TEXT NOT NULL,
  status TEXT NOT NULL,
  turn_message_id INTEGER REFERENCES messages(id),
  result TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  decided_at TEXT
);
CREATE INDEX approvals_turn ON approvals(turn_message_id);
CREATE TABLE cal_cache (
  event_id TEXT NOT NULL,
  start TEXT NOT NULL,
  "end" TEXT NOT NULL,
  title TEXT NOT NULL,
  attendees TEXT NOT NULL DEFAULT '[]',
  meet_link TEXT,
  all_day INTEGER NOT NULL,
  my_response TEXT,
  html_link TEXT,
  description TEXT,
  updated TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  PRIMARY KEY (event_id, start)
);
CREATE TABLE pings (
  id INTEGER PRIMARY KEY,
  kind TEXT NOT NULL,
  ref_id TEXT NOT NULL,
  sent_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  feedback TEXT
);
CREATE UNIQUE INDEX pings_kind_ref ON pings(kind, ref_id);
"#];

pub struct Store {
    conn: Mutex<Connection>,
}

fn row_to_message(r: &Row) -> rusqlite::Result<Message> {
    let steps: String = r.get(4)?;
    Ok(Message {
        id: r.get(0)?,
        role: Role::parse(&r.get::<_, String>(1)?),
        kind: Kind::parse(&r.get::<_, String>(2)?),
        content: r.get(3)?,
        tool_steps: serde_json::from_str(&steps).unwrap_or(Value::Array(vec![])),
        tokens: r.get(5)?,
        status: MsgStatus::parse(&r.get::<_, String>(6)?),
        session_id: r.get(7)?,
        created_at: r.get(8)?,
        approvals: vec![],
    })
}

fn row_to_approval(r: &Row) -> rusqlite::Result<Approval> {
    let payload: String = r.get(2)?;
    Ok(Approval {
        id: r.get(0)?,
        tool: r.get(1)?,
        payload: serde_json::from_str(&payload).unwrap_or(Value::Null),
        status: ApprovalStatus::parse(&r.get::<_, String>(3)?),
        turn_message_id: r.get(4)?,
        result: r.get(5)?,
        created_at: r.get(6)?,
        decided_at: r.get(7)?,
    })
}

/// Times in the cache are UTC RFC 3339 to the second, so text order is time order.
fn ts(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn row_to_cal(r: &Row) -> rusqlite::Result<CalEvent> {
    let when = |i: usize| -> rusqlite::Result<DateTime<Utc>> {
        let s: String = r.get(i)?;
        DateTime::parse_from_rfc3339(&s).map(|d| d.with_timezone(&Utc)).map_err(|e| rusqlite::Error::FromSqlConversionFailure(i, rusqlite::types::Type::Text, Box::new(e)))
    };
    let attendees: String = r.get(4)?;
    Ok(CalEvent {
        event_id: r.get(0)?,
        start: when(1)?,
        end: when(2)?,
        title: r.get(3)?,
        attendees: serde_json::from_str(&attendees).unwrap_or_default(),
        meet_link: r.get(5)?,
        all_day: r.get(6)?,
        my_response: r.get(7)?,
        html_link: r.get(8)?,
        description_untrusted: r.get(9)?,
    })
}

const CAL_COLS: &str = "event_id, start, \"end\", title, attendees, meet_link, all_day, my_response, html_link, description";

/// What an approved card says when bleep quit before the change finished.
pub const UNFINISHED: &str = "bleep closed before this finished. Check your calendar.";

const APPROVAL_COLS: &str = "id, tool, payload, status, turn_message_id, result, created_at, decided_at";

fn row_to_session(r: &Row) -> rusqlite::Result<Session> {
    Ok(Session { id: r.get(0)?, runtime: r.get(1)?, runtime_session_id: r.get(2)?, started_at: r.get(3)?, ended_at: r.get(4)? })
}

const MSG_COLS: &str = "id, role, kind, content, tool_steps, tokens, status, session_id, created_at";
const SESSION_COLS: &str = "id, runtime, runtime_session_id, started_at, ended_at";

impl Store {
    pub fn open(path: &Path) -> Result<Store> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(version as usize) {
            conn.execute_batch(sql)?;
            conn.pragma_update(None, "user_version", (i + 1) as i64)?;
        }
        Ok(Store { conn: Mutex::new(conn) })
    }

    fn db(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn journal_mode(&self) -> Result<String> {
        Ok(self.db().pragma_query_value(None, "journal_mode", |r| r.get(0))?)
    }

    pub fn insert_message(&self, m: NewMessage) -> Result<i64> {
        let db = self.db();
        db.execute(
            "INSERT INTO messages (role, kind, content, status, session_id) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![m.role.as_str(), m.kind.as_str(), m.content, m.status.as_str(), m.session_id],
        )?;
        Ok(db.last_insert_rowid())
    }

    pub fn finish_message(&self, id: i64, content: &str, tool_steps: &Value, tokens: Option<i64>, status: MsgStatus) -> Result<()> {
        self.db().execute(
            "UPDATE messages SET content = ?2, tool_steps = ?3, tokens = ?4, status = ?5 WHERE id = ?1",
            params![id, content, tool_steps.to_string(), tokens, status.as_str()],
        )?;
        Ok(())
    }

    /// Up to `limit` messages older than `before_id` (or the newest), oldest first.
    pub fn page_messages(&self, before_id: Option<i64>, limit: u32) -> Result<Vec<Message>> {
        let db = self.db();
        let sql = format!("SELECT {MSG_COLS} FROM messages WHERE id < ?1 ORDER BY id DESC LIMIT ?2");
        let mut rows: Vec<Message> = db
            .prepare(&sql)?
            .query_map(params![before_id.unwrap_or(i64::MAX), limit], row_to_message)?
            .collect::<rusqlite::Result<_>>()?;
        rows.reverse();
        if let (Some(lo), Some(hi)) = (rows.first().map(|m| m.id), rows.last().map(|m| m.id)) {
            let sql = format!("SELECT {APPROVAL_COLS} FROM approvals WHERE turn_message_id BETWEEN ?1 AND ?2 ORDER BY id");
            let cards: Vec<Approval> = db.prepare(&sql)?.query_map(params![lo, hi], row_to_approval)?.collect::<rusqlite::Result<_>>()?;
            for a in cards {
                if let Some(m) = rows.iter_mut().find(|m| Some(m.id) == a.turn_message_id) {
                    m.approvals.push(a);
                }
            }
        }
        Ok(rows)
    }

    pub fn insert_approval(&self, tool: &str, payload: &Value, turn: Option<i64>) -> Result<i64> {
        let db = self.db();
        db.execute(
            "INSERT INTO approvals (tool, payload, status, turn_message_id) VALUES (?1, ?2, 'pending', ?3)",
            params![tool, payload.to_string(), turn],
        )?;
        Ok(db.last_insert_rowid())
    }

    /// Moves a card to `to` only along `ApprovalStatus::can_become`. Returns false otherwise.
    pub fn set_approval_status(&self, id: i64, to: ApprovalStatus, result: Option<&str>) -> Result<bool> {
        let db = self.db();
        let Some(from) = db.query_row("SELECT status FROM approvals WHERE id = ?1", params![id], |r| r.get::<_, String>(0)).optional()? else {
            return Ok(false);
        };
        if !ApprovalStatus::parse(&from).can_become(to) {
            return Ok(false);
        }
        let n = db.execute(
            "UPDATE approvals SET status = ?2, result = COALESCE(?3, result), decided_at = COALESCE(decided_at, strftime('%Y-%m-%dT%H:%M:%fZ','now')) WHERE id = ?1 AND status = ?4",
            params![id, to.as_str(), result, from],
        )?;
        Ok(n == 1)
    }

    pub fn get_approval(&self, id: i64) -> Result<Option<Approval>> {
        let sql = format!("SELECT {APPROVAL_COLS} FROM approvals WHERE id = ?1");
        Ok(self.db().query_row(&sql, params![id], row_to_approval).optional()?)
    }

    /// Replaces cached events that start inside `[from, to)` with `events`.
    pub fn replace_cal_window(&self, from: DateTime<Utc>, to: DateTime<Utc>, events: &[CalEvent]) -> Result<()> {
        let mut db = self.db();
        let tx = db.transaction()?;
        tx.execute("DELETE FROM cal_cache WHERE start >= ?1 AND start < ?2", params![ts(from), ts(to)])?;
        for e in events {
            tx.execute(
                "INSERT OR REPLACE INTO cal_cache (event_id, start, \"end\", title, attendees, meet_link, all_day, my_response, html_link, description) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![e.event_id, ts(e.start), ts(e.end), e.title, serde_json::to_string(&e.attendees)?, e.meet_link, e.all_day, e.my_response, e.html_link, e.description_untrusted],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Cached events that have not ended by `now` and start before `until`, soonest first.
    pub fn cal_upcoming(&self, now: DateTime<Utc>, until: DateTime<Utc>) -> Result<Vec<CalEvent>> {
        let sql = format!("SELECT {CAL_COLS} FROM cal_cache WHERE \"end\" > ?1 AND start < ?2 ORDER BY start");
        Ok(self.db().prepare(&sql)?.query_map(params![ts(now), ts(until)], row_to_cal)?.collect::<rusqlite::Result<_>>()?)
    }

    pub fn cal_event(&self, event_id: &str) -> Result<Option<CalEvent>> {
        let sql = format!("SELECT {CAL_COLS} FROM cal_cache WHERE event_id = ?1 ORDER BY start LIMIT 1");
        Ok(self.db().query_row(&sql, params![event_id], row_to_cal).optional()?)
    }

    pub fn clear_cal_cache(&self) -> Result<()> {
        self.db().execute("DELETE FROM cal_cache", [])?;
        Ok(())
    }

    /// Records a ping once. Returns false if it was already recorded.
    pub fn record_ping(&self, kind: &str, ref_id: &str) -> Result<bool> {
        Ok(self.db().execute("INSERT OR IGNORE INTO pings (kind, ref_id) VALUES (?1, ?2)", params![kind, ref_id])? == 1)
    }

    pub fn has_ping(&self, kind: &str, ref_id: &str) -> Result<bool> {
        Ok(self.db().query_row("SELECT 1 FROM pings WHERE kind = ?1 AND ref_id = ?2", params![kind, ref_id], |_| Ok(())).optional()?.is_some())
    }

    /// How many `tool` cards finished (status done) at or after `since`.
    pub fn count_done_since(&self, tool: &str, since: DateTime<Utc>) -> Result<usize> {
        let n: i64 = self.db().query_row(
            "SELECT COUNT(*) FROM approvals WHERE tool = ?1 AND status = 'done' AND created_at >= ?2",
            params![tool, ts(since)],
            |r| r.get(0),
        )?;
        Ok(n as usize)
    }

    /// Cards still waiting when the app quit can never be answered: mark them expired.
    /// Approved cards that never finished may or may not have run: mark them failed with a
    /// note to check the calendar.
    pub fn expire_pending_approvals(&self) -> Result<usize> {
        let db = self.db();
        let pending = db.execute(
            "UPDATE approvals SET status = 'expired', decided_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE status = 'pending'",
            [],
        )?;
        let approved = db.execute("UPDATE approvals SET status = 'failed', result = ?1 WHERE status = 'approved'", params![UNFINISHED])?;
        Ok(pending + approved)
    }

    pub fn mark_streaming_as_interrupted(&self) -> Result<usize> {
        Ok(self.db().execute("UPDATE messages SET status = 'interrupted' WHERE status = 'streaming'", [])?)
    }

    pub fn current_session(&self, runtime: &str) -> Result<Option<Session>> {
        let sql = format!("SELECT {SESSION_COLS} FROM sessions WHERE runtime = ?1 AND ended_at IS NULL ORDER BY id DESC LIMIT 1");
        Ok(self.db().query_row(&sql, params![runtime], row_to_session).optional()?)
    }

    pub fn create_session(&self, runtime: &str) -> Result<Session> {
        let db = self.db();
        db.execute("INSERT INTO sessions (runtime) VALUES (?1)", params![runtime])?;
        let sql = format!("SELECT {SESSION_COLS} FROM sessions WHERE id = ?1");
        Ok(db.query_row(&sql, params![db.last_insert_rowid()], row_to_session)?)
    }

    pub fn set_runtime_session_id(&self, id: i64, rsid: &str) -> Result<()> {
        self.db().execute("UPDATE sessions SET runtime_session_id = ?2 WHERE id = ?1", params![id, rsid])?;
        Ok(())
    }

    pub fn end_session(&self, id: i64) -> Result<()> {
        self.db().execute(
            "UPDATE sessions SET ended_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(content: &str, status: MsgStatus) -> NewMessage {
        NewMessage { role: Role::User, kind: Kind::Chat, content: content.into(), status, session_id: None }
    }

    #[test]
    fn history_survives_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("bleep.db");
        {
            let s = Store::open(&p).unwrap();
            for i in 0..3 {
                s.insert_message(msg(&format!("m{i}"), MsgStatus::Done)).unwrap();
            }
        }
        let s = Store::open(&p).unwrap();
        let page = s.page_messages(None, 50).unwrap();
        assert_eq!(page.len(), 3);
        assert_eq!(page[0].content, "m0");
    }

    #[test]
    fn paging_goes_backwards() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::open(&dir.path().join("b.db")).unwrap();
        for i in 1..=120 {
            s.insert_message(msg(&i.to_string(), MsgStatus::Done)).unwrap();
        }
        let p1 = s.page_messages(None, 50).unwrap();
        assert_eq!(p1.first().unwrap().id, 71);
        assert_eq!(p1.last().unwrap().id, 120);
        let p2 = s.page_messages(Some(71), 50).unwrap();
        assert_eq!(p2.last().unwrap().id, 70);
        assert_eq!(p2.len(), 50);
    }

    #[test]
    fn wal_mode_on() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::open(&dir.path().join("b.db")).unwrap();
        assert_eq!(s.journal_mode().unwrap(), "wal");
    }

    #[test]
    fn streaming_rows_become_interrupted() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::open(&dir.path().join("b.db")).unwrap();
        let id = s.insert_message(msg("half", MsgStatus::Streaming)).unwrap();
        s.insert_message(msg("whole", MsgStatus::Done)).unwrap();
        assert_eq!(s.mark_streaming_as_interrupted().unwrap(), 1);
        let m = s.page_messages(None, 50).unwrap().into_iter().find(|m| m.id == id).unwrap();
        assert_eq!(m.status, MsgStatus::Interrupted);
    }

    #[test]
    fn finish_message_saves_content_steps_tokens() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::open(&dir.path().join("b.db")).unwrap();
        let id = s.insert_message(msg("", MsgStatus::Streaming)).unwrap();
        let steps = serde_json::json!([{"name": "bleep_now", "ok": true}]);
        s.finish_message(id, "hi", &steps, Some(42), MsgStatus::Done).unwrap();
        let m = &s.page_messages(None, 1).unwrap()[0];
        assert_eq!((m.content.as_str(), m.tokens, m.status), ("hi", Some(42), MsgStatus::Done));
        assert_eq!(m.tool_steps[0]["name"], "bleep_now");
    }

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::open(&dir.path().join("b.db")).unwrap();
        (dir, s)
    }

    #[test]
    fn pending_approvals_expire_on_startup() {
        let (_d, s) = store();
        let a = s.insert_approval("calendar_create", &serde_json::json!({"title": "x"}), None).unwrap();
        let b = s.insert_approval("calendar_create", &serde_json::json!({}), None).unwrap();
        assert!(s.set_approval_status(b, ApprovalStatus::Declined, None).unwrap());
        assert_eq!(s.expire_pending_approvals().unwrap(), 1);
        assert_eq!(s.get_approval(a).unwrap().unwrap().status, ApprovalStatus::Expired);
        assert_eq!(s.get_approval(b).unwrap().unwrap().status, ApprovalStatus::Declined);
        assert!(s.get_approval(a).unwrap().unwrap().decided_at.is_some());
    }

    #[test]
    fn approved_cards_fail_on_startup() {
        let (_d, s) = store();
        let a = s.insert_approval("calendar_create", &serde_json::json!({}), None).unwrap();
        assert!(s.set_approval_status(a, ApprovalStatus::Approved, None).unwrap());
        assert_eq!(s.expire_pending_approvals().unwrap(), 1);
        let got = s.get_approval(a).unwrap().unwrap();
        assert_eq!(got.status, ApprovalStatus::Failed);
        assert_eq!(got.result.as_deref(), Some(UNFINISHED));
    }

    #[test]
    fn approval_status_moves_only_forward() {
        let (_d, s) = store();
        let a = s.insert_approval("t", &serde_json::json!({}), None).unwrap();
        assert!(s.set_approval_status(a, ApprovalStatus::Approved, None).unwrap());
        assert!(!s.set_approval_status(a, ApprovalStatus::Declined, None).unwrap());
        assert!(s.set_approval_status(a, ApprovalStatus::Done, Some("ok")).unwrap());
        assert!(!s.set_approval_status(a, ApprovalStatus::Approved, None).unwrap());
        assert!(!s.set_approval_status(a, ApprovalStatus::Failed, None).unwrap());
        let got = s.get_approval(a).unwrap().unwrap();
        assert_eq!((got.status, got.result.as_deref()), (ApprovalStatus::Done, Some("ok")));
        let d = s.insert_approval("t", &serde_json::json!({}), None).unwrap();
        assert!(s.set_approval_status(d, ApprovalStatus::Declined, None).unwrap());
        assert!(!s.set_approval_status(d, ApprovalStatus::Approved, None).unwrap());
        assert!(s.get_approval(999).unwrap().is_none());
    }

    #[test]
    fn history_carries_approvals_of_their_turn() {
        let (_d, s) = store();
        let turn = s.insert_message(msg("", MsgStatus::Streaming)).unwrap();
        let other = s.insert_message(msg("x", MsgStatus::Done)).unwrap();
        let payload = serde_json::json!({"title": "Pay rent"});
        let a = s.insert_approval("calendar_create", &payload, Some(turn)).unwrap();
        let page = s.page_messages(None, 50).unwrap();
        let t = page.iter().find(|m| m.id == turn).unwrap();
        assert_eq!(t.approvals.len(), 1);
        assert_eq!((t.approvals[0].id, &t.approvals[0].payload, t.approvals[0].status), (a, &payload, ApprovalStatus::Pending));
        assert!(page.iter().find(|m| m.id == other).unwrap().approvals.is_empty());
    }

    fn cal(id: &str, start: &str) -> CalEvent {
        let s = DateTime::parse_from_rfc3339(start).unwrap().with_timezone(&Utc);
        CalEvent {
            event_id: id.into(),
            title: format!("T {id}"),
            start: s,
            end: s + chrono::Duration::minutes(30),
            all_day: false,
            attendees: vec!["a@b.co".into()],
            meet_link: Some("https://meet.google.com/x".into()),
            my_response: Some("accepted".into()),
            html_link: None,
            description_untrusted: Some("notes".into()),
        }
    }

    fn utc(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    #[test]
    fn replace_window_drops_cancelled_events_and_keeps_others() {
        let (_d, s) = store();
        let (from, to) = (utc("2026-11-12T08:00:00Z"), utc("2026-11-13T20:00:00Z"));
        let outside = cal("far", "2026-11-20T10:00:00Z");
        s.replace_cal_window(utc("2026-11-20T00:00:00Z"), utc("2026-11-21T00:00:00Z"), &[outside.clone()]).unwrap();
        s.replace_cal_window(from, to, &[cal("a", "2026-11-12T10:00:00Z"), cal("b", "2026-11-12T11:00:00Z")]).unwrap();
        s.replace_cal_window(from, to, &[cal("a", "2026-11-12T10:00:00Z")]).unwrap();
        let all = s.cal_upcoming(utc("2026-11-12T00:00:00Z"), utc("2026-12-01T00:00:00Z")).unwrap();
        assert_eq!(all.iter().map(|e| e.event_id.as_str()).collect::<Vec<_>>(), vec!["a", "far"]);
        assert_eq!(all[0], cal("a", "2026-11-12T10:00:00Z"));
        assert_eq!(s.cal_event("far").unwrap(), Some(outside));
        // Ended events are not upcoming.
        assert!(s.cal_upcoming(utc("2026-11-12T10:31:00Z"), utc("2026-11-13T00:00:00Z")).unwrap().is_empty());
        s.clear_cal_cache().unwrap();
        assert!(s.cal_event("a").unwrap().is_none());
    }

    #[test]
    fn pings_are_recorded_once() {
        let (_d, s) = store();
        assert!(!s.has_ping("meeting_bar", "k").unwrap());
        assert!(s.record_ping("meeting_bar", "k").unwrap());
        assert!(!s.record_ping("meeting_bar", "k").unwrap());
        assert!(s.has_ping("meeting_bar", "k").unwrap());
        assert!(!s.has_ping("other", "k").unwrap());
    }

    #[test]
    fn current_session_ignores_ended() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::open(&dir.path().join("b.db")).unwrap();
        assert!(s.current_session("claude").unwrap().is_none());
        let a = s.create_session("claude").unwrap();
        s.set_runtime_session_id(a.id, "rs-1").unwrap();
        assert_eq!(s.current_session("claude").unwrap().unwrap().runtime_session_id.as_deref(), Some("rs-1"));
        s.end_session(a.id).unwrap();
        assert!(s.current_session("claude").unwrap().is_none());
        assert!(s.current_session("codex").unwrap().is_none());
    }
}
