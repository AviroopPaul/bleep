//! Phase 7 storage: the last tool list of each remote MCP server, and the tool-set hash
//! a chat session started with.
use super::*;

impl Store {
    /// The last `tools/list` of a server, as the raw MCP tool objects, in order.
    pub fn mcp_tools(&self, server: &str) -> Result<Vec<Value>> {
        let db = self.db();
        let mut st = db.prepare("SELECT spec FROM mcp_tools WHERE server = ?1 ORDER BY pos")?;
        let rows = st.query_map(params![server], |r| r.get::<_, String>(0))?;
        Ok(rows.filter_map(|r| r.ok()).filter_map(|s| serde_json::from_str(&s).ok()).collect())
    }

    /// Replaces a server's cached tool list in one step.
    pub fn replace_mcp_tools(&self, server: &str, tools: &[Value]) -> Result<()> {
        let mut db = self.db();
        let tx = db.transaction()?;
        tx.execute("DELETE FROM mcp_tools WHERE server = ?1", params![server])?;
        for (i, t) in tools.iter().enumerate() {
            tx.execute("INSERT INTO mcp_tools (server, pos, spec) VALUES (?1, ?2, ?3)", params![server, i as i64, t.to_string()])?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn delete_mcp_tools(&self, server: &str) -> Result<()> {
        self.db().execute("DELETE FROM mcp_tools WHERE server = ?1", params![server])?;
        Ok(())
    }

    pub fn set_session_tools_hash(&self, id: i64, hash: &str) -> Result<()> {
        self.db().execute("UPDATE sessions SET tools_hash = ?2 WHERE id = ?1", params![id, hash])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn store() -> (tempfile::TempDir, Store) {
        let d = tempfile::tempdir().unwrap();
        let s = Store::open(&d.path().join("b.db")).unwrap();
        (d, s)
    }

    #[test]
    fn migration_6_keeps_v5_data() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("b.db");
        {
            let c = Connection::open(&p).unwrap();
            for sql in &MIGRATIONS[..5] {
                c.execute_batch(sql).unwrap();
            }
            c.pragma_update(None, "user_version", 5).unwrap();
            c.execute("INSERT INTO messages (role, kind, content, status) VALUES ('user','chat','hello','done')", []).unwrap();
            c.execute("INSERT INTO sessions (runtime, seed) VALUES ('claude', 'old seed')", []).unwrap();
        }
        let s = Store::open(&p).unwrap();
        assert_eq!(s.page_messages(None, 5).unwrap().len(), 1);
        let sess = s.current_session("claude").unwrap().unwrap();
        assert_eq!((sess.seed.as_deref(), sess.tools_hash.as_deref()), (Some("old seed"), None));
    }

    #[test]
    fn tools_replace_and_list_in_order() {
        let (_d, s) = store();
        assert!(s.mcp_tools("dayblocks").unwrap().is_empty());
        s.replace_mcp_tools("dayblocks", &[json!({"name": "list_tasks"}), json!({"name": "move_task"})]).unwrap();
        s.replace_mcp_tools("wiki", &[json!({"name": "ask"})]).unwrap();
        let names = |v: Vec<Value>| v.iter().map(|t| t["name"].as_str().unwrap().to_string()).collect::<Vec<_>>();
        assert_eq!(names(s.mcp_tools("dayblocks").unwrap()), ["list_tasks", "move_task"]);
        s.replace_mcp_tools("dayblocks", &[json!({"name": "add_task"})]).unwrap();
        assert_eq!(names(s.mcp_tools("dayblocks").unwrap()), ["add_task"], "replace drops the old list");
        s.delete_mcp_tools("dayblocks").unwrap();
        assert!(s.mcp_tools("dayblocks").unwrap().is_empty());
        assert_eq!(names(s.mcp_tools("wiki").unwrap()), ["ask"], "other servers stay");
    }

    #[test]
    fn session_tools_hash_round_trip() {
        let (_d, s) = store();
        let id = s.create_session("claude").unwrap().id;
        s.set_session_tools_hash(id, "abc").unwrap();
        assert_eq!(s.current_session("claude").unwrap().unwrap().tools_hash.as_deref(), Some("abc"));
    }
}
