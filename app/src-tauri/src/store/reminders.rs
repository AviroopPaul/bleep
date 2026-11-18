//! Native reminders (0.5.0, F5): bleep's own, kept on this Mac. No Google needed.
use super::*;

str_enum!(Repeat { Daily = "daily", Weekdays = "weekdays", Weekly = "weekly", Monthly = "monthly" });
str_enum!(ReminderStatus { Scheduled = "scheduled", Fired = "fired", Done = "done" });

impl Repeat {
    pub fn from_arg(s: &str) -> Result<Option<Repeat>, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "" | "none" | "never" | "once" => Ok(None),
            "daily" | "day" | "every day" => Ok(Some(Repeat::Daily)),
            "weekdays" | "weekday" => Ok(Some(Repeat::Weekdays)),
            "weekly" | "week" | "every week" => Ok(Some(Repeat::Weekly)),
            "monthly" | "month" | "every month" => Ok(Some(Repeat::Monthly)),
            other => Err(format!("Unknown repeat: {other}. Use none, daily, weekdays, weekly or monthly.")),
        }
    }
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Reminder {
    pub id: i64,
    pub text: String,
    /// The next time it is due, UTC.
    pub due_at: DateTime<Utc>,
    pub repeat: Option<Repeat>,
    pub status: ReminderStatus,
    /// Set by Snooze: fires at this time instead, once.
    pub snoozed_until: Option<DateTime<Utc>>,
    pub fired_at: Option<DateTime<Utc>>,
    /// "chat" (set by the model) or "user" (typed in the list).
    pub source: String,
    pub created_at: String,
}

impl Reminder {
    /// When it fires next: the snooze if set and sooner, else the due time.
    pub fn next_at(&self) -> DateTime<Utc> {
        match self.snoozed_until {
            Some(s) if s < self.due_at || self.repeat.is_none() => s,
            _ => self.due_at,
        }
    }
}

const REMINDER_COLS: &str = "id, text, due_at, repeat, status, snoozed_until, fired_at, source, created_at";

fn opt_ts(s: Option<String>) -> Option<DateTime<Utc>> {
    s.as_deref().map(parse_ts)
}

fn row_to_reminder(r: &Row) -> rusqlite::Result<Reminder> {
    Ok(Reminder {
        id: r.get(0)?,
        text: r.get(1)?,
        due_at: parse_ts(&r.get::<_, String>(2)?),
        repeat: r.get::<_, Option<String>>(3)?.map(|s| Repeat::parse(&s)),
        status: ReminderStatus::parse(&r.get::<_, String>(4)?),
        snoozed_until: opt_ts(r.get(5)?),
        fired_at: opt_ts(r.get(6)?),
        source: r.get(7)?,
        created_at: r.get(8)?,
    })
}

/// The first time after `after` that a repeating reminder falls on, keeping its local wall
/// time. Monthly keeps the day of the month, or the month's last day when it is shorter.
pub fn next_due<Tz: chrono::TimeZone>(due: DateTime<Utc>, repeat: Repeat, after: DateTime<Utc>, tz: &Tz) -> DateTime<Utc> {
    use chrono::{Datelike, Duration as D, NaiveDate, Weekday};
    let start = due.with_timezone(tz).naive_local();
    let (day0, time) = (start.date(), start.time());
    let step = |n: i64| -> NaiveDate {
        match repeat {
            Repeat::Daily | Repeat::Weekdays => day0 + D::days(n),
            Repeat::Weekly => day0 + D::weeks(n),
            Repeat::Monthly => {
                let m0 = day0.year() as i64 * 12 + day0.month0() as i64 + n;
                let (y, m) = ((m0 / 12) as i32, (m0 % 12) as u32 + 1);
                (1..=day0.day()).rev().find_map(|d| NaiveDate::from_ymd_opt(y, m, d)).unwrap_or(day0)
            }
        }
    };
    let to_utc = |d: NaiveDate| -> DateTime<Utc> {
        let n = d.and_time(time);
        tz.from_local_datetime(&n).earliest().or_else(|| tz.from_local_datetime(&(n + D::hours(1))).earliest()).map(|t| t.with_timezone(&Utc)).unwrap_or_else(|| n.and_utc())
    };
    // Jump close to `after` first, so a reminder off for months does not loop day by day.
    let behind_days = (after - due).num_days().max(0);
    let mut n = match repeat {
        Repeat::Daily | Repeat::Weekdays => (behind_days - 1).max(0),
        Repeat::Weekly => (behind_days / 7 - 1).max(0),
        Repeat::Monthly => (behind_days / 31 - 1).max(0),
    } + 1;
    loop {
        let d = step(n);
        let weekend = matches!(d.weekday(), Weekday::Sat | Weekday::Sun);
        let t = to_utc(d);
        if t > after && !(repeat == Repeat::Weekdays && weekend) {
            return t;
        }
        n += 1;
    }
}

