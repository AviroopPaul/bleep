//! Threads waiting on the user. Pure.
use crate::store::{Label, MailRow};
use chrono::{DateTime, Utc};

/// Thread ids whose last message is from someone else, sent To the user, over 24 h old,
/// and labelled important or needs_reply. Oldest first.
pub fn waiting_on_me(rows: &[MailRow], now: DateTime<Utc>) -> Vec<String> {
    let mut last: std::collections::HashMap<&str, &MailRow> = std::collections::HashMap::new();
    for r in rows {
        let e = last.entry(r.thread_id.as_str()).or_insert(r);
        if r.received > e.received {
            *e = r;
        }
    }
    let mut waiting: Vec<&MailRow> = last
        .into_values()
        .filter(|m| !m.from_me && m.to_me && now - m.received > chrono::Duration::hours(24) && matches!(m.triage, Some(Label::Important | Label::NeedsReply)))
        .collect();
    waiting.sort_by_key(|m| m.received);
    waiting.into_iter().map(|m| m.thread_id.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::tests::{mail, utc};

    fn now() -> DateTime<Utc> {
        utc("2026-11-12T12:00:00Z")
    }

    fn msg(id: &str, thread: &str, hours_ago: i64, label: Option<Label>) -> MailRow {
        let mut m = mail(id, thread, 0);
        m.received = now() - chrono::Duration::hours(hours_ago);
        m.triage = label;
        m
    }

    #[test]
    fn waiting_on_me_rules() {
        let old = msg("a", "t_old", 30, Some(Label::NeedsReply));
        let young = msg("b", "t_young", 20, Some(Label::NeedsReply));
        let fyi = msg("c", "t_fyi", 40, Some(Label::Fyi));
        let mut cc = msg("d", "t_cc", 40, Some(Label::Important));
        cc.to_me = false;
        let asked = msg("e", "t_replied", 50, Some(Label::Important));
        let mut reply = msg("f", "t_replied", 45, None);
        reply.from_me = true;
        let older = msg("g", "t_oldest", 72, Some(Label::Important));
        let rows = vec![old, young, fyi, cc, asked, reply, older];
        assert_eq!(waiting_on_me(&rows, now()), vec!["t_oldest".to_string(), "t_old".to_string()]);
        // Exactly 24 h is not over 24 h.
        assert!(waiting_on_me(&[msg("x", "t_edge", 24, Some(Label::NeedsReply))], now()).is_empty());
    }
}
