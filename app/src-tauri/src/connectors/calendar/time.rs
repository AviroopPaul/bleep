//! Reading times from the model and finding free time. Pure.
use chrono::{DateTime, FixedOffset, Local, NaiveDate, NaiveDateTime, TimeZone, Utc};
use std::time::Duration;

/// RFC 3339 with an offset, or a local `YYYY-MM-DDTHH:MM` read in this Mac's time zone.
pub fn parse_when(s: &str) -> Result<DateTime<FixedOffset>, String> {
    let t = s.trim();
    if let Ok(d) = DateTime::parse_from_rfc3339(t) {
        return Ok(d);
    }
    for f in ["%Y-%m-%dT%H:%M:%S", "%Y-%m-%dT%H:%M", "%Y-%m-%d %H:%M:%S", "%Y-%m-%d %H:%M"] {
        if let Ok(n) = NaiveDateTime::parse_from_str(t, f) {
            if let Some(d) = Local.from_local_datetime(&n).earliest() {
                return Ok(d.fixed_offset());
            }
        }
    }
    Err(format!("Could not read the time \"{t}\". Use a date and time like 2026-11-12T18:00."))
}

/// A date for all-day events: `YYYY-MM-DD`, or the date part of a time.
pub fn parse_day(s: &str) -> Result<NaiveDate, String> {
    let t = s.trim();
    NaiveDate::parse_from_str(t, "%Y-%m-%d")
        .or_else(|_| parse_when(t).map(|d| d.date_naive()))
        .map_err(|_| format!("Could not read the date \"{t}\". Use a date like 2026-11-12."))
}

/// Gaps in `[from, to]` of at least `min` that no busy block covers. Busy blocks may overlap.
pub fn free_slots(busy: &[(DateTime<Utc>, DateTime<Utc>)], from: DateTime<Utc>, to: DateTime<Utc>, min: Duration) -> Vec<(DateTime<Utc>, DateTime<Utc>)> {
    let min = chrono::Duration::from_std(min).unwrap_or_default();
    let mut blocks: Vec<_> = busy.to_vec();
    blocks.sort();
    let mut out = vec![];
    let mut cursor = from;
    for (b0, b1) in blocks {
        if b0 > cursor {
            let end = b0.min(to);
            if end - cursor >= min {
                out.push((cursor, end));
            }
        }
        cursor = cursor.max(b1);
        if cursor >= to {
            return out;
        }
    }
    if to - cursor >= min {
        out.push((cursor, to));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    #[test]
    fn parse_when_reads_rfc3339_and_local_naive() {
        assert_eq!(parse_when("2026-11-12T18:00:00+05:30").unwrap().to_rfc3339(), "2026-11-12T18:00:00+05:30");
        assert_eq!(parse_when("2026-11-12T18:00:00Z").unwrap().with_timezone(&Utc), utc("2026-11-12T18:00:00Z"));
        let local = Local.with_ymd_and_hms(2026, 11, 12, 18, 0, 0).unwrap().fixed_offset();
        assert_eq!(parse_when("2026-11-12T18:00").unwrap(), local);
        assert_eq!(parse_when("2026-11-12 18:00").unwrap(), local);
        assert_eq!(parse_when(" 2026-11-12T18:00:00 ").unwrap(), local);
        let e = parse_when("tomorrow 6pm").unwrap_err();
        assert!(e.contains("2026-11-12T18:00") || e.contains("like"), "{e}");
    }

    #[test]
    fn free_slots_skips_short_gaps_and_merges_overlaps() {
        let busy = [
            (utc("2026-11-12T04:30:00Z"), utc("2026-11-12T05:00:00Z")),
            (utc("2026-11-12T06:20:00Z"), utc("2026-11-12T07:00:00Z")),
            (utc("2026-11-12T04:45:00Z"), utc("2026-11-12T06:00:00Z")),
        ];
        let free = free_slots(&busy, utc("2026-11-12T04:00:00Z"), utc("2026-11-12T08:00:00Z"), Duration::from_secs(30 * 60));
        assert_eq!(free, vec![(utc("2026-11-12T04:00:00Z"), utc("2026-11-12T04:30:00Z")), (utc("2026-11-12T07:00:00Z"), utc("2026-11-12T08:00:00Z"))]);
        let all = free_slots(&[], utc("2026-11-12T04:00:00Z"), utc("2026-11-12T05:00:00Z"), Duration::from_secs(60));
        assert_eq!(all, vec![(utc("2026-11-12T04:00:00Z"), utc("2026-11-12T05:00:00Z"))]);
        let edge = [(utc("2026-11-12T03:00:00Z"), utc("2026-11-12T09:00:00Z"))];
        assert!(free_slots(&edge, utc("2026-11-12T04:00:00Z"), utc("2026-11-12T08:00:00Z"), Duration::from_secs(60)).is_empty());
    }
}
