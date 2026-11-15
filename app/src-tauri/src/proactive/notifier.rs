//! Calm mail pings (build plan 8.5): quiet hours, a budget per hour, one ping per thread
//! per 6 hours, Pause. Seeded mail never pings. Everything not pinged waits for a check-in.
use super::scheduler::{BarUi, Clock};
use crate::config::MailConfig;
use crate::store::{Label, MailRow, Store};
use chrono::{DateTime, Duration, NaiveDateTime, NaiveTime, Utc};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub const KIND: &str = "mail";
/// Agent pings share the hourly budget with mail pings.
pub const AGENT_KIND: &str = "agent";
/// The nightly "memory suggestions" bubble.
pub const MEMORY_KIND: &str = "memory";
/// Every ping kind counts against the same hourly budget.
pub const BUDGET_KINDS: &[&str] = &[KIND, AGENT_KIND, MEMORY_KIND];
/// "Not useful" taps on one sender before bleep suggests muting them.
pub const MUTE_AFTER: usize = 3;

#[derive(Clone, Debug)]
pub struct Calm {
    pub quiet_start: NaiveTime,
    pub quiet_end: NaiveTime,
    pub per_hour: usize,
    pub dedup: Duration,
}

impl Default for Calm {
    fn default() -> Self {
        Calm { quiet_start: NaiveTime::from_hms_opt(23, 0, 0).unwrap(), quiet_end: NaiveTime::from_hms_opt(8, 0, 0).unwrap(), per_hour: 4, dedup: Duration::hours(6) }
    }
}

#[derive(Debug, PartialEq)]
pub enum Verdict {
    Send,
    /// Quiet hours: the morning brief picks it up.
    Hold,
    /// Over the hourly budget: the next check-in picks it up.
    Merge,
    /// Paused, or this thread pinged recently.
    Skip,
}

pub fn may_ping(calm: &Calm, now_local: NaiveDateTime, sent_last_hour: usize, last_for_ref: Option<DateTime<Utc>>, now: DateTime<Utc>, paused: bool) -> Verdict {
    if paused || last_for_ref.is_some_and(|t| now - t < calm.dedup) {
        return Verdict::Skip;
    }
    let t = now_local.time();
    let quiet = if calm.quiet_start > calm.quiet_end { t >= calm.quiet_start || t < calm.quiet_end } else { t >= calm.quiet_start && t < calm.quiet_end };
    if quiet {
        return Verdict::Hold;
    }
    if sent_last_hour >= calm.per_hour {
        return Verdict::Merge;
    }
    Verdict::Send
}

fn cut(s: &str, n: usize) -> String {
    if s.chars().count() > n { s.chars().take(n).collect::<String>() + "…" } else { s.to_string() }
}

pub struct Notifier {
    store: Arc<Store>,
    ui: Arc<dyn BarUi>,
    paused: Arc<AtomicBool>,
    calm: Calm,
    cfg: Arc<dyn Fn() -> MailConfig + Send + Sync>,
    now: Clock,
}

impl Notifier {
    pub fn new(store: Arc<Store>, ui: Arc<dyn BarUi>, paused: Arc<AtomicBool>, cfg: Arc<dyn Fn() -> MailConfig + Send + Sync>, now: Clock) -> Arc<Notifier> {
        Arc::new(Notifier { store, ui, paused, calm: Calm::default(), cfg, now })
    }

