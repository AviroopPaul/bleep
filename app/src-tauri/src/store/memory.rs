//! Phase 5 storage: compaction seeds on sessions, memory suggestions, and the FTS5
//! indexes behind memory_search (chat messages and daily-note lines).
use super::*;

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Proposal {
    pub id: i64,
    pub fact: String,
    /// "compaction" or "nightly".
    pub source: String,
    pub status: String,
    pub created_at: String,
}

/// One search hit from the message index.
#[derive(Clone, Debug, PartialEq)]
pub struct MsgHit {
    pub id: i64,
    pub role: Role,
    pub created_at: String,
    pub content: String,
}

/// One search hit from the daily-note index.
#[derive(Clone, Debug, PartialEq)]
pub struct NoteHit {
    pub date: String,
    pub time: String,
    pub text: String,
}

/// Lower case, single spaces, no trailing dot: two facts that differ only in these are one fact.
pub fn norm_fact(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").trim_end_matches('.').to_lowercase()
}

const PROPOSAL_COLS: &str = "id, fact, source, status, created_at";

fn row_to_proposal(r: &Row) -> rusqlite::Result<Proposal> {
    Ok(Proposal { id: r.get(0)?, fact: r.get(1)?, source: r.get(2)?, status: r.get(3)?, created_at: r.get(4)? })
}

impl Store {
    pub fn set_session_seed(&self, id: i64, seed: &str) -> Result<()> {
        self.db().execute("UPDATE sessions SET seed = ?2 WHERE id = ?1", params![id, seed])?;
        Ok(())
    }

    pub fn set_session_memory_known(&self, id: i64, text: &str) -> Result<()> {
        self.db().execute("UPDATE sessions SET memory_known = ?2 WHERE id = ?1", params![id, text])?;
        Ok(())
    }

    pub fn set_session_context(&self, id: i64, used: i64) -> Result<()> {
        self.db().execute("UPDATE sessions SET context_used = ?2 WHERE id = ?1", params![id, used])?;
        Ok(())
    }

    /// Ends a session that was compacted, keeping its summary and size.
    pub fn end_session_with(&self, id: i64, summary: &str, tokens: i64) -> Result<()> {
        self.db().execute(
            "UPDATE sessions SET ended_at = strftime('%Y-%m-%dT%H:%M:%fZ','now'), summary = ?2, tokens_at_end = ?3 WHERE id = ?1",
            params![id, summary, tokens],
        )?;
        Ok(())
    }

    /// Every message of one session, oldest first.
    pub fn session_messages(&self, session_id: i64) -> Result<Vec<Message>> {
        let db = self.db();
        let sql = format!("SELECT {MSG_COLS} FROM messages WHERE session_id = ?1 ORDER BY id");
        let rows = db.prepare(&sql)?.query_map(params![session_id], row_to_message)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Adds a suggestion unless the same fact was suggested before (any status). `None` = skipped.
    pub fn add_proposal(&self, fact: &str, source: &str) -> Result<Option<i64>> {
        let n = norm_fact(fact);
        if n.is_empty() {
            return Ok(None);
        }
        let db = self.db();
        let seen: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM memory_proposals WHERE norm = ?1)", params![n], |r| r.get(0))?;
        if seen {
            return Ok(None);
        }
        db.execute("INSERT INTO memory_proposals (fact, norm, source) VALUES (?1, ?2, ?3)", params![fact.trim(), n, source])?;
        Ok(Some(db.last_insert_rowid()))
    }

