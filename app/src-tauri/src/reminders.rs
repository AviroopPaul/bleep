//! Native reminders (0.5.0, F5): the clock that fires them, bleep's reminder tools, and the
//! commands behind the Reminders list and the bubble buttons.
//!
//! A reminder I set myself is not a proactive ping: it fires in quiet hours and while paused,
//! and it does not use the ping budget.
use crate::connectors::{Connector, ConnectorStatus, Risk, ToolSpec};
use crate::proactive::scheduler::Clock;
use crate::store::reminders::{next_due, Reminder, Repeat};
use crate::store::Store;
use async_trait::async_trait;
use chrono::{DateTime, Duration, NaiveDateTime, TimeZone, Utc};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::Notify;

/// A reminder this late still fires, marked late. Later than this, it only goes to the chat.
pub const LATE_LIMIT: Duration = Duration::hours(2);
/// Under this, a reminder is on time.
const ON_TIME: Duration = Duration::minutes(2);
/// The clock re-checks at least this often, so sleep and clock changes are caught.
const MAX_WAIT: std::time::Duration = std::time::Duration::from_secs(20);

#[derive(Clone, Debug, PartialEq)]
pub enum Fire {
    /// Show it: bubble, notification and a chat line.
    Now { late: bool },
    /// Missed by more than `LATE_LIMIT` (the Mac slept or bleep was closed): a chat line only.
    Missed,
}

pub type FireFn = Arc<dyn Fn(&Reminder, Fire) + Send + Sync>;
pub type ChangedFn = Arc<dyn Fn() + Send + Sync>;

pub struct Reminders {
    pub store: Arc<Store>,
    fire: FireFn,
    changed: ChangedFn,
    now: Clock,
    wake: Notify,
}

impl Reminders {
    pub fn new(store: Arc<Store>, fire: FireFn, changed: ChangedFn, now: Clock) -> Arc<Reminders> {
        Arc::new(Reminders { store, fire, changed, now, wake: Notify::new() })
    }

    pub fn now(&self) -> DateTime<Utc> {
        (self.now)()
    }

    /// After any change: wake the clock and refresh the list.
    pub fn touched(&self) {
        self.wake.notify_one();
        (self.changed)();
    }

    /// Fires everything due. Returns what fired, for tests.
    pub fn tick(&self) -> Vec<(i64, Fire)> {
        self.tick_in(&chrono::Local)
    }

    pub fn tick_in<Tz: TimeZone>(&self, tz: &Tz) -> Vec<(i64, Fire)> {
        let now = self.now();
        let mut out = vec![];
        for r in self.store.scheduled_reminders().unwrap_or_default() {
            let at = r.next_at();
            if at > now {
                break;
            }
            let late_by = now - at;
            let how = if late_by > LATE_LIMIT { Fire::Missed } else { Fire::Now { late: late_by > ON_TIME } };
            // A snoozed repeat keeps its next time; an unsnoozed one moves on.
            let next = r.repeat.map(|rep| if r.snoozed_until.is_some_and(|s| s < r.due_at) { r.due_at } else { next_due(r.due_at, rep, now, tz) });
            if let Err(e) = self.store.mark_reminder_fired(r.id, now, next) {
                eprintln!("bleep: reminder {}: {e}", r.id);
                continue;
            }
            (self.fire)(&r, how.clone());
            out.push((r.id, how));
        }
        if !out.is_empty() {
            (self.changed)();
        }
        out
    }

    pub async fn run(self: Arc<Self>) {
        loop {
            self.tick();
            let wait = self
                .store
                .scheduled_reminders()
                .ok()
                .and_then(|v| v.first().map(|r| r.next_at()))
                .map(|t| (t - self.now()).to_std().unwrap_or_default())
                .unwrap_or(MAX_WAIT)
                .clamp(std::time::Duration::from_millis(200), MAX_WAIT);
            tokio::select! {
                _ = tokio::time::sleep(wait) => {}
                _ = self.wake.notified() => {}
            }
        }
    }

