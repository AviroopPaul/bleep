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
str_enum!(Label { Ignore = "ignore", Fyi = "fyi", Important = "important", NeedsReply = "needs_reply", Bill = "bill" });
str_enum!(ApprovalStatus { Pending = "pending", Approved = "approved", Declined = "declined", Expired = "expired", Done = "done", Failed = "failed" });

impl ApprovalStatus {
    /// Statuses only move forward: a decided card never becomes pending or approved again.
    pub fn can_become(self, to: ApprovalStatus) -> bool {
        use ApprovalStatus::*;
        matches!((self, to), (Pending, Approved | Declined | Expired) | (Approved, Done | Failed))
    }
}

/// One email bleep has seen. `subject` and `snippet` are other people's text.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct MailRow {
    pub message_id: String,
    pub thread_id: String,
    pub from_addr: String,
    pub from_name: Option<String>,
    pub to_me: bool,
    pub from_me: bool,
    pub subject: String,
    pub snippet: String,
    pub labels: Vec<String>,
    pub received: DateTime<Utc>,
    pub triage: Option<Label>,
    pub amount: Option<String>,
    pub due: Option<chrono::NaiveDate>,
    pub reason: Option<String>,
    pub seeded: bool,
    pub paid: bool,
}

str_enum!(AgentTool { Claude = "claude", Codex = "codex" });
str_enum!(AgentState { Running = "running", Waiting = "waiting", Done = "done" });
str_enum!(AskKind { Permission = "permission", Question = "question" });
str_enum!(AskDecision { Auto = "auto", Approved = "approved", Denied = "denied", Queued = "queued", Expired = "expired" });

/// One of my coding sessions. `id` is "claude:<session id>" or "codex:<thread id>".
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct AgentSession {
    pub id: String,
    pub tool: AgentTool,
    /// The session's working folder.
    pub project: String,
    pub title: Option<String>,
    pub transcript_path: Option<String>,
    pub state: AgentState,
    /// One-line summary of what it waits for. Agent text: untrusted.
    pub last_ask: Option<String>,
    pub updated: DateTime<Utc>,
}

