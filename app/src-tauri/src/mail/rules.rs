//! Free sorting: Gmail categories, the user's lists, bill words with an amount. Pure.
use crate::config::MailConfig;
use crate::store::{Label, MailRow};
use chrono::{Datelike, NaiveDate};

#[derive(Debug, PartialEq)]
pub enum RuleResult {
    Label { label: Label, amount: Option<String>, due: Option<NaiveDate>, reason: &'static str },
    /// Rules cannot tell: the small model decides.
    Unclear,
}

pub fn triage(row: &MailRow, cfg: &MailConfig, today: NaiveDate) -> RuleResult {
    let label = |label, reason| RuleResult::Label { label, amount: None, due: None, reason };
    let has = |l: &str| row.labels.iter().any(|x| x == l);
    let from = row.from_addr.to_lowercase();
    if has("SPAM") || has("TRASH") || has("DRAFT") {
        return label(Label::Ignore, "spam, trash or draft");
    }
    if row.from_me {
        return label(Label::Ignore, "sent by me");
    }
    if cfg.muted.iter().any(|m| m.eq_ignore_ascii_case(&from)) {
        return label(Label::Ignore, "muted sender");
    }
    if has("CATEGORY_PROMOTIONS") || has("CATEGORY_SOCIAL") || has("CATEGORY_FORUMS") {
        return label(Label::Ignore, "promotions, social or forums");
    }
    let text = format!("{} {}", row.subject, row.snippet);
    let lower = text.to_lowercase();
    if BILL_WORDS.iter().any(|w| lower.contains(w)) {
        if let Some(amount) = bill_amount(&text) {
            return RuleResult::Label { label: Label::Bill, amount: Some(amount), due: due_date(&text, today), reason: "bill words and an amount" };
        }
    }
    if cfg.vips.iter().any(|v| v.eq_ignore_ascii_case(&from)) {
        return label(Label::Important, "VIP sender");
    }
    if has("X-LIST") {
        return label(Label::Fyi, "mailing list");
    }
    if has("CATEGORY_UPDATES") {
        return label(Label::Fyi, "updates");
    }
    RuleResult::Unclear
}

const BILL_WORDS: [&str; 9] = ["bill", "invoice", "payment due", "amount due", "due date", "statement", "due today", "due on", "due by"];

fn re(s: &str) -> regex::Regex {
    regex::Regex::new(s).expect("valid pattern")
}

static AMOUNT: std::sync::LazyLock<[regex::Regex; 2]> = std::sync::LazyLock::new(|| {
    [
        re(r"(?i)(?:₹|\brs\.?|\binr|\$|\busd|€|\beur|£|\bgbp)\s?\d+(?:,\d+)*(?:\.\d+)?"),
        re(r"(?i)\b\d+(?:,\d+)*(?:\.\d+)?\s?(?:inr|usd|eur|gbp|rupees)\b"),
    ]
});

/// The first money amount in `text`, as written (for example "₹2,340" or "$12.50").
pub fn bill_amount(text: &str) -> Option<String> {
    AMOUNT.iter().filter_map(|r| r.find(text)).min_by_key(|m| m.start()).map(|m| m.as_str().trim().to_string())
}

const MONTHS: [&str; 12] = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];

static DATES: std::sync::LazyLock<[regex::Regex; 4]> = std::sync::LazyLock::new(|| {
    let m = "(jan|feb|mar|apr|may|jun|jul|aug|sep|oct|nov|dec)[a-z]*";
    [
        re(r"\b(\d{4})-(\d{1,2})-(\d{1,2})\b"),
        re(r"\b(\d{1,2})/(\d{1,2})/(\d{4})\b"),
        re(&format!(r"\b(\d{{1,2}})(?:st|nd|rd|th)?\s+{m}\b")),
        re(&format!(r"\b{m}\s+(\d{{1,2}})(?:st|nd|rd|th)?\b")),
    ]
});