    pub fn done(&self, id: i64) -> Result<(), String> {
        self.store.reminder_done(id).map_err(|e| e.to_string())?;
        self.touched();
        Ok(())
    }

    pub fn snooze(&self, id: i64, minutes: i64) -> Result<(), String> {
        if !(1..=7 * 24 * 60).contains(&minutes) {
            return Err("Snooze between 1 minute and 7 days.".into());
        }
        self.store.snooze_reminder(id, self.now() + Duration::minutes(minutes)).map_err(|e| e.to_string())?;
        self.touched();
        Ok(())
    }
}

/// When a reminder is due: `at` is a local time ("2026-11-17T19:00", seconds and an offset
/// allowed), or `in_minutes` from now. It must be in the future.
pub fn parse_when<Tz: TimeZone>(at: Option<&str>, in_minutes: Option<i64>, now: DateTime<Utc>, tz: &Tz) -> Result<DateTime<Utc>, String> {
    let t = match (at.map(str::trim).filter(|s| !s.is_empty()), in_minutes) {
        (_, Some(m)) if m >= 1 => now + Duration::minutes(m),
        (_, Some(_)) => return Err("in_minutes must be 1 or more.".into()),
        (Some(s), None) => {
            if let Ok(d) = DateTime::parse_from_rfc3339(s) {
                d.with_timezone(&Utc)
            } else {
                let n = ["%Y-%m-%dT%H:%M:%S", "%Y-%m-%dT%H:%M", "%Y-%m-%d %H:%M"]
                    .iter()
                    .find_map(|f| NaiveDateTime::parse_from_str(s, f).ok())
                    .ok_or_else(|| format!("Could not read the time {s:?}. Use local time like 2026-11-17T19:00."))?;
                tz.from_local_datetime(&n).earliest().ok_or("That local time does not exist (clock change).")?.with_timezone(&Utc)
            }
        }
        (None, None) => return Err("Give `at` (local time) or `in_minutes`.".into()),
    };
    if t <= now {
        return Err("That time has passed. Pick a time in the future.".into());
    }
    Ok(t)
}

/// What the model and the list see: the time in local terms.
pub fn reminder_json(r: &Reminder) -> Value {
    let local = |t: DateTime<Utc>| t.with_timezone(&chrono::Local).format("%Y-%m-%dT%H:%M").to_string();
    json!({
        "id": r.id,
        "text": r.text,
        "due": local(r.due_at),
        "next": local(r.next_at()),
        "repeat": r.repeat.map(|x| x.as_str()).unwrap_or("none"),
        "status": r.status.as_str(),
        "snoozed": r.snoozed_until.is_some(),
    })
}

/// bleep's reminder tools. All local and easy to undo, so none asks first.
pub struct ReminderConnector(pub Arc<Reminders>);

fn repeat_arg(args: &Value) -> Result<Option<Repeat>, String> {
    Repeat::from_arg(args.get("repeat").and_then(Value::as_str).unwrap_or(""))
}

fn text_arg(args: &Value) -> Result<String, String> {
    let t = args.get("text").and_then(Value::as_str).unwrap_or("").trim().to_string();
    match t.chars().count() {
        0 => Err("text is empty.".into()),
        n if n > 200 => Err("text is over 200 characters.".into()),
        _ => Ok(t),
    }
}

#[async_trait]
impl Connector for ReminderConnector {
    fn id(&self) -> &str {
        "reminders"
    }

