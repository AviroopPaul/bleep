//! Raw rows for the Usage tab (0.5.0, F4). `crate::usage` turns them into charts.
use super::*;

/// One model call bleep made (`llm_calls`).
#[derive(Clone, Debug, PartialEq)]
pub struct LlmRow {
    pub runtime: String,
    pub tier: String,
    pub purpose: String,
    pub tokens_in: Option<i64>,
    pub tokens_out: Option<i64>,
    pub ok: bool,
    pub at: DateTime<Utc>,
}

/// One API call with a known model and price (`api_usage`, bring your own key).
#[derive(Clone, Debug, PartialEq)]
pub struct ApiRow {
    pub model: String,
    pub purpose: String,
    /// Input including cache reads and writes.
    pub tokens_in: i64,
    pub tokens_out: i64,
    pub cost: Option<f64>,
    pub at: DateTime<Utc>,
}

/// Tool name, calls, failed calls.
pub type ToolStat = (String, i64, i64);

impl Store {
    pub fn llm_rows_since(&self, t: DateTime<Utc>) -> Result<Vec<LlmRow>> {
        let db = self.db();
        let mut st = db.prepare("SELECT runtime, tier, purpose, tokens_in, tokens_out, ok, created_at FROM llm_calls WHERE created_at >= ?1 ORDER BY id")?;
        let rows = st.query_map(params![ts(t)], |r| {
            Ok(LlmRow { runtime: r.get(0)?, tier: r.get(1)?, purpose: r.get(2)?, tokens_in: r.get(3)?, tokens_out: r.get(4)?, ok: r.get(5)?, at: parse_ts(&r.get::<_, String>(6)?) })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn api_rows_since(&self, t: DateTime<Utc>) -> Result<Vec<ApiRow>> {
        let db = self.db();
        let mut st = db.prepare("SELECT model, purpose, tokens_in + cache_read + cache_write, tokens_out, cost, created_at FROM api_usage WHERE created_at >= ?1 ORDER BY id")?;
        let rows = st.query_map(params![ts(t)], |r| {
            Ok(ApiRow { model: r.get(0)?, purpose: r.get(1)?, tokens_in: r.get(2)?, tokens_out: r.get(3)?, cost: r.get(4)?, at: parse_ts(&r.get::<_, String>(5)?) })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Most used tools first.
    pub fn tool_stats_since(&self, t: DateTime<Utc>, limit: usize) -> Result<Vec<ToolStat>> {
        let db = self.db();
        let mut st = db.prepare("SELECT tool, COUNT(*), SUM(ok = 0) FROM tool_calls WHERE created_at >= ?1 GROUP BY tool ORDER BY COUNT(*) DESC, tool LIMIT ?2")?;
        let rows = st.query_map(params![ts(t), limit as i64], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Agent asks by decision ("auto", "approved", ...).
    pub fn ask_stats_since(&self, t: DateTime<Utc>) -> Result<Vec<(String, i64)>> {
        let db = self.db();
        let mut st = db.prepare("SELECT decision, COUNT(*) FROM agent_asks WHERE created_at >= ?1 GROUP BY decision")?;
        let rows = st.query_map(params![ts(t)], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}