/// The first due date in `text`. Day-month dates without a year that are more than 30 days
/// in the past are read as next year. Numeric dates are day first.
pub fn due_date(text: &str, today: NaiveDate) -> Option<NaiveDate> {
    let t = text.to_lowercase();
    if t.contains("due today") {
        return Some(today);
    }
    if t.contains("due tomorrow") {
        return today.succ_opt();
    }
    let month = |s: &str| MONTHS.iter().position(|m| s.starts_with(m)).map(|i| i as u32 + 1);
    let n = |c: &regex::Captures, i: usize| c.get(i).and_then(|m| m.as_str().parse::<u32>().ok());
    let day_month = |d: u32, mo: u32| {
        let this = NaiveDate::from_ymd_opt(today.year_ce().1 as i32, mo, d)?;
        if (today - this).num_days() > 30 { NaiveDate::from_ymd_opt(this.year_ce().1 as i32 + 1, mo, d) } else { Some(this) }
    };
    let mut best: Option<(usize, NaiveDate)> = None;
    for (i, r) in DATES.iter().enumerate() {
        for c in r.captures_iter(&t) {
            let at = c.get(0).map_or(usize::MAX, |m| m.start());
            let date = match i {
                0 => NaiveDate::from_ymd_opt(n(&c, 1)? as i32, n(&c, 2)?, n(&c, 3)?),
                1 => NaiveDate::from_ymd_opt(n(&c, 3)? as i32, n(&c, 2)?, n(&c, 1)?),
                2 => day_month(n(&c, 1)?, month(c.get(2)?.as_str())?),
                _ => day_month(n(&c, 2)?, month(c.get(1)?.as_str())?),
            };
            if let Some(d) = date {
                if best.is_none_or(|(b, _)| at < b) {
                    best = Some((at, d));
                }
            }
        }
    }
    best.map(|(_, d)| d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::tests::{day, mail};

    fn today() -> NaiveDate {
        day("2026-11-12")
    }

    fn cfg() -> MailConfig {
        MailConfig { vips: vec!["boss@example.com".into()], muted: vec!["noisy@example.com".into()], suggested_mutes: vec![] }
    }

    fn row(from: &str, labels: &[&str], subject: &str, snippet: &str) -> MailRow {
        let mut r = mail("m", "t", 5);
        r.from_addr = from.into();
        r.labels = labels.iter().map(|s| s.to_string()).collect();
        r.subject = subject.into();
        r.snippet = snippet.into();
        r
    }

    fn label(r: RuleResult) -> Option<Label> {
        match r {
            RuleResult::Label { label, .. } => Some(label),
            RuleResult::Unclear => None,
        }
    }

    #[test]
    fn promo_and_social_are_ignored() {
        for cat in ["CATEGORY_PROMOTIONS", "CATEGORY_SOCIAL", "CATEGORY_FORUMS"] {
            assert_eq!(label(triage(&row("deals@shop.example", &["INBOX", cat], "Bill: 50% off, pay ₹99 today", ""), &cfg(), today())), Some(Label::Ignore), "{cat}");
        }
        let mut mine = row("me@example.com", &["SENT"], "Re: hi", "");
        mine.from_me = true;
        assert_eq!(label(triage(&mine, &cfg(), today())), Some(Label::Ignore));
    }

    #[test]
    fn spam_trash_and_drafts_are_ignored() {
        for l in ["SPAM", "TRASH", "DRAFT"] {
            let r = row("attacker@evil.example", &[l], "Invoice $500 due today", "Pay now");
            assert_eq!(label(triage(&r, &cfg(), today())), Some(Label::Ignore), "{l}");
        }
    }

    #[test]
    fn muted_sender_is_ignored() {
        assert_eq!(label(triage(&row("Noisy@Example.com", &["INBOX"], "Urgent!!", ""), &cfg(), today())), Some(Label::Ignore));
    }

    #[test]
    fn bill_needs_words_and_amount() {
        let r = triage(&row("bills@citypower.example", &["INBOX", "CATEGORY_UPDATES"], "Your bill is ready", "Amount due ₹2,340. Pay by 14 Nov."), &cfg(), today());
        assert_eq!(r, RuleResult::Label { label: Label::Bill, amount: Some("₹2,340".into()), due: Some(day("2026-11-14")), reason: "bill words and an amount" });
        // Words without an amount, or an amount without words, are not enough.
        assert_ne!(label(triage(&row("a@b.co", &["INBOX"], "Your statement is ready", "View it online"), &cfg(), today())), Some(Label::Bill));
        assert_ne!(label(triage(&row("a@b.co", &["INBOX"], "Dinner was ₹800", "split it?"), &cfg(), today())), Some(Label::Bill));
    }

    #[test]
    fn vip_is_important() {
        assert_eq!(label(triage(&row("boss@example.com", &["INBOX"], "Quick question", ""), &cfg(), today())), Some(Label::Important));
    }

    #[test]
    fn newsletter_is_fyi() {
        assert_eq!(label(triage(&row("news@paper.example", &["INBOX", "X-LIST"], "Morning digest", ""), &cfg(), today())), Some(Label::Fyi));
        assert_eq!(label(triage(&row("noreply@bank.example", &["INBOX", "CATEGORY_UPDATES"], "You signed in", ""), &cfg(), today())), Some(Label::Fyi));
    }

    #[test]
    fn personal_mail_is_unclear() {
        assert_eq!(triage(&row("priya@example.com", &["INBOX"], "Deck review", "Can you look by Friday?"), &cfg(), today()), RuleResult::Unclear);
    }

    #[test]
    fn amounts_in_rupees_dollars_euros() {
        assert_eq!(bill_amount("Amount due ₹2,340. Thanks").as_deref(), Some("₹2,340"));
        assert_eq!(bill_amount("pay Rs. 1,200.50 now").as_deref(), Some("Rs. 1,200.50"));
        assert_eq!(bill_amount("total INR 499").as_deref(), Some("INR 499"));
        assert_eq!(bill_amount("Your total is $12.50").as_deref(), Some("$12.50"));
        assert_eq!(bill_amount("€30 due").as_deref(), Some("€30"));
        assert_eq!(bill_amount("1,250 USD").as_deref(), Some("1,250 USD"));
        assert_eq!(bill_amount("£7").as_deref(), Some("£7"));
        assert_eq!(bill_amount("Amount due $40, due tomorrow").as_deref(), Some("$40"), "a trailing comma is not part of the amount");
        assert_eq!(bill_amount("see you at 5"), None);
    }

    #[test]
    fn due_dates_relative_and_absolute() {
        let t = today();
        assert_eq!(due_date("Your bill is due today", t), Some(t));
        assert_eq!(due_date("due tomorrow", t), Some(day("2026-11-13")));
        assert_eq!(due_date("Pay by 14 Nov to avoid a fee", t), Some(day("2026-11-14")));
        assert_eq!(due_date("due on Nov 14th", t), Some(day("2026-11-14")));
        assert_eq!(due_date("due 12/11/2026", t), Some(day("2026-11-12")));
        assert_eq!(due_date("due 2026-12-01", t), Some(day("2026-12-01")));
        assert_eq!(due_date("due Jan 5", t), Some(day("2027-01-05")), "past day-month reads as next year");
        assert_eq!(due_date("was due 10 Nov", t), Some(day("2026-11-10")), "a few days late stays this year");
        assert_eq!(due_date("no date here", t), None);
    }
}