impl Store {
    pub fn add_reminder(&self, text: &str, due_at: DateTime<Utc>, repeat: Option<Repeat>, source: &str) -> Result<Reminder> {
        let db = self.db();
        db.execute(
            "INSERT INTO reminders (text, due_at, repeat, source) VALUES (?1, ?2, ?3, ?4)",
            params![text, ts(due_at), repeat.map(|r| r.as_str()), source],
        )?;
        let sql = format!("SELECT {REMINDER_COLS} FROM reminders WHERE id = ?1");
        Ok(db.query_row(&sql, params![db.last_insert_rowid()], row_to_reminder)?)
    }

    pub fn reminder(&self, id: i64) -> Result<Option<Reminder>> {
        let sql = format!("SELECT {REMINDER_COLS} FROM reminders WHERE id = ?1");
        Ok(self.db().query_row(&sql, params![id], row_to_reminder).optional()?)
    }

    /// Fired ones first (they wait for Done), then upcoming by time, then the last `done` done ones.
    pub fn reminders(&self, done: usize) -> Result<Vec<Reminder>> {
        let db = self.db();
        let sql = format!(
            "SELECT {REMINDER_COLS} FROM reminders WHERE status != 'done'
             ORDER BY CASE status WHEN 'fired' THEN 0 ELSE 1 END, COALESCE(MIN(snoozed_until, due_at), due_at)"
        );
        let mut st = db.prepare(&sql)?;
        let mut out: Vec<Reminder> = st.query_map([], row_to_reminder)?.collect::<rusqlite::Result<_>>()?;
        let sql = format!("SELECT {REMINDER_COLS} FROM reminders WHERE status = 'done' ORDER BY COALESCE(fired_at, due_at) DESC LIMIT ?1");
        let mut st = db.prepare(&sql)?;
        out.extend(st.query_map(params![done as i64], row_to_reminder)?.collect::<rusqlite::Result<Vec<_>>>()?);
        Ok(out)
    }

    /// Scheduled reminders, soonest first.
    pub fn scheduled_reminders(&self) -> Result<Vec<Reminder>> {
        let sql = format!("SELECT {REMINDER_COLS} FROM reminders WHERE status = 'scheduled'");
        let db = self.db();
        let mut st = db.prepare(&sql)?;
        let mut v: Vec<Reminder> = st.query_map([], row_to_reminder)?.collect::<rusqlite::Result<_>>()?;
        v.sort_by_key(|r| r.next_at());
        Ok(v)
    }

    pub fn update_reminder(&self, id: i64, text: &str, due_at: DateTime<Utc>, repeat: Option<Repeat>) -> Result<bool> {
        Ok(self.db().execute(
            "UPDATE reminders SET text = ?2, due_at = ?3, repeat = ?4, status = 'scheduled', snoozed_until = NULL WHERE id = ?1",
            params![id, text, ts(due_at), repeat.map(|r| r.as_str())],
        )? > 0)
    }

    /// After it fires: a one-off waits for Done; a repeating one moves to its next time.
    pub fn mark_reminder_fired(&self, id: i64, at: DateTime<Utc>, next_due: Option<DateTime<Utc>>) -> Result<()> {
        let db = self.db();
        match next_due {
            Some(n) => db.execute("UPDATE reminders SET due_at = ?2, snoozed_until = NULL, fired_at = ?3 WHERE id = ?1", params![id, ts(n), ts(at)])?,
            None => db.execute("UPDATE reminders SET status = 'fired', snoozed_until = NULL, fired_at = ?2 WHERE id = ?1", params![id, ts(at)])?,
        };
        Ok(())
    }

    /// A repeating reminder only drops its snooze; it keeps its next time.
    pub fn reminder_done(&self, id: i64) -> Result<bool> {
        Ok(self.db().execute(
            "UPDATE reminders SET status = CASE WHEN repeat IS NULL THEN 'done' ELSE 'scheduled' END, snoozed_until = NULL WHERE id = ?1",
            params![id],
        )? > 0)
    }

    pub fn snooze_reminder(&self, id: i64, until: DateTime<Utc>) -> Result<bool> {
        Ok(self.db().execute("UPDATE reminders SET status = 'scheduled', snoozed_until = ?2 WHERE id = ?1", params![id, ts(until)])? > 0)
    }