    fn tools(&self) -> Vec<ToolSpec> {
        let when = json!({
            "at": {"type": "string", "description": "Local time on this Mac, like 2026-11-17T19:00. Call bleep_now first for today's date."},
            "in_minutes": {"type": "integer", "minimum": 1, "description": "Minutes from now. Use instead of `at` for \"in 20 minutes\"."},
            "repeat": {"type": "string", "enum": ["none", "daily", "weekdays", "weekly", "monthly"]},
        });
        let mut create = json!({"type": "object", "properties": {"text": {"type": "string", "description": "What to remind me about, short."}}, "required": ["text"]});
        create["properties"].as_object_mut().unwrap().extend(when.as_object().unwrap().clone());
        let mut update = json!({"type": "object", "properties": {"id": {"type": "integer"}, "text": {"type": "string"}}, "required": ["id"]});
        update["properties"].as_object_mut().unwrap().extend(when.as_object().unwrap().clone());
        vec![
            ToolSpec {
                name: "reminder_create".into(),
                description: "Set a reminder on this Mac. At the time, bleep shows it over the droid and as a macOS notification. Use this for \"remind me to...\" and for bill reminders. Use calendar_create only when I ask for a calendar event.".into(),
                schema: create,
                risk: Risk::Safe,
            },
            ToolSpec {
                name: "reminder_list".into(),
                description: "My reminders: due ones waiting for Done, then upcoming ones by time. Times are local.".into(),
                schema: json!({"type": "object", "properties": {"include_done": {"type": "boolean", "description": "Also the last 10 done ones."}}}),
                risk: Risk::Read,
            },
            ToolSpec {
                name: "reminder_update".into(),
                description: "Change a reminder's text, time or repeat. Give only what changes. It is scheduled again.".into(),
                schema: update,
                risk: Risk::Safe,
            },
            ToolSpec {
                name: "reminder_delete".into(),
                description: "Delete a reminder by id.".into(),
                schema: json!({"type": "object", "properties": {"id": {"type": "integer"}}, "required": ["id"]}),
                risk: Risk::Safe,
            },
        ]
    }

    fn status(&self) -> ConnectorStatus {
        ConnectorStatus::Connected { account: "this Mac".into() }
    }

    async fn call(&self, tool: &str, args: Value) -> Result<Value, String> {
        let r = &self.0;
        let now = r.now();
        let id = || args.get("id").and_then(Value::as_i64).ok_or("id is required.");
        let out = match tool {
            "reminder_create" => {
                let text = text_arg(&args)?;
                let due = parse_when(args.get("at").and_then(Value::as_str), args.get("in_minutes").and_then(Value::as_i64), now, &chrono::Local)?;
                let rem = r.store.add_reminder(&text, due, repeat_arg(&args)?, "chat").map_err(|e| e.to_string())?;
                reminder_json(&rem)
            }
            "reminder_list" => {
                let done = if args.get("include_done").and_then(Value::as_bool).unwrap_or(false) { 10 } else { 0 };
                let list = r.store.reminders(done).map_err(|e| e.to_string())?;
                return Ok(json!({"reminders": list.iter().map(reminder_json).collect::<Vec<_>>()}));
            }
            "reminder_update" => {
                let old = r.store.reminder(id()?).map_err(|e| e.to_string())?.ok_or("No reminder with that id.")?;
                let text = if args.get("text").is_some() { text_arg(&args)? } else { old.text.clone() };
                let (at, mins) = (args.get("at").and_then(Value::as_str), args.get("in_minutes").and_then(Value::as_i64));
                let due = if at.is_some() || mins.is_some() { parse_when(at, mins, now, &chrono::Local)? } else { old.due_at };
                let repeat = if args.get("repeat").is_some() { repeat_arg(&args)? } else { old.repeat };
                r.store.update_reminder(old.id, &text, due, repeat).map_err(|e| e.to_string())?;
                reminder_json(&r.store.reminder(old.id).map_err(|e| e.to_string())?.ok_or("gone")?)
            }
            "reminder_delete" => {
                if !r.store.delete_reminder(id()?).map_err(|e| e.to_string())? {
                    return Err("No reminder with that id.".into());
                }
                json!({"deleted": true})
            }
            _ => return Err(format!("Unknown tool: {tool}")),
        };
        r.touched();
        Ok(out)
    }
}

// ---- Commands for the Reminders list and the bubble ----

use tauri::State;

