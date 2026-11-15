//! Phase 9 storage: the message history of API-run chats (bleep keeps it, as no CLI does),
//! and what each API call cost.
use super::*;

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
pub struct Spend {
    /// US dollars for calls with a known price.
    pub cost: f64,
    /// Calls with no price, so the total is low.
    pub unpriced: i64,
    pub calls: i64,
}

pub struct ApiUsage<'a> {
    pub day: &'a str,
    pub provider: &'a str,
    pub model: &'a str,
    pub purpose: &'a str,
    pub tokens_in: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub tokens_out: u64,
    pub cost: Option<f64>,
}

impl Store {
    pub fn api_conv_exists(&self, conv: &str) -> Result<bool> {
        Ok(self.db().query_row("SELECT EXISTS(SELECT 1 FROM api_messages WHERE conv = ?1)", params![conv], |r| r.get(0))?)
    }

    /// The messages of a conversation, as JSON, oldest first.
    pub fn api_messages(&self, conv: &str) -> Result<Vec<String>> {
        let db = self.db();
        let mut st = db.prepare("SELECT msg FROM api_messages WHERE conv = ?1 ORDER BY seq")?;
        let rows = st.query_map(params![conv], |r| r.get::<_, String>(0))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn api_append(&self, conv: &str, msg: &str) -> Result<()> {
        self.db().execute("INSERT INTO api_messages (conv, seq, msg) SELECT ?1, COALESCE(MAX(seq), 0) + 1, ?2 FROM api_messages WHERE conv = ?1", params![conv, msg])?;
        Ok(())
    }

    pub fn log_api_usage(&self, u: &ApiUsage) -> Result<()> {
        self.db().execute(
            "INSERT INTO api_usage (day, provider, model, purpose, tokens_in, cache_read, cache_write, tokens_out, cost) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![u.day, u.provider, u.model, u.purpose, u.tokens_in as i64, u.cache_read as i64, u.cache_write as i64, u.tokens_out as i64, u.cost],
        )?;
        Ok(())
    }

    pub fn spend_on(&self, day: &str) -> Result<Spend> {
        Ok(self.db().query_row(
            "SELECT COALESCE(SUM(cost), 0), COALESCE(SUM(cost IS NULL), 0), COUNT(*) FROM api_usage WHERE day = ?1",
            params![day],
            |r| Ok(Spend { cost: r.get(0)?, unpriced: r.get(1)?, calls: r.get(2)? }),
        )?)
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

    #[test]
    fn conversations_append_in_order_and_stay_apart() {
        let (_d, s) = store();
        assert!(!s.api_conv_exists("a").unwrap());
        s.api_append("a", "1").unwrap();
        s.api_append("b", "x").unwrap();
        s.api_append("a", "2").unwrap();
        assert!(s.api_conv_exists("a").unwrap());
        assert_eq!(s.api_messages("a").unwrap(), ["1", "2"]);
        assert_eq!(s.api_messages("b").unwrap(), ["x"]);
    }

    #[test]
    fn spend_sums_one_day_and_counts_unpriced_calls() {
        let (_d, s) = store();
        let u = |day, cost| ApiUsage { day, provider: "openrouter", model: "m", purpose: "chat", tokens_in: 10, cache_read: 0, cache_write: 0, tokens_out: 5, cost };
        s.log_api_usage(&u("2026-11-15", Some(0.25))).unwrap();
        s.log_api_usage(&u("2026-11-15", Some(0.5))).unwrap();
        s.log_api_usage(&u("2026-11-15", None)).unwrap();
        s.log_api_usage(&u("2026-11-14", Some(9.0))).unwrap();
        assert_eq!(s.spend_on("2026-11-15").unwrap(), Spend { cost: 0.75, unpriced: 1, calls: 3 });
        assert_eq!(s.spend_on("2026-11-13").unwrap(), Spend::default());
    }
}