    /// Pings for newly labelled mail that matters now: bills due today or tomorrow, and
    /// important or needs-reply mail from a VIP. Returns the texts sent.
    pub fn after_triage(&self, rows: &[MailRow]) -> Vec<String> {
        let now = (self.now)();
        let local = now.with_timezone(&chrono::Local);
        let today = local.date_naive();
        let vips = (self.cfg)().vips;
        let mut sent = vec![];
        for m in rows.iter().filter(|m| !m.seeded && !m.from_me) {
            // The display name is chosen by the sender: keep it short.
            let who = cut(m.from_name.as_deref().unwrap_or(&m.from_addr), 40);
            let text = match m.triage {
                Some(Label::Bill) => match m.due {
                    Some(d) if d == today => format!("Bill due today: {who}"),
                    Some(d) if Some(d) == today.succ_opt() => format!("Bill due tomorrow: {who}"),
                    _ => continue,
                },
                Some(Label::Important | Label::NeedsReply) if vips.iter().any(|v| v.eq_ignore_ascii_case(&m.from_addr)) => {
                    format!("{who} needs a reply: {}", cut(&m.subject, 60))
                }
                _ => continue,
            };
            let text = match (&m.triage, &m.amount) {
                (Some(Label::Bill), Some(a)) => format!("{text} {a}"),
                _ => text,
            };
            let last = self.store.last_ping(KIND, &format!("{}@", m.thread_id)).ok().flatten();
            let in_hour = self.store.pings_since_kinds(BUDGET_KINDS, now - Duration::hours(1)).unwrap_or(usize::MAX);
            if may_ping(&self.calm, local.naive_local(), in_hour, last, now, self.paused.load(Ordering::SeqCst)) != Verdict::Send {
                continue;
            }
            let ref_id = format!("{}@{}", m.thread_id, now.timestamp_millis());
            let Ok(Some(id)) = self.store.record_ping_from(KIND, &ref_id, Some(&m.from_addr)) else { continue };
            self.ui.ping(&text, id);
            sent.push(text);
        }
        sent
    }

    /// Records "Not useful". Returns the sender when it just reached the mute threshold.
    pub fn not_useful(&self, ping_id: i64) -> Option<String> {
        let sender = self.store.set_ping_feedback(ping_id, "not_useful").ok().flatten()?;
        let cfg = (self.cfg)();
        let known = cfg.muted.iter().chain(&cfg.suggested_mutes).any(|m| m.eq_ignore_ascii_case(&sender));
        (self.store.not_useful_count(&sender).ok()? == MUTE_AFTER && !known).then_some(sender)
    }
}

impl crate::agentwatch::hub::AgentPing for Notifier {
    /// "shop: wants to run git push --force", or "2 agents need you". One ping per ask,
    /// under the same calm rules as mail.
    fn waiting(&self, project: &str, summary: &str, ask_id: i64, waiting_now: usize) {
        let now = (self.now)();
        let local = now.with_timezone(&chrono::Local);
        let ref_id = format!("ask:{ask_id}");
        let last = self.store.last_ping(AGENT_KIND, &ref_id).ok().flatten();
        let in_hour = self.store.pings_since_kinds(BUDGET_KINDS, now - Duration::hours(1)).unwrap_or(usize::MAX);
        if may_ping(&self.calm, local.naive_local(), in_hour, last, now, self.paused.load(Ordering::SeqCst)) != Verdict::Send {
            return;
        }
        let folder = project.trim_end_matches('/').rsplit('/').next().unwrap_or(project);
        let text = if waiting_now >= 2 { format!("{waiting_now} agents need you") } else { cut(&format!("{folder}: {summary}"), 80) };
        if let Ok(Some(_)) = self.store.record_ping_from(AGENT_KIND, &ref_id, None) {
            self.ui.bubble(&text);
        }
    }
}

pub fn suggestion_text(n: usize) -> String {
    if n == 1 { "I have a memory suggestion for you.".into() } else { format!("I have {n} memory suggestions for you.") }
}

