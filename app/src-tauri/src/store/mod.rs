//! SQLite store: one chat history and the runtime sessions behind it.
use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;
use std::sync::Mutex;

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
    })
}

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
        Ok(rows)
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