    pub fn pending_proposals(&self) -> Result<Vec<Proposal>> {
        let db = self.db();
        let sql = format!("SELECT {PROPOSAL_COLS} FROM memory_proposals WHERE status = 'pending' ORDER BY id");
        let rows = db.prepare(&sql)?.query_map([], row_to_proposal)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Moves a pending suggestion to "added" or "dismissed". Returns it, or `None` if it was not pending.
    pub fn decide_proposal(&self, id: i64, added: bool) -> Result<Option<Proposal>> {
        let db = self.db();
        let status = if added { "added" } else { "dismissed" };
        let n = db.execute(
            "UPDATE memory_proposals SET status = ?2, decided_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = ?1 AND status = 'pending'",
            params![id, status],
        )?;
        if n == 0 {
            return Ok(None);
        }
        let sql = format!("SELECT {PROPOSAL_COLS} FROM memory_proposals WHERE id = ?1");
        Ok(Some(db.query_row(&sql, params![id], row_to_proposal)?))
    }

    pub fn index_note(&self, date: &str, time: &str, text: &str) -> Result<()> {
        self.db().execute("INSERT INTO note_fts (text, date, time) VALUES (?1, ?2, ?3)", params![text, date, time])?;
        Ok(())
    }

    pub fn clear_note_index(&self) -> Result<()> {
        self.db().execute("DELETE FROM note_fts", [])?;
        Ok(())
    }

    /// `fts` must already be a safe FTS5 query (see `memory::search::fts_query`).
    pub fn search_messages(&self, fts: &str, limit: u32) -> Result<Vec<MsgHit>> {
        let db = self.db();
        let mut st = db.prepare(
            "SELECT m.id, m.role, m.created_at, m.content FROM msg_fts JOIN messages m ON m.id = msg_fts.rowid
             WHERE msg_fts MATCH ?1 ORDER BY rank, m.id DESC LIMIT ?2",
        )?;
        let rows = st
            .query_map(params![fts, limit], |r| Ok(MsgHit { id: r.get(0)?, role: Role::parse(&r.get::<_, String>(1)?), created_at: r.get(2)?, content: r.get(3)? }))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn search_notes(&self, fts: &str, limit: u32) -> Result<Vec<NoteHit>> {
        let db = self.db();
        let mut st = db.prepare("SELECT date, time, text FROM note_fts WHERE note_fts MATCH ?1 ORDER BY rank, date DESC, time DESC LIMIT ?2")?;
        let rows = st
            .query_map(params![fts, limit], |r| Ok(NoteHit { date: r.get(0)?, time: r.get(1)?, text: r.get(2)? }))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, Store) {
        let d = tempfile::tempdir().unwrap();
        let s = Store::open(&d.path().join("b.db")).unwrap();
        (d, s)
    }

    fn msg(s: &Store, role: Role, content: &str, status: MsgStatus) -> i64 {
        s.insert_message(NewMessage { role, kind: Kind::Chat, content: content.into(), status, session_id: None }).unwrap()
    }

    #[test]
    fn migration_5_keeps_v4_data() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("b.db");
        {
            let c = Connection::open(&p).unwrap();
            for sql in &MIGRATIONS[..4] {
                c.execute_batch(sql).unwrap();
            }
            c.pragma_update(None, "user_version", 4).unwrap();
            c.execute("INSERT INTO messages (role, kind, content, status) VALUES ('user','chat','sync means my 1:1 with Priya','done')", []).unwrap();
        }
        let s = Store::open(&p).unwrap();
        assert_eq!(s.page_messages(None, 5).unwrap().len(), 1);
        let hits = s.search_messages("\"priya\"", 5).unwrap();
        assert_eq!(hits.len(), 1, "old done messages are backfilled into the index");
    }

    #[test]
    fn message_enters_index_when_done_and_leaves_when_deleted() {
        let (_d, s) = store();
        let id = msg(&s, Role::Assistant, "", MsgStatus::Streaming);
        assert!(s.search_messages("\"mango\"", 5).unwrap().is_empty());
        s.finish_message(id, "Your favourite fruit is mango.", &Value::Array(vec![]), None, MsgStatus::Done).unwrap();
        let hits = s.search_messages("\"mango\"", 5).unwrap();
        assert_eq!((hits.len(), hits[0].id, hits[0].role), (1, id, Role::Assistant));
        s.finish_message(id, "Your favourite fruit is kiwi.", &Value::Array(vec![]), None, MsgStatus::Done).unwrap();
        assert!(s.search_messages("\"mango\"", 5).unwrap().is_empty(), "old text leaves the index");
        assert_eq!(s.search_messages("\"kiwi\"", 5).unwrap().len(), 1);
        s.db().execute("DELETE FROM messages WHERE id = ?1", params![id]).unwrap();
        assert!(s.search_messages("\"kiwi\"", 5).unwrap().is_empty());
    }

    #[test]
    fn system_and_error_rows_are_not_indexed() {
        let (_d, s) = store();
        msg(&s, Role::System, "mango note", MsgStatus::Done);
        msg(&s, Role::Assistant, "mango half", MsgStatus::Error);
        assert!(s.search_messages("\"mango\"", 5).unwrap().is_empty());
    }

    #[test]
    fn note_index_add_and_clear() {
        let (_d, s) = store();
        s.index_note("2026-11-15", "10:02", "Sync means my 1:1 with Priya").unwrap();
        let h = s.search_notes("\"sync\"", 5).unwrap();
        assert_eq!(h, vec![NoteHit { date: "2026-11-15".into(), time: "10:02".into(), text: "Sync means my 1:1 with Priya".into() }]);
        s.clear_note_index().unwrap();
        assert!(s.search_notes("\"sync\"", 5).unwrap().is_empty());
    }

    #[test]
    fn proposals_dedupe_and_decide_once() {
        let (_d, s) = store();
        let id = s.add_proposal("You pay bills the evening they are due.", "nightly").unwrap().unwrap();
        assert_eq!(s.add_proposal("  you pay bills the  evening they are due ", "compaction").unwrap(), None);
        assert_eq!(s.add_proposal("   ", "nightly").unwrap(), None);
        assert_eq!(s.pending_proposals().unwrap().len(), 1);
        let p = s.decide_proposal(id, false).unwrap().unwrap();
        assert_eq!((p.status.as_str(), p.source.as_str()), ("dismissed", "nightly"));
        assert_eq!(s.decide_proposal(id, true).unwrap(), None, "only from pending");
        assert_eq!(s.add_proposal("You pay bills the evening they are due", "nightly").unwrap(), None, "a dismissed fact is not suggested again");
        assert!(s.pending_proposals().unwrap().is_empty());
    }

    #[test]
    fn session_seed_memory_and_end_with_summary() {
        let (_d, s) = store();
        let a = s.create_session("claude").unwrap();
        assert_eq!((a.seed.as_deref(), a.memory_known.as_deref()), (None, None));
        s.set_session_seed(a.id, "summary here").unwrap();
        s.set_session_memory_known(a.id, "- fact").unwrap();
        let cur = s.current_session("claude").unwrap().unwrap();
        assert_eq!((cur.seed.as_deref(), cur.memory_known.as_deref()), (Some("summary here"), Some("- fact")));
        s.end_session_with(a.id, "the summary", 250_000).unwrap();
        assert!(s.current_session("claude").unwrap().is_none());
        let (sum, tok): (String, i64) = s.db().query_row("SELECT summary, tokens_at_end FROM sessions WHERE id = ?1", params![a.id], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!((sum.as_str(), tok), ("the summary", 250_000));
    }

    #[test]
    fn session_messages_in_order() {
        let (_d, s) = store();
        let a = s.create_session("claude").unwrap();
        for c in ["one", "two"] {
            s.insert_message(NewMessage { role: Role::User, kind: Kind::Chat, content: c.into(), status: MsgStatus::Done, session_id: Some(a.id) }).unwrap();
        }
        msg(&s, Role::User, "other session", MsgStatus::Done);
        let m: Vec<String> = s.session_messages(a.id).unwrap().into_iter().map(|m| m.content).collect();
        assert_eq!(m, ["one", "two"]);
    }

    #[test]
    fn norm_fact_folds_case_space_and_dot() {
        assert_eq!(norm_fact("  Sync  means 1:1.  "), "sync means 1:1");
    }
}