#[tauri::command]
pub fn reminders_list(r: State<'_, Arc<Reminders>>) -> Result<Vec<Value>, String> {
    Ok(r.store.reminders(10).map_err(|e| e.to_string())?.iter().map(reminder_json).collect())
}

/// From the quick-add field: `at` is the local time from a date-time input.
#[tauri::command]
pub fn reminder_add(r: State<'_, Arc<Reminders>>, text: String, at: String, repeat: Option<String>) -> Result<Value, String> {
    let args = json!({"text": text});
    let text = text_arg(&args)?;
    let due = parse_when(Some(&at), None, r.now(), &chrono::Local)?;
    let rem = r.store.add_reminder(&text, due, Repeat::from_arg(repeat.as_deref().unwrap_or(""))?, "user").map_err(|e| e.to_string())?;
    r.touched();
    Ok(reminder_json(&rem))
}

#[tauri::command]
pub fn reminder_done(app: tauri::AppHandle, r: State<'_, Arc<Reminders>>, id: i64) -> Result<(), String> {
    crate::bar::hide_bubble(&app);
    r.done(id)
}

#[tauri::command]
pub fn reminder_snooze(app: tauri::AppHandle, r: State<'_, Arc<Reminders>>, id: i64, minutes: i64) -> Result<(), String> {
    crate::bar::hide_bubble(&app);
    r.snooze(id, minutes)
}