pub struct NewAsk {
    pub session_id: String,
    pub kind: AskKind,
    pub summary: String,
    pub payload: Value,
    /// Why rules did not approve it, or what was checked.
    pub checks: Vec<String>,
    pub decision: AskDecision,
    pub rule: Option<String>,
    /// The Codex JSON-RPC request id, to answer later.
    pub rpc_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct AgentAsk {
    pub id: i64,
    pub session_id: String,
    pub kind: AskKind,
    pub summary: String,
    pub payload: Value,
    pub checks: Vec<String>,
    pub decision: AskDecision,
    pub rule: Option<String>,
    pub rpc_id: Option<String>,
    pub created_at: String,
    pub decided_at: Option<String>,
}

/// One line in the Activity tab.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct ActivityItem {
    /// "tool", "approval" or "model".
    pub kind: String,
    /// Tool name, or a model call's purpose.
    pub name: String,
    /// A tool result preview, an approval status, or a model tier.
    pub detail: Option<String>,
    pub ok: bool,
    pub at: String,
    /// The agent rule that approved an ask.
    pub rule: Option<String>,
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
"#, r#"
CREATE TABLE mail_cache (
  message_id TEXT PRIMARY KEY,
  thread_id TEXT NOT NULL,
  from_addr TEXT NOT NULL,
  from_name TEXT,
  to_me INTEGER NOT NULL,
  from_me INTEGER NOT NULL,
  subject TEXT NOT NULL,
  snippet TEXT NOT NULL,
  labels TEXT NOT NULL DEFAULT '[]',
  received TEXT NOT NULL,
  triage TEXT,
  amount TEXT,
  due TEXT,
  reason TEXT,
  seeded INTEGER NOT NULL DEFAULT 0,
  paid INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX mail_thread ON mail_cache(thread_id, received);
CREATE TABLE cursors (connector TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE llm_calls (
  id INTEGER PRIMARY KEY,
  runtime TEXT NOT NULL,
  tier TEXT NOT NULL,
  purpose TEXT NOT NULL,
  tokens_in INTEGER,
  tokens_out INTEGER,
  ok INTEGER NOT NULL,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
CREATE TABLE tool_calls (
  id INTEGER PRIMARY KEY,
  tool TEXT NOT NULL,
  risk TEXT NOT NULL,
  ok INTEGER NOT NULL,
  ms INTEGER NOT NULL,
  preview TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
ALTER TABLE pings ADD COLUMN sender TEXT;
"#, r#"
CREATE TABLE agent_sessions (
  id TEXT PRIMARY KEY,
  tool TEXT NOT NULL,
  project TEXT NOT NULL,
  title TEXT,
  transcript_path TEXT,
  state TEXT NOT NULL,
  last_ask TEXT,
  go_ons TEXT NOT NULL DEFAULT '[]',
  ended INTEGER NOT NULL DEFAULT 0,
  updated TEXT NOT NULL
);
CREATE TABLE agent_asks (
  id INTEGER PRIMARY KEY,
  session_id TEXT NOT NULL,
  kind TEXT NOT NULL,
  summary TEXT NOT NULL,
  payload TEXT NOT NULL,
  checks TEXT NOT NULL DEFAULT '[]',
  decision TEXT NOT NULL,
  rule TEXT,
  rpc_id TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  decided_at TEXT
);
CREATE INDEX agent_asks_session ON agent_asks(session_id, created_at);
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

const MAIL_COLS: &str = "message_id, thread_id, from_addr, from_name, to_me, from_me, subject, snippet, labels, received, triage, amount, due, reason, seeded, paid";

fn row_to_mail(r: &Row) -> rusqlite::Result<MailRow> {
    let labels: String = r.get(8)?;
    let received: String = r.get(9)?;
    let due: Option<String> = r.get(12)?;
    Ok(MailRow {
        message_id: r.get(0)?,
        thread_id: r.get(1)?,
        from_addr: r.get(2)?,
        from_name: r.get(3)?,
        to_me: r.get(4)?,
        from_me: r.get(5)?,
        subject: r.get(6)?,
        snippet: r.get(7)?,
        labels: serde_json::from_str(&labels).unwrap_or_default(),
        received: DateTime::parse_from_rfc3339(&received).map(|d| d.with_timezone(&Utc)).map_err(|e| rusqlite::Error::FromSqlConversionFailure(9, rusqlite::types::Type::Text, Box::new(e)))?,
        triage: r.get::<_, Option<String>>(10)?.map(|s| Label::parse(&s)),
        amount: r.get(11)?,
        due: due.and_then(|d| chrono::NaiveDate::parse_from_str(&d, "%Y-%m-%d").ok()),
        reason: r.get(13)?,
        seeded: r.get(14)?,
        paid: r.get(15)?,
    })
}

const CAL_COLS: &str = "event_id, start, \"end\", title, attendees, meet_link, all_day, my_response, html_link, description";

/// What an approved card says when bleep quit before the change finished.
pub const UNFINISHED: &str = "bleep closed before this finished. Check your calendar.";

const APPROVAL_COLS: &str = "id, tool, payload, status, turn_message_id, result, created_at, decided_at";

fn row_to_session(r: &Row) -> rusqlite::Result<Session> {
    Ok(Session { id: r.get(0)?, runtime: r.get(1)?, runtime_session_id: r.get(2)?, started_at: r.get(3)?, ended_at: r.get(4)? })
}

const AGENT_COLS: &str = "id, tool, project, title, transcript_path, state, last_ask, updated";
const ASK_COLS: &str = "id, session_id, kind, summary, payload, checks, decision, rule, rpc_id, created_at, decided_at";

fn parse_ts(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s).map(|d| d.with_timezone(&Utc)).unwrap_or_default()
}

fn row_to_agent(r: &Row) -> rusqlite::Result<AgentSession> {
    Ok(AgentSession {
        id: r.get(0)?,
        tool: AgentTool::parse(&r.get::<_, String>(1)?),
        project: r.get(2)?,
        title: r.get(3)?,
        transcript_path: r.get(4)?,
        state: AgentState::parse(&r.get::<_, String>(5)?),
        last_ask: r.get(6)?,
        updated: parse_ts(&r.get::<_, String>(7)?),
    })
}

fn row_to_ask(r: &Row) -> rusqlite::Result<AgentAsk> {
    Ok(AgentAsk {
        id: r.get(0)?,
        session_id: r.get(1)?,
        kind: AskKind::parse(&r.get::<_, String>(2)?),
        summary: r.get(3)?,
        payload: serde_json::from_str(&r.get::<_, String>(4)?).unwrap_or(Value::Null),
        checks: serde_json::from_str(&r.get::<_, String>(5)?).unwrap_or_default(),
        decision: AskDecision::parse(&r.get::<_, String>(6)?),
        rule: r.get(7)?,
        rpc_id: r.get(8)?,
        created_at: r.get(9)?,
        decided_at: r.get(10)?,
    })
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

    /// Inserts a message, or refreshes its Gmail labels. Never touches bleep's own fields
    /// (label, amount, due, paid, seeded) on a message it already has.
    pub fn upsert_mail(&self, m: &MailRow) -> Result<()> {
        self.db().execute(
            "INSERT INTO mail_cache (message_id, thread_id, from_addr, from_name, to_me, from_me, subject, snippet, labels, received, triage, amount, due, reason, seeded, paid)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
             ON CONFLICT(message_id) DO UPDATE SET labels = excluded.labels",
            params![
                m.message_id, m.thread_id, m.from_addr, m.from_name, m.to_me, m.from_me, m.subject, m.snippet,
                serde_json::to_string(&m.labels)?, ts(m.received), m.triage.map(|l| l.as_str()), m.amount,
                m.due.map(|d| d.to_string()), m.reason, m.seeded, m.paid
            ],
        )?;
        Ok(())
    }

    fn mail_query(&self, filter: &str, p: &[&dyn rusqlite::ToSql]) -> Result<Vec<MailRow>> {
        let sql = format!("SELECT {MAIL_COLS} FROM mail_cache {filter}");
        Ok(self.db().prepare(&sql)?.query_map(p, row_to_mail)?.collect::<rusqlite::Result<_>>()?)
    }

    /// Rows with no label yet, oldest first.
    pub fn untriaged(&self, limit: usize) -> Result<Vec<MailRow>> {
        self.mail_query("WHERE triage IS NULL ORDER BY received LIMIT ?1", &[&(limit as i64)])
    }

    pub fn set_triage(&self, message_id: &str, label: Label, amount: Option<&str>, due: Option<chrono::NaiveDate>, reason: &str) -> Result<()> {
        self.db().execute(
            "UPDATE mail_cache SET triage = ?2, amount = ?3, due = ?4, reason = ?5 WHERE message_id = ?1",
            params![message_id, label.as_str(), amount, due.map(|d| d.to_string()), reason],
        )?;
        Ok(())
    }

    pub fn mail_since(&self, t: DateTime<Utc>) -> Result<Vec<MailRow>> {
        self.mail_query("WHERE received >= ?1 ORDER BY received", &[&ts(t)])
    }

    pub fn mail_rows(&self, ids: &[String]) -> Result<Vec<MailRow>> {
        let mut out = vec![];
        for id in ids {
            out.extend(self.mail_query("WHERE message_id = ?1", &[id])?);
        }
        Ok(out)
    }

    pub fn thread_rows(&self, thread_id: &str) -> Result<Vec<MailRow>> {
        self.mail_query("WHERE thread_id = ?1 ORDER BY received", &[&thread_id])
    }

    /// Unpaid bills due on or before `until` (or with no due date), soonest first.
    pub fn open_bills(&self, until: chrono::NaiveDate) -> Result<Vec<MailRow>> {
        self.mail_query("WHERE triage = 'bill' AND paid = 0 AND (due IS NULL OR due <= ?1) ORDER BY due IS NULL, due, received", &[&until.to_string()])
    }

    pub fn get_cursor(&self, connector: &str) -> Result<Option<String>> {
        Ok(self.db().query_row("SELECT value FROM cursors WHERE connector = ?1", params![connector], |r| r.get(0)).optional()?)
    }

    pub fn set_cursor(&self, connector: &str, value: Option<&str>) -> Result<()> {
        match value {
            Some(v) => self.db().execute("INSERT INTO cursors (connector, value) VALUES (?1, ?2) ON CONFLICT(connector) DO UPDATE SET value = excluded.value", params![connector, v])?,
            None => self.db().execute("DELETE FROM cursors WHERE connector = ?1", params![connector])?,
        };
        Ok(())
    }

    pub fn log_llm(&self, runtime: &str, tier: &str, purpose: &str, tokens_in: Option<i64>, tokens_out: Option<i64>, ok: bool) -> Result<()> {
        self.db().execute(
            "INSERT INTO llm_calls (runtime, tier, purpose, tokens_in, tokens_out, ok) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![runtime, tier, purpose, tokens_in, tokens_out, ok],
        )?;
        Ok(())
    }

    /// Tokens of the newest model call (tests and the Activity tab).
    pub fn llm_tokens_last(&self) -> Result<(Option<i64>, Option<i64>)> {
        Ok(self.db().query_row("SELECT tokens_in, tokens_out FROM llm_calls ORDER BY id DESC LIMIT 1", [], |r| Ok((r.get(0)?, r.get(1)?)))?)
    }

    /// Background model calls since `t`. Chat turns (`purpose = "chat"`) do not count.
    pub fn llm_calls_since(&self, t: DateTime<Utc>) -> Result<usize> {
        let n: i64 = self.db().query_row("SELECT COUNT(*) FROM llm_calls WHERE purpose != 'chat' AND created_at >= ?1", params![ts(t)], |r| r.get(0))?;
        Ok(n as usize)
    }

    pub fn log_tool(&self, tool: &str, risk: &str, ok: bool, ms: i64, preview: Option<&str>) -> Result<()> {
        self.db().execute("INSERT INTO tool_calls (tool, risk, ok, ms, preview) VALUES (?1, ?2, ?3, ?4, ?5)", params![tool, risk, ok, ms, preview])?;
        Ok(())
    }

    /// Tool calls, approvals and model calls, newest first. A declined card is not a failure.
    pub fn activity(&self, limit: usize) -> Result<Vec<ActivityItem>> {
        let sql = "SELECT 'tool', tool, preview, ok, created_at, NULL FROM tool_calls
             UNION ALL SELECT 'approval', tool, status, status != 'failed', COALESCE(decided_at, created_at), NULL FROM approvals
             UNION ALL SELECT 'model', purpose, tier, ok, created_at, NULL FROM llm_calls
             UNION ALL SELECT 'agent', decision, summary, 1, COALESCE(decided_at, created_at), rule FROM agent_asks
             ORDER BY 5 DESC LIMIT ?1";
        Ok(self
            .db()
            .prepare(sql)?
            .query_map(params![limit as i64], |r| Ok(ActivityItem { kind: r.get(0)?, name: r.get(1)?, detail: r.get(2)?, ok: r.get(3)?, at: r.get(4)?, rule: r.get(5)? }))?
            .collect::<rusqlite::Result<_>>()?)
    }

    /// Records a ping and returns its id; `None` if this exact ping was already recorded.
    pub fn record_ping_from(&self, kind: &str, ref_id: &str, sender: Option<&str>) -> Result<Option<i64>> {
        let db = self.db();
        let n = db.execute("INSERT OR IGNORE INTO pings (kind, ref_id, sender) VALUES (?1, ?2, ?3)", params![kind, ref_id, sender])?;
        Ok((n == 1).then(|| db.last_insert_rowid()))
    }

    pub fn pings_since(&self, kind: &str, t: DateTime<Utc>) -> Result<usize> {
        let n: i64 = self.db().query_row("SELECT COUNT(*) FROM pings WHERE kind = ?1 AND sent_at >= ?2", params![kind, ts(t)], |r| r.get(0))?;
        Ok(n as usize)
    }

    pub fn pings_since_kinds(&self, kinds: &[&str], t: DateTime<Utc>) -> Result<usize> {
        let mut n = 0;
        for k in kinds {
            n += self.pings_since(k, t)?;
        }
        Ok(n)
    }

    /// Saves what bleep knows about a session. A new event reopens an ended one; the
    /// "go on" history stays.
    pub fn upsert_agent_session(&self, a: &AgentSession) -> Result<()> {
        self.db().execute(
            "INSERT INTO agent_sessions (id, tool, project, title, transcript_path, state, last_ask, updated) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(id) DO UPDATE SET tool = ?2, project = ?3, title = ?4, transcript_path = ?5, state = ?6, last_ask = ?7, updated = ?8, ended = 0",
            params![a.id, a.tool.as_str(), a.project, a.title, a.transcript_path, a.state.as_str(), a.last_ask, ts(a.updated)],
        )?;
        Ok(())
    }

    pub fn agent_session(&self, id: &str) -> Result<Option<AgentSession>> {
        Ok(self.db().query_row(&format!("SELECT {AGENT_COLS} FROM agent_sessions WHERE id = ?1"), params![id], row_to_agent).optional()?)
    }

    /// Sessions with an event in the last 12 hours that have not ended: waiting first, then newest.
    pub fn agent_sessions(&self, now: DateTime<Utc>) -> Result<Vec<AgentSession>> {
        let sql = format!("SELECT {AGENT_COLS} FROM agent_sessions WHERE ended = 0 AND updated >= ?1 ORDER BY state = 'waiting' DESC, updated DESC");
        Ok(self.db().prepare(&sql)?.query_map(params![ts(now - chrono::Duration::hours(12))], row_to_agent)?.collect::<rusqlite::Result<_>>()?)
    }

    pub fn end_agent_session(&self, id: &str) -> Result<()> {
        self.db().execute("UPDATE agent_sessions SET ended = 1 WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn insert_agent_ask(&self, a: &NewAsk) -> Result<i64> {
        let db = self.db();
        let decided = (a.decision != AskDecision::Queued).then_some(());
        db.execute(
            "INSERT INTO agent_asks (session_id, kind, summary, payload, checks, decision, rule, rpc_id, decided_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, CASE WHEN ?9 THEN strftime('%Y-%m-%dT%H:%M:%fZ','now') END)",
            params![a.session_id, a.kind.as_str(), a.summary, a.payload.to_string(), serde_json::to_string(&a.checks)?, a.decision.as_str(), a.rule, a.rpc_id, decided.is_some()],
        )?;
        Ok(db.last_insert_rowid())
    }

    /// Decides a queued ask. False when it was not queued (already decided or expired).
    pub fn set_ask_decision(&self, id: i64, to: AskDecision, rule: Option<&str>) -> Result<bool> {
        Ok(self.db().execute(
            "UPDATE agent_asks SET decision = ?2, rule = COALESCE(?3, rule), decided_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = ?1 AND decision = 'queued'",
            params![id, to.as_str(), rule],
        )? == 1)
    }

    /// Expires every queued ask of sessions whose id starts with `prefix` ("codex:").
    pub fn expire_queued_with_prefix(&self, prefix: &str) -> Result<usize> {
        Ok(self.db().execute(
            "UPDATE agent_asks SET decision = 'expired', decided_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE decision = 'queued' AND substr(session_id, 1, length(?1)) = ?1",
            params![prefix],
        )?)
    }

    pub fn queued_asks(&self, session_id: &str) -> Result<Vec<AgentAsk>> {
        let sql = format!("SELECT {ASK_COLS} FROM agent_asks WHERE session_id = ?1 AND decision = 'queued' ORDER BY id");
        Ok(self.db().prepare(&sql)?.query_map(params![session_id], row_to_ask)?.collect::<rusqlite::Result<_>>()?)
    }

    pub fn agent_ask(&self, id: i64) -> Result<Option<AgentAsk>> {
        Ok(self.db().query_row(&format!("SELECT {ASK_COLS} FROM agent_asks WHERE id = ?1"), params![id], row_to_ask).optional()?)
    }

    pub fn agent_asks_recent(&self, limit: usize) -> Result<Vec<AgentAsk>> {
        let sql = format!("SELECT {ASK_COLS} FROM agent_asks ORDER BY id DESC LIMIT ?1");
        Ok(self.db().prepare(&sql)?.query_map(params![limit as i64], row_to_ask)?.collect::<rusqlite::Result<_>>()?)
    }

    /// Notes one screenshot "go on" for a session.
    pub fn push_go_on(&self, session_id: &str, t: DateTime<Utc>) -> Result<()> {
        let db = self.db();
        let cur: Option<String> = db.query_row("SELECT go_ons FROM agent_sessions WHERE id = ?1", params![session_id], |r| r.get(0)).optional()?;
        let mut list: Vec<String> = cur.and_then(|c| serde_json::from_str(&c).ok()).unwrap_or_default();
        list.push(ts(t));
        // Only the last hour matters; keep the list short.
        if list.len() > 20 {
            list.drain(..list.len() - 20);
        }
        db.execute("UPDATE agent_sessions SET go_ons = ?2 WHERE id = ?1", params![session_id, serde_json::to_string(&list)?])?;
        Ok(())
    }

    pub fn go_ons_since(&self, session_id: &str, t: DateTime<Utc>) -> Result<usize> {
        let cur: Option<String> = self.db().query_row("SELECT go_ons FROM agent_sessions WHERE id = ?1", params![session_id], |r| r.get(0)).optional()?;
        let list: Vec<String> = cur.and_then(|c| serde_json::from_str(&c).ok()).unwrap_or_default();
        Ok(list.iter().filter(|x| parse_ts(x) >= t).count())
    }

    /// When a ping whose ref starts with `ref_prefix` was last sent.
    pub fn last_ping(&self, kind: &str, ref_prefix: &str) -> Result<Option<DateTime<Utc>>> {
        let at: Option<String> = self.db().query_row(
            "SELECT MAX(sent_at) FROM pings WHERE kind = ?1 AND substr(ref_id, 1, length(?2)) = ?2",
            params![kind, ref_prefix],
            |r| r.get(0),
        )?;
        Ok(at.and_then(|s| DateTime::parse_from_rfc3339(&s).ok()).map(|d| d.with_timezone(&Utc)))
    }

    /// Returns the ping's sender, or `None` if there is no such ping.
    pub fn set_ping_feedback(&self, id: i64, feedback: &str) -> Result<Option<String>> {
        let db = self.db();
        if db.execute("UPDATE pings SET feedback = ?2 WHERE id = ?1", params![id, feedback])? == 0 {
            return Ok(None);
        }
        Ok(db.query_row("SELECT sender FROM pings WHERE id = ?1", params![id], |r| r.get(0))?)
    }

    pub fn not_useful_count(&self, sender: &str) -> Result<usize> {
        let n: i64 = self.db().query_row("SELECT COUNT(*) FROM pings WHERE sender = ?1 AND feedback = 'not_useful'", params![sender], |r| r.get(0))?;
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
pub mod tests {
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

    pub fn utc(s: &str) -> DateTime<Utc> {
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

    pub fn mail(id: &str, thread: &str, mins_ago: i64) -> MailRow {
        MailRow {
            message_id: id.into(),
            thread_id: thread.into(),
            from_addr: "priya@example.com".into(),
            from_name: Some("Priya".into()),
            to_me: true,
            from_me: false,
            subject: format!("Subject {id}"),
            snippet: "snippet".into(),
            labels: vec!["INBOX".into()],
            received: utc("2026-11-12T12:00:00Z") - chrono::Duration::minutes(mins_ago),
            triage: None,
            amount: None,
            due: None,
            reason: None,
            seeded: false,
            paid: false,
        }
    }

    pub fn day(s: &str) -> chrono::NaiveDate {
        chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn mail_round_trips_and_untriaged_are_oldest_first() {
        let (_d, s) = store();
        let a = mail("a", "t1", 10);
        let b = mail("b", "t1", 30);
        s.upsert_mail(&a).unwrap();
        s.upsert_mail(&b).unwrap();
        s.upsert_mail(&a).unwrap();
        assert_eq!(s.untriaged(10).unwrap().iter().map(|m| m.message_id.as_str()).collect::<Vec<_>>(), vec!["b", "a"]);
        s.set_triage("b", Label::Bill, Some("₹2,340"), Some(day("2026-11-13")), "bill words and amount").unwrap();
        let rows = s.thread_rows("t1").unwrap();
        let got = rows.iter().find(|m| m.message_id == "b").unwrap();
        assert_eq!((got.triage, got.amount.as_deref(), got.due), (Some(Label::Bill), Some("₹2,340"), Some(day("2026-11-13"))));
        assert_eq!(s.untriaged(10).unwrap().len(), 1);
        assert_eq!(*rows.iter().find(|m| m.message_id == "a").unwrap(), a);
        // An upsert of a known message keeps its label.
        s.upsert_mail(&b).unwrap();
        assert_eq!(s.thread_rows("t1").unwrap().iter().find(|m| m.message_id == "b").unwrap().triage, Some(Label::Bill));
        assert_eq!(s.mail_since(utc("2026-11-12T11:45:00Z")).unwrap().len(), 1);
    }

    #[test]
    fn open_bills_skip_paid_and_far_due() {
        let (_d, s) = store();
        for (id, due) in [("soon", Some("2026-11-13")), ("far", Some("2026-11-30")), ("nodate", None), ("paid", Some("2026-11-12"))] {
            s.upsert_mail(&mail(id, id, 5)).unwrap();
            s.set_triage(id, Label::Bill, Some("$10"), due.map(day), "r").unwrap();
        }
        s.upsert_mail(&mail("fyi", "fyi", 5)).unwrap();
        s.set_triage("fyi", Label::Fyi, None, None, "r").unwrap();
        let mut p = mail("paid", "paid", 5);
        p.paid = true;
        s.upsert_mail(&p).unwrap();
        s.db().execute("UPDATE mail_cache SET paid = 1 WHERE message_id = 'paid'", []).unwrap();
        let ids: Vec<String> = s.open_bills(day("2026-11-15")).unwrap().into_iter().map(|m| m.message_id).collect();
        assert_eq!(ids, vec!["soon".to_string(), "nodate".to_string()]);
    }

    #[test]
    fn cursor_round_trips() {
        let (_d, s) = store();
        assert_eq!(s.get_cursor("gmail").unwrap(), None);
        s.set_cursor("gmail", Some("100")).unwrap();
        s.set_cursor("gmail", Some("105")).unwrap();
        assert_eq!(s.get_cursor("gmail").unwrap().as_deref(), Some("105"));
        s.set_cursor("gmail", None).unwrap();
        assert_eq!(s.get_cursor("gmail").unwrap(), None);
    }

    #[test]
    fn llm_calls_since_counts_one_shots_only() {
        let (_d, s) = store();
        s.log_llm("claude", "small", "triage", None, None, true).unwrap();
        s.log_llm("claude", "small", "triage", None, None, false).unwrap();
        s.log_llm("claude", "main", "chat", Some(1200), Some(40), true).unwrap();
        assert_eq!(s.llm_calls_since(utc("2020-01-01T00:00:00Z")).unwrap(), 2);
        assert_eq!(s.llm_calls_since(utc("2999-01-01T00:00:00Z")).unwrap(), 0);
    }

    #[test]
    fn activity_merges_kinds_newest_first() {
        let (_d, s) = store();
        let tick = || std::thread::sleep(std::time::Duration::from_millis(5));
        s.log_tool("calendar_list", "read", true, 120, Some("{}")).unwrap();
        tick();
        let a = s.insert_approval("calendar_create", &serde_json::json!({}), None).unwrap();
        s.set_approval_status(a, ApprovalStatus::Declined, None).unwrap();
        tick();
        s.log_llm("claude", "small", "triage", None, None, false).unwrap();
        let items = s.activity(10).unwrap();
        assert_eq!(items.iter().map(|i| i.kind.as_str()).collect::<Vec<_>>(), vec!["model", "approval", "tool"]);
        assert_eq!((items[0].name.as_str(), items[0].ok), ("triage", false));
        assert_eq!((items[1].name.as_str(), items[1].detail.as_deref(), items[1].ok), ("calendar_create", Some("declined"), true));
        assert_eq!((items[2].name.as_str(), items[2].ok), ("calendar_list", true));
        assert_eq!(s.activity(1).unwrap().len(), 1);
    }

    #[test]
    fn pings_with_senders_and_feedback() {
        let (_d, s) = store();
        let id = s.record_ping_from("mail", "t1@1", Some("bills@city.example")).unwrap().unwrap();
        assert_eq!(s.record_ping_from("mail", "t1@1", Some("bills@city.example")).unwrap(), None);
        s.record_ping_from("mail", "t1@2", Some("bills@city.example")).unwrap();
        assert_eq!(s.pings_since("mail", utc("2020-01-01T00:00:00Z")).unwrap(), 2);
        assert_eq!(s.pings_since("meeting_bar", utc("2020-01-01T00:00:00Z")).unwrap(), 0);
        assert!(s.last_ping("mail", "t1@").unwrap().is_some());
        assert!(s.last_ping("mail", "t9@").unwrap().is_none());
        assert_eq!(s.set_ping_feedback(id, "not_useful").unwrap().as_deref(), Some("bills@city.example"));
        assert_eq!(s.not_useful_count("bills@city.example").unwrap(), 1);
        assert_eq!(s.set_ping_feedback(999, "not_useful").unwrap(), None);
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

    pub fn agent(id: &str, state: AgentState, mins_ago: i64) -> AgentSession {
        AgentSession {
            id: id.into(),
            tool: AgentTool::Claude,
            project: "/Users/me/code/shop".into(),
            title: None,
            transcript_path: None,
            state,
            last_ask: None,
            updated: utc("2026-11-13T10:00:00Z") - chrono::Duration::minutes(mins_ago),
        }
    }

    fn ask(session: &str, decision: AskDecision) -> NewAsk {
        NewAsk { session_id: session.into(), kind: AskKind::Permission, summary: "wants to run npm test".into(), payload: serde_json::json!({"command":"npm test"}), checks: vec![], decision, rule: None, rpc_id: None }
    }

    #[test]
    fn agent_session_round_trips_and_stale_ones_hide() {
        let (_d, s) = store();
        let now = utc("2026-11-13T10:00:00Z");
        let mut a = agent("claude:s1", AgentState::Running, 5);
        a.title = Some("Fix cart".into());
        a.last_ask = Some("wants to run npm test".into());
        s.upsert_agent_session(&a).unwrap();
        assert_eq!(s.agent_session("claude:s1").unwrap().unwrap(), a);
        s.upsert_agent_session(&agent("claude:old", AgentState::Waiting, 13 * 60)).unwrap();
        let ids: Vec<_> = s.agent_sessions(now).unwrap().into_iter().map(|x| x.id).collect();
        assert_eq!(ids, vec!["claude:s1"]);
        s.end_agent_session("claude:s1").unwrap();
        assert!(s.agent_sessions(now).unwrap().is_empty());
        assert!(s.agent_session("claude:s1").unwrap().is_some());
    }

    #[test]
    fn a_new_event_reopens_an_ended_session() {
        let (_d, s) = store();
        s.upsert_agent_session(&agent("claude:s1", AgentState::Done, 5)).unwrap();
        s.end_agent_session("claude:s1").unwrap();
        s.upsert_agent_session(&agent("claude:s1", AgentState::Running, 1)).unwrap();
        assert_eq!(s.agent_sessions(utc("2026-11-13T10:00:00Z")).unwrap().len(), 1);
    }

    #[test]
    fn waiting_sessions_sort_first() {
        let (_d, s) = store();
        s.upsert_agent_session(&agent("a", AgentState::Running, 1)).unwrap();
        s.upsert_agent_session(&agent("b", AgentState::Waiting, 30)).unwrap();
        s.upsert_agent_session(&agent("c", AgentState::Done, 2)).unwrap();
        let ids: Vec<_> = s.agent_sessions(utc("2026-11-13T10:00:00Z")).unwrap().into_iter().map(|x| x.id).collect();
        assert_eq!(ids, vec!["b", "a", "c"]);
    }

    #[test]
    fn ask_decision_moves_only_from_queued() {
        let (_d, s) = store();
        let q = s.insert_agent_ask(&ask("a", AskDecision::Queued)).unwrap();
        let auto = s.insert_agent_ask(&NewAsk { rule: Some("npm test".into()), ..ask("a", AskDecision::Auto) }).unwrap();
        assert_eq!(s.queued_asks("a").unwrap().iter().map(|x| x.id).collect::<Vec<_>>(), vec![q]);
        assert!(s.set_ask_decision(q, AskDecision::Approved, None).unwrap());
        assert!(!s.set_ask_decision(q, AskDecision::Denied, None).unwrap());
        assert!(!s.set_ask_decision(auto, AskDecision::Expired, None).unwrap());
        let got = s.agent_ask(q).unwrap().unwrap();
        assert_eq!(got.decision, AskDecision::Approved);
        assert!(got.decided_at.is_some());
        assert_eq!(got.payload["command"], "npm test");
        assert_eq!(s.agent_ask(auto).unwrap().unwrap().rule.as_deref(), Some("npm test"));
        assert_eq!(s.agent_asks_recent(10).unwrap().first().map(|x| x.id), Some(auto));
        assert!(s.queued_asks("a").unwrap().is_empty());
    }

    #[test]
    fn expire_queued_with_prefix_touches_only_those_sessions() {
        let (_d, s) = store();
        let codex = s.insert_agent_ask(&ask("codex:t1", AskDecision::Queued)).unwrap();
        let claude = s.insert_agent_ask(&ask("claude:a", AskDecision::Queued)).unwrap();
        assert_eq!(s.expire_queued_with_prefix("codex:").unwrap(), 1);
        assert_eq!(s.agent_ask(codex).unwrap().unwrap().decision, AskDecision::Expired);
        assert_eq!(s.agent_ask(claude).unwrap().unwrap().decision, AskDecision::Queued);
    }

    #[test]
    fn go_ons_count_in_window() {
        let (_d, s) = store();
        s.upsert_agent_session(&agent("a", AgentState::Done, 1)).unwrap();
        let t = utc("2026-11-13T10:00:00Z");
        s.push_go_on("a", t - chrono::Duration::minutes(90)).unwrap();
        s.push_go_on("a", t - chrono::Duration::minutes(20)).unwrap();
        s.push_go_on("a", t - chrono::Duration::minutes(5)).unwrap();
        assert_eq!(s.go_ons_since("a", t - chrono::Duration::hours(1)).unwrap(), 2);
        assert_eq!(s.go_ons_since("nobody", t).unwrap(), 0);
        // Upserting the session keeps its go-ons.
        s.upsert_agent_session(&agent("a", AgentState::Running, 0)).unwrap();
        assert_eq!(s.go_ons_since("a", t - chrono::Duration::hours(1)).unwrap(), 2);
    }

    #[test]
    fn pings_since_kinds_counts_mail_and_agent() {
        let (_d, s) = store();
        s.record_ping("mail", "t1@1").unwrap();
        s.record_ping("agent", "ask:1").unwrap();
        s.record_ping("checkin", "2026-11-13@13:00").unwrap();
        let t = Utc::now() - chrono::Duration::minutes(1);
        assert_eq!(s.pings_since_kinds(&["mail", "agent"], t).unwrap(), 2);
    }

    #[test]
    fn activity_lists_agent_asks() {
        let (_d, s) = store();
        let id = s.insert_agent_ask(&NewAsk { rule: Some("npm test".into()), ..ask("a", AskDecision::Auto) }).unwrap();
        let a = s.activity(10).unwrap();
        assert_eq!(a.len(), 1);
        assert_eq!((a[0].kind.as_str(), a[0].name.as_str(), a[0].detail.as_deref(), a[0].rule.as_deref(), a[0].ok), ("agent", "auto", Some("wants to run npm test"), Some("npm test"), true));
        let _ = id;
    }
}