impl crate::proactive::patterns::SuggestPing for Notifier {
    /// One bubble a day at most, under the calm rules. Settings > Memory lists them.
    fn suggestions(&self, n: usize) {
        let now = (self.now)();
        let local = now.with_timezone(&chrono::Local);
        let ref_id = format!("suggest:{}", local.date_naive());
        let last = self.store.last_ping(MEMORY_KIND, &ref_id).ok().flatten();
        let in_hour = self.store.pings_since_kinds(BUDGET_KINDS, now - Duration::hours(1)).unwrap_or(usize::MAX);
        if last.is_some() || may_ping(&self.calm, local.naive_local(), in_hour, last, now, self.paused.load(Ordering::SeqCst)) != Verdict::Send {
            return;
        }
        if let Ok(Some(_)) = self.store.record_ping_from(MEMORY_KIND, &ref_id, None) {
            self.ui.bubble(&suggestion_text(n));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::tests::{day, mail, utc};
    use std::sync::Mutex;

    fn at(local: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(local, "%Y-%m-%d %H:%M").unwrap()
    }

    fn now() -> DateTime<Utc> {
        // 10:00 UTC is outside quiet hours both in UTC (CI) and in India.
        utc("2026-11-12T10:00:00Z")
    }

    #[test]
    fn quiet_hours_hold() {
        let c = Calm::default();
        assert_eq!(may_ping(&c, at("2026-11-12 23:30"), 0, None, now(), false), Verdict::Hold);
        assert_eq!(may_ping(&c, at("2026-11-12 07:59"), 0, None, now(), false), Verdict::Hold);
        assert_eq!(may_ping(&c, at("2026-11-12 08:00"), 0, None, now(), false), Verdict::Send);
        assert_eq!(may_ping(&c, at("2026-11-12 22:59"), 0, None, now(), false), Verdict::Send);
    }

    #[test]
    fn fifth_ping_in_an_hour_merges() {
        let c = Calm::default();
        assert_eq!(may_ping(&c, at("2026-11-12 15:00"), 3, None, now(), false), Verdict::Send);
        assert_eq!(may_ping(&c, at("2026-11-12 15:00"), 4, None, now(), false), Verdict::Merge);
    }

    #[test]
    fn same_thread_within_6h_skips() {
        let c = Calm::default();
        assert_eq!(may_ping(&c, at("2026-11-12 15:00"), 0, Some(now() - Duration::hours(5)), now(), false), Verdict::Skip);
        assert_eq!(may_ping(&c, at("2026-11-12 15:00"), 0, Some(now() - Duration::hours(7)), now(), false), Verdict::Send);
    }

    #[test]
    fn pause_skips() {
        assert_eq!(may_ping(&Calm::default(), at("2026-11-12 15:00"), 0, None, now(), true), Verdict::Skip);
    }

    #[derive(Default)]
    struct FakeUi(Mutex<Vec<String>>);
    impl BarUi for FakeUi {
        fn show(&self, _c: &crate::proactive::cues::BarCue) {}
        fn hide(&self) {}
        fn bubble(&self, text: &str) {
            self.0.lock().unwrap().push(text.into());
        }
        fn ping(&self, text: &str, id: i64) {
            self.0.lock().unwrap().push(format!("{text} #{id}"));
        }
    }

    struct Rig {
        _d: tempfile::TempDir,
        store: Arc<Store>,
        ui: Arc<FakeUi>,
        n: Arc<Notifier>,
    }

    fn rig() -> Rig {
        let d = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::open(&d.path().join("b.db")).unwrap());
        let ui = Arc::new(FakeUi::default());
        let cfg = || MailConfig { vips: vec!["boss@example.com".into()], ..Default::default() };
        let n = Notifier::new(store.clone(), ui.clone(), Arc::new(AtomicBool::new(false)), Arc::new(cfg), Arc::new(now));
        Rig { _d: d, store, ui, n }
    }

    fn bill(id: &str, due: &str) -> MailRow {
        let mut m = mail(id, id, 1);
        m.from_name = Some("City Power".into());
        m.from_addr = "bills@city.example".into();
        m.triage = Some(Label::Bill);
        m.amount = Some("₹2,340".into());
        m.due = Some(day(due));
        m
    }

    #[test]
    fn bills_due_soon_and_vip_mail_ping() {
        let r = rig();
        let today = now().with_timezone(&chrono::Local).date_naive().to_string();
        let mut vip = mail("v", "tv", 1);
        vip.from_addr = "boss@example.com".into();
        vip.from_name = Some("Boss".into());
        vip.subject = "Can you send the numbers before the board call tomorrow morning please".into();
        vip.triage = Some(Label::NeedsReply);
        let mut other = mail("o", "to", 1);
        other.triage = Some(Label::NeedsReply);
        let sent = r.n.after_triage(&[bill("b", &today), bill("far", "2026-12-30"), vip, other]);
        assert_eq!(sent, vec!["Bill due today: City Power ₹2,340".to_string(), "Boss needs a reply: Can you send the numbers before the board call tomorrow morn…".to_string()]);
        let ui = r.ui.0.lock().unwrap().clone();
        assert!(ui[0].starts_with("Bill due today: City Power ₹2,340 #"), "{ui:?}");
        // The same thread does not ping again within 6 hours.
        assert!(r.n.after_triage(&[bill("b2", &today)]).len() == 1, "another thread pings");
        let mut again = bill("b3", &today);
        again.thread_id = "b".into();
        assert!(r.n.after_triage(&[again]).is_empty());
    }

    #[test]
    fn long_sender_names_are_cut() {
        let r = rig();
        let today = now().with_timezone(&chrono::Local).date_naive().to_string();
        let mut b = bill("long", &today);
        b.from_name = Some("Totally Real Bank Customer Care Department Please Call Now".into());
        let sent = r.n.after_triage(&[b]);
        assert_eq!(sent, vec!["Bill due today: Totally Real Bank Customer Care Departme… ₹2,340".to_string()]);
    }

    #[test]
    fn seeded_and_promo_never_ping() {
        let r = rig();
        let today = now().with_timezone(&chrono::Local).date_naive().to_string();
        let mut seeded = bill("s", &today);
        seeded.seeded = true;
        let mut promo = mail("p", "p", 1);
        promo.from_addr = "boss@example.com".into();
        promo.triage = Some(Label::Ignore);
        assert!(r.n.after_triage(&[seeded, promo]).is_empty());
        assert!(r.ui.0.lock().unwrap().is_empty());
    }

    #[test]
    fn third_not_useful_suggests_mute() {
        let r = rig();
        let ids: Vec<i64> = (0..3).map(|i| r.store.record_ping_from(KIND, &format!("t{i}@x"), Some("bills@city.example")).unwrap().unwrap()).collect();
        assert_eq!(r.n.not_useful(ids[0]), None);
        assert_eq!(r.n.not_useful(ids[1]), None);
        assert_eq!(r.n.not_useful(ids[2]).as_deref(), Some("bills@city.example"));
        assert_eq!(r.n.not_useful(999), None);
    }

    #[test]
    fn agent_ping_names_the_folder() {
        let r = rig();
        crate::agentwatch::hub::AgentPing::waiting(&*r.n, "/Users/me/code/shop", "wants to run git push --force", 7, 1);
        assert_eq!(r.ui.0.lock().unwrap().as_slice(), ["shop: wants to run git push --force"]);
        crate::agentwatch::hub::AgentPing::waiting(&*r.n, "/Users/me/code/shop", "wants to run git push --force", 7, 1);
        assert_eq!(r.ui.0.lock().unwrap().len(), 1, "one ping per ask");
    }

    #[test]
    fn memory_suggestions_bubble_once_and_share_the_budget() {
        let r = rig();
        crate::proactive::patterns::SuggestPing::suggestions(&*r.n, 2);
        assert_eq!(r.ui.0.lock().unwrap().as_slice(), ["I have 2 memory suggestions for you."]);
        crate::proactive::patterns::SuggestPing::suggestions(&*r.n, 1);
        assert_eq!(r.ui.0.lock().unwrap().len(), 1, "one a day");
        assert_eq!(suggestion_text(1), "I have a memory suggestion for you.");
        assert_eq!(r.store.pings_since_kinds(BUDGET_KINDS, now() - Duration::hours(1)).unwrap(), 1);
        assert!(BUDGET_KINDS.contains(&MEMORY_KIND) && BUDGET_KINDS.contains(&KIND) && BUDGET_KINDS.contains(&AGENT_KIND));
    }

    #[test]
    fn two_waiting_say_n_agents() {
        let r = rig();
        crate::agentwatch::hub::AgentPing::waiting(&*r.n, "/Users/me/code/shop", "wants to run rm -rf dist", 8, 2);
        assert_eq!(r.ui.0.lock().unwrap().as_slice(), ["2 agents need you"]);
    }

    #[test]
    fn agent_pings_share_hourly_budget_with_mail() {
        let r = rig();
        for i in 0..4 {
            r.store.record_ping(KIND, &format!("t{i}@1")).unwrap();
        }
        crate::agentwatch::hub::AgentPing::waiting(&*r.n, "/p/shop", "wants to run x", 9, 1);
        assert!(r.ui.0.lock().unwrap().is_empty());
        let r = rig();
        for i in 0..4 {
            r.store.record_ping(AGENT_KIND, &format!("ask:{i}")).unwrap();
        }
        assert!(r.n.after_triage(&[bill("b1", "2026-11-12")]).is_empty());
    }
}