#[tauri::command]
pub fn reminder_delete(r: State<'_, Arc<Reminders>>, id: i64) -> Result<(), String> {
    r.store.delete_reminder(id).map_err(|e| e.to_string())?;
    r.touched();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::reminders::ReminderStatus;
    use crate::store::tests::utc;
    use chrono::FixedOffset;
    use std::sync::Mutex;

    fn ist() -> FixedOffset {
        FixedOffset::east_opt(5 * 3600 + 1800).unwrap()
    }

    struct Rig {
        _d: tempfile::TempDir,
        r: Arc<Reminders>,
        now: Arc<Mutex<DateTime<Utc>>>,
        fired: Arc<Mutex<Vec<(String, Fire)>>>,
    }

    fn rig(start: &str) -> Rig {
        let d = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::open(&d.path().join("b.db")).unwrap());
        let now = Arc::new(Mutex::new(utc(start)));
        let fired = Arc::new(Mutex::new(vec![]));
        let (n, f) = (now.clone(), fired.clone());
        let r = Reminders::new(store, Arc::new(move |rem: &Reminder, how| f.lock().unwrap().push((rem.text.clone(), how))), Arc::new(|| {}), Arc::new(move || *n.lock().unwrap()));
        Rig { _d: d, r, now, fired }
    }

    impl Rig {
        fn at(&self, t: &str) -> Vec<(i64, Fire)> {
            *self.now.lock().unwrap() = utc(t);
            self.r.tick_in(&ist())
        }
    }

    #[test]
    fn fires_on_time_once() {
        let g = rig("2026-11-17T13:00:00Z");
        g.r.store.add_reminder("Call mom", utc("2026-11-17T13:30:00Z"), None, "chat").unwrap();
        assert!(g.at("2026-11-17T13:29:59Z").is_empty());
        assert_eq!(g.at("2026-11-17T13:30:10Z").len(), 1);
        assert!(g.at("2026-11-17T13:31:00Z").is_empty(), "fires once");
        assert_eq!(g.fired.lock().unwrap()[0], ("Call mom".into(), Fire::Now { late: false }));
        assert_eq!(g.r.store.reminders(0).unwrap()[0].status, ReminderStatus::Fired);
    }

    #[test]
    fn late_and_missed_after_sleep() {
        let g = rig("2026-11-17T10:00:00Z");
        g.r.store.add_reminder("late one", utc("2026-11-17T11:00:00Z"), None, "chat").unwrap();
        g.r.store.add_reminder("missed one", utc("2026-11-17T08:30:00Z"), None, "chat").unwrap();
        g.at("2026-11-17T11:30:00Z");
        let f = g.fired.lock().unwrap().clone();
        assert_eq!(f, vec![("missed one".into(), Fire::Missed), ("late one".into(), Fire::Now { late: true })]);
    }

    #[test]
    fn daily_repeat_fires_again_next_day() {
        let g = rig("2026-11-17T03:00:00Z");
        g.r.store.add_reminder("Stretch", utc("2026-11-17T03:30:00Z"), Some(Repeat::Daily), "user").unwrap();
        assert_eq!(g.at("2026-11-17T03:30:00Z").len(), 1);
        let r = &g.r.store.scheduled_reminders().unwrap()[0];
        assert_eq!(r.due_at, utc("2026-11-18T03:30:00Z"));
        g.r.snooze(r.id, 10).unwrap();
        assert_eq!(g.at("2026-11-17T03:40:00Z").len(), 1, "the snooze fires");
        assert_eq!(g.r.store.scheduled_reminders().unwrap()[0].due_at, utc("2026-11-18T03:30:00Z"), "and keeps tomorrow");
        assert_eq!(g.at("2026-11-18T03:30:00Z").len(), 1);
    }

    #[test]
    fn snooze_brings_a_fired_one_back() {
        let g = rig("2026-11-17T13:00:00Z");
        let rem = g.r.store.add_reminder("Oven", utc("2026-11-17T13:05:00Z"), None, "user").unwrap();
        g.at("2026-11-17T13:05:00Z");
        g.r.snooze(rem.id, 10).unwrap();
        assert!(g.at("2026-11-17T13:14:00Z").is_empty());
        assert_eq!(g.at("2026-11-17T13:15:00Z").len(), 1);
        assert!(g.r.snooze(rem.id, 0).is_err());
    }

    #[test]
    fn parse_when_reads_local_time_and_minutes() {
        let now = utc("2026-11-17T10:00:00Z");
        assert_eq!(parse_when(Some("2026-11-17T19:00"), None, now, &ist()).unwrap(), utc("2026-11-17T13:30:00Z"));
        assert_eq!(parse_when(Some("2026-11-17T19:00:00+05:30"), None, now, &ist()).unwrap(), utc("2026-11-17T13:30:00Z"));
        assert_eq!(parse_when(None, Some(20), now, &ist()).unwrap(), utc("2026-11-17T10:20:00Z"));
        assert!(parse_when(Some("2026-11-17T09:00"), None, now, &ist()).is_err(), "past");
        assert!(parse_when(Some("tomorrow"), None, now, &ist()).is_err());
        assert!(parse_when(None, None, now, &ist()).is_err());
        assert!(parse_when(None, Some(0), now, &ist()).is_err());
    }

    #[tokio::test]
    async fn tools_create_list_update_delete() {
        let g = rig("2026-11-17T10:00:00Z");
        let c = ReminderConnector(g.r.clone());
        assert!(c.tools().iter().all(|t| t.risk != Risk::Write), "reminders never need a card");
        let made = c.call("reminder_create", json!({"text": "Pay rent", "in_minutes": 30, "repeat": "monthly"})).await.unwrap();
        let id = made["id"].as_i64().unwrap();
        assert_eq!(made["repeat"], "monthly");
        let list = c.call("reminder_list", json!({})).await.unwrap();
        assert_eq!(list["reminders"].as_array().unwrap().len(), 1);
        let up = c.call("reminder_update", json!({"id": id, "text": "Pay rent now"})).await.unwrap();
        assert_eq!((up["text"].as_str(), up["repeat"].as_str()), (Some("Pay rent now"), Some("monthly")));
        assert!(c.call("reminder_create", json!({"text": "  "})).await.is_err());
        assert!(c.call("reminder_create", json!({"text": "x"})).await.is_err(), "needs a time");
        c.call("reminder_delete", json!({"id": id})).await.unwrap();
        assert!(c.call("reminder_delete", json!({"id": id})).await.is_err());
    }
}
