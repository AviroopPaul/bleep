//! When the meeting bar shows. Pure: events + now in, at most one cue out.
use crate::connectors::calendar::CalEvent;
use chrono::{DateTime, Duration, Utc};
use serde::Serialize;

#[derive(Serialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CueKind {
    /// From 60 to 10 minutes before the start.
    Hour,
    /// The last 10 minutes before the start.
    TenMin,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct BarCue {
    pub key: String,
    pub kind: CueKind,
    pub event_id: String,
    pub title: String,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub meet_link: Option<String>,
    pub attendees: Vec<String>,
}

/// One key per event, start time and window: a moved meeting gets new cues.
pub fn cue_key(event_id: &str, start: DateTime<Utc>, kind: CueKind) -> String {
    let k = match kind {
        CueKind::Hour => "hour",
        CueKind::TenMin => "ten",
    };
    format!("{event_id}@{}:{k}", start.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}

/// The cue to show now, if any: the soonest timed meeting I have not declined, inside its
/// window, whose key `seen` does not know yet.
/// A meeting worth a bar: timed, not declined, and with another person or a meeting link.
/// Reminders and solo blocks (focus time, "Pay rent") get no bar.
pub fn bar_worthy(e: &CalEvent) -> bool {
    !e.all_day && e.my_response.as_deref() != Some("declined") && (e.meet_link.is_some() || e.attendees.len() >= 2)
}

pub fn due_cue(events: &[CalEvent], now: DateTime<Utc>, seen: &dyn Fn(&str) -> bool) -> Option<BarCue> {
    let mut list: Vec<&CalEvent> = events.iter().filter(|e| bar_worthy(e) && e.start > now).collect();
    list.sort_by_key(|e| e.start);
    list.into_iter().find_map(|e| {
        let left = e.start - now;
        let kind = if left <= Duration::minutes(10) {
            CueKind::TenMin
        } else if left <= Duration::minutes(60) {
            CueKind::Hour
        } else {
            return None;
        };
        let key = cue_key(&e.event_id, e.start, kind);
        (!seen(&key)).then(|| BarCue {
            key,
            kind,
            event_id: e.event_id.clone(),
            title: e.title.clone(),
            start: e.start,
            end: e.end,
            meet_link: e.meet_link.clone(),
            attendees: e.attendees.clone(),
        })
    })
}

/// The droid bubble line, for example "Design review in 1 hour".
pub fn bubble_text(cue: &BarCue, now: DateTime<Utc>) -> String {
    let title: String = if cue.title.chars().count() > 40 { cue.title.chars().take(40).collect::<String>() + "…" } else { cue.title.clone() };
    let mins = ((cue.start - now).num_seconds().max(0) + 59) / 60;
    if cue.kind == CueKind::Hour && mins >= 55 {
        format!("{title} in 1 hour")
    } else {
        format!("{title} in {mins} min")
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-11-12T09:00:00Z").unwrap().with_timezone(&Utc)
    }

    pub fn ev(id: &str, mins_from_now: i64) -> CalEvent {
        let start = now() + Duration::minutes(mins_from_now);
        CalEvent {
            event_id: id.into(),
            title: format!("Meeting {id}"),
            start,
            end: start + Duration::minutes(30),
            all_day: false,
            attendees: vec!["priya@example.com".into()],
            meet_link: Some("https://meet.google.com/abc".into()),
            my_response: Some("accepted".into()),
            html_link: None,
            description_untrusted: None,
        }
    }

    fn none(_: &str) -> bool {
        false
    }

    #[test]
    fn hour_cue_inside_its_window() {
        let c = due_cue(&[ev("a", 45)], now(), &none).unwrap();
        assert_eq!((c.kind, c.event_id.as_str(), c.title.as_str()), (CueKind::Hour, "a", "Meeting a"));
        assert_eq!(due_cue(&[ev("a", 60)], now(), &none).unwrap().kind, CueKind::Hour);
        assert_eq!(c.meet_link.as_deref(), Some("https://meet.google.com/abc"));
    }

    #[test]
    fn ten_min_cue_inside_its_window() {
        assert_eq!(due_cue(&[ev("a", 7)], now(), &none).unwrap().kind, CueKind::TenMin);
        assert_eq!(due_cue(&[ev("a", 10)], now(), &none).unwrap().kind, CueKind::TenMin);
    }

    #[test]
    fn nothing_before_the_hour_window() {
        assert_eq!(due_cue(&[ev("a", 61)], now(), &none), None);
    }

    #[test]
    fn past_and_started_meetings_get_nothing() {
        assert_eq!(due_cue(&[ev("a", 0), ev("b", -5), ev("c", -120)], now(), &none), None);
    }

    #[test]
    fn declined_and_all_day_get_nothing() {
        let mut d = ev("d", 30);
        d.my_response = Some("declined".into());
        let mut h = ev("h", 30);
        h.all_day = true;
        assert_eq!(due_cue(&[d, h], now(), &none), None);
        let mut t = ev("t", 30);
        t.my_response = Some("tentative".into());
        assert!(due_cue(&[t], now(), &none).is_some());
    }

    #[test]
    fn seen_cue_is_not_repeated_but_ten_min_still_fires() {
        let e = ev("a", 45);
        let hour = cue_key("a", e.start, CueKind::Hour);
        let seen = |k: &str| k == hour;
        assert_eq!(due_cue(&[e.clone()], now(), &seen), None);
        let later = now() + Duration::minutes(37);
        assert_eq!(due_cue(&[e], later, &seen).unwrap().kind, CueKind::TenMin);
    }

    #[test]
    fn moved_meeting_gets_a_new_key() {
        let a = ev("a", 45);
        let b = ev("a", 50);
        assert_ne!(cue_key("a", a.start, CueKind::Hour), cue_key("a", b.start, CueKind::Hour));
        assert_ne!(cue_key("a", a.start, CueKind::Hour), cue_key("a", a.start, CueKind::TenMin));
        let old = cue_key("a", a.start, CueKind::Hour);
        assert!(due_cue(&[b], now(), &|k| k == old).is_some());
    }

    #[test]
    fn soonest_meeting_wins() {
        assert_eq!(due_cue(&[ev("late", 50), ev("soon", 20)], now(), &none).unwrap().event_id, "soon");
        let soon = ev("soon", 20);
        let k = cue_key("soon", soon.start, CueKind::Hour);
        assert_eq!(due_cue(&[ev("late", 50), soon], now(), &|x| x == k).unwrap().event_id, "late");
    }

    #[test]
    fn solo_events_and_reminders_get_no_bar() {
        let mut solo = ev("pay", 30);
        solo.attendees = vec![];
        solo.meet_link = None;
        assert_eq!(due_cue(&[solo.clone()], now(), &none), None);
        let mut just_me = solo.clone();
        just_me.attendees = vec!["me@example.com".into()];
        assert_eq!(due_cue(&[just_me], now(), &none), None);
        let mut with_link = solo.clone();
        with_link.meet_link = Some("https://meet.google.com/x".into());
        assert!(due_cue(&[with_link], now(), &none).is_some());
        let mut with_people = solo;
        with_people.attendees = vec!["me@example.com".into(), "priya@example.com".into()];
        assert!(due_cue(&[with_people], now(), &none).is_some());
    }

    #[test]
    fn bubble_text_says_when() {
        let c = due_cue(&[ev("a", 60)], now(), &none).unwrap();
        assert_eq!(bubble_text(&c, now()), "Meeting a in 1 hour");
        let c = due_cue(&[ev("a", 42)], now(), &none).unwrap();
        assert_eq!(bubble_text(&c, now()), "Meeting a in 42 min");
        let c = due_cue(&[ev("a", 10)], now(), &none).unwrap();
        assert_eq!(bubble_text(&c, now()), "Meeting a in 10 min");
        let mut long = ev("a", 10);
        long.title = "A very long meeting title that goes on and on and on".into();
        let c = due_cue(&[long], now(), &none).unwrap();
        assert_eq!(bubble_text(&c, now()), "A very long meeting title that goes on a… in 10 min");
    }
}