    pub fn delete_reminder(&self, id: i64) -> Result<bool> {
        Ok(self.db().execute("DELETE FROM reminders WHERE id = ?1", params![id])? > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::tests::utc;
    use chrono::FixedOffset;

    fn store() -> (tempfile::TempDir, Store) {
        let d = tempfile::tempdir().unwrap();
        let s = Store::open(&d.path().join("b.db")).unwrap();
        (d, s)
    }

    // India: UTC+5:30, no DST.
    fn ist() -> FixedOffset {
        FixedOffset::east_opt(5 * 3600 + 1800).unwrap()
    }

    #[test]
    fn daily_keeps_the_local_time() {
        // 19:00 IST on Tue 17 Nov 2026 = 13:30 UTC.
        let due = utc("2026-11-17T13:30:00Z");
        assert_eq!(next_due(due, Repeat::Daily, due, &ist()), utc("2026-11-18T13:30:00Z"));
        assert_eq!(next_due(due, Repeat::Daily, utc("2026-11-20T14:00:00Z"), &ist()), utc("2026-11-21T13:30:00Z"), "after a long sleep, the next one is in the future");
    }

    #[test]
    fn weekdays_skip_the_weekend() {
        // Fri 20 Nov 2026, 09:00 IST.
        let fri = utc("2026-11-20T03:30:00Z");
        assert_eq!(next_due(fri, Repeat::Weekdays, fri, &ist()), utc("2026-11-23T03:30:00Z"), "Friday goes to Monday");
    }

    #[test]
    fn weekly_and_monthly() {
        let due = utc("2026-11-17T13:30:00Z");
        assert_eq!(next_due(due, Repeat::Weekly, due, &ist()), utc("2026-11-24T13:30:00Z"));
        // 31 Jan 10:00 IST -> 28 Feb (2027 is not a leap year) -> 31 Mar.
        let jan31 = utc("2027-01-31T04:30:00Z");
        assert_eq!(next_due(jan31, Repeat::Monthly, jan31, &ist()), utc("2027-02-28T04:30:00Z"));
        assert_eq!(next_due(jan31, Repeat::Monthly, utc("2027-03-01T00:00:00Z"), &ist()), utc("2027-03-31T04:30:00Z"));
    }

    #[test]
    fn one_off_life_cycle() {
        let (_d, s) = store();
        let r = s.add_reminder("Call mom", utc("2026-11-17T13:30:00Z"), None, "chat").unwrap();
        assert_eq!((r.status, r.next_at()), (ReminderStatus::Scheduled, utc("2026-11-17T13:30:00Z")));
        assert_eq!(s.scheduled_reminders().unwrap().len(), 1);
        s.mark_reminder_fired(r.id, utc("2026-11-17T13:30:05Z"), None).unwrap();
        assert!(s.scheduled_reminders().unwrap().is_empty());
        assert_eq!(s.reminders(10).unwrap()[0].status, ReminderStatus::Fired);
        s.snooze_reminder(r.id, utc("2026-11-17T13:40:05Z")).unwrap();
        let back = s.reminder(r.id).unwrap().unwrap();
        assert_eq!((back.status, back.next_at()), (ReminderStatus::Scheduled, utc("2026-11-17T13:40:05Z")));
        s.reminder_done(r.id).unwrap();
        assert_eq!(s.reminder(r.id).unwrap().unwrap().status, ReminderStatus::Done);
        assert!(s.delete_reminder(r.id).unwrap());
        assert!(s.reminders(10).unwrap().is_empty());
    }

    #[test]
    fn repeating_moves_on_and_done_keeps_it() {
        let (_d, s) = store();
        let r = s.add_reminder("Stretch", utc("2026-11-17T03:30:00Z"), Some(Repeat::Daily), "user").unwrap();
        s.mark_reminder_fired(r.id, utc("2026-11-17T03:30:00Z"), Some(utc("2026-11-18T03:30:00Z"))).unwrap();
        s.snooze_reminder(r.id, utc("2026-11-17T03:40:00Z")).unwrap();
        assert_eq!(s.reminder(r.id).unwrap().unwrap().next_at(), utc("2026-11-17T03:40:00Z"), "a snooze comes before the next day");
        s.reminder_done(r.id).unwrap();
        let r = s.reminder(r.id).unwrap().unwrap();
        assert_eq!((r.status, r.next_at()), (ReminderStatus::Scheduled, utc("2026-11-18T03:30:00Z")));
    }

    #[test]
    fn list_order_fired_then_upcoming_then_done() {
        let (_d, s) = store();
        let a = s.add_reminder("later", utc("2026-11-19T00:00:00Z"), None, "user").unwrap();
        let b = s.add_reminder("soon", utc("2026-11-18T00:00:00Z"), None, "user").unwrap();
        let c = s.add_reminder("fired", utc("2026-11-17T00:00:00Z"), None, "user").unwrap();
        let d = s.add_reminder("old", utc("2026-11-16T00:00:00Z"), None, "user").unwrap();
        s.mark_reminder_fired(c.id, utc("2026-11-17T00:00:00Z"), None).unwrap();
        s.reminder_done(d.id).unwrap();
        let ids: Vec<i64> = s.reminders(10).unwrap().iter().map(|r| r.id).collect();
        assert_eq!(ids, [c.id, b.id, a.id, d.id]);
        assert_eq!(s.reminders(0).unwrap().len(), 3);
    }

    #[test]
    fn repeat_args() {
        assert_eq!(Repeat::from_arg("none").unwrap(), None);
        assert_eq!(Repeat::from_arg("Weekdays").unwrap(), Some(Repeat::Weekdays));
        assert!(Repeat::from_arg("hourly").is_err());
    }
}
