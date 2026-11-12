//! Google Calendar connector (build plan 9, Phase 2). Calendar is always `primary`.
pub mod time;

use super::{Connector, ConnectorStatus, Risk, ToolSpec};
use crate::google::GoogleAuth;
use async_trait::async_trait;
use chrono::{DateTime, Local, SecondsFormat, TimeZone, Utc};
use serde::Serialize;
use serde_json::{json, Value};
use std::sync::Arc;

/// One event, as bleep keeps it. `description_untrusted` is text other people wrote.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct CalEvent {
    pub event_id: String,
    pub title: String,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub all_day: bool,
    pub attendees: Vec<String>,
    pub meet_link: Option<String>,
    pub my_response: Option<String>,
    pub html_link: Option<String>,
    pub description_untrusted: Option<String>,
}

pub struct CalendarConnector {
    auth: Arc<GoogleAuth>,
    http: reqwest::Client,
    api: String,
}

impl CalendarConnector {
    pub fn new(auth: Arc<GoogleAuth>, http: reqwest::Client, api: String) -> Arc<CalendarConnector> {
        Arc::new(CalendarConnector { auth, http, api })
    }

    /// Sends with a fresh access token; on 401 drops the token and tries once more.
    async fn send(&self, build: impl Fn() -> reqwest::RequestBuilder) -> Result<Value, String> {
        for attempt in 0..2 {
            let token = self.auth.access_token().await.map_err(|e| e.to_string())?;
            let resp = build().bearer_auth(&token).send().await.map_err(|e| format!("Could not reach Google Calendar: {e}"))?;
            let status = resp.status();
            if status == reqwest::StatusCode::UNAUTHORIZED && attempt == 0 {
                self.auth.invalidate().await;
                continue;
            }
            let text = resp.text().await.unwrap_or_default();
            if status.is_success() {
                return if text.trim().is_empty() { Ok(Value::Null) } else { serde_json::from_str(&text).map_err(|e| format!("Google Calendar sent a reply bleep could not read: {e}")) };
            }
            let msg = serde_json::from_str::<Value>(&text).ok().and_then(|v| v["error"]["message"].as_str().map(String::from)).unwrap_or(text);
            return Err(format!("Google Calendar error {}: {msg}", status.as_u16()));
        }
        Err("Google Calendar refused the sign-in twice.".into())
    }

    fn events_url(&self) -> String {
        format!("{}/calendars/primary/events", self.api)
    }

    async fn get_event(&self, id: &str) -> Result<Value, String> {
        let url = format!("{}/{id}", self.events_url());
        self.send(|| self.http.get(&url)).await
    }

    /// Single events (recurring ones expanded) that overlap `[from, to]`, by start time.
    pub async fn list_window(&self, from: DateTime<Utc>, to: DateTime<Utc>) -> Result<Vec<CalEvent>, String> {
        let url = self.events_url();
        let (t0, t1) = (rfc(from), rfc(to));
        let mut out = vec![];
        let mut page: Option<String> = None;
        for _ in 0..MAX_PAGES {
            let mut q = vec![("singleEvents", "true".to_string()), ("orderBy", "startTime".into()), ("timeMin", t0.clone()), ("timeMax", t1.clone()), ("maxResults", "250".into())];
            if let Some(p) = &page {
                q.push(("pageToken", p.clone()));
            }
            let v = self.send(|| self.http.get(&url).query(&q)).await?;
            out.extend(v["items"].as_array().into_iter().flatten().filter_map(to_event));
            page = v["nextPageToken"].as_str().map(String::from);
            if page.is_none() {
                break;
            }
        }
        Ok(out)
    }

    async fn tool_list(&self, args: &Value) -> Result<Value, String> {
        let today = Local::now().date_naive().and_hms_opt(0, 0, 0).and_then(|n| Local.from_local_datetime(&n).earliest()).map(|d| d.with_timezone(&Utc)).unwrap_or_else(Utc::now);
        let from = match opt_str(args, "start") { Some(s) => time::parse_when(&s)?.with_timezone(&Utc), None => today };
        let to = match opt_str(args, "end") { Some(s) => time::parse_when(&s)?.with_timezone(&Utc), None => from + chrono::Duration::days(1) };
        if to <= from {
            return Err("The end must be after the start.".into());
        }
        let evs = self.list_window(from, to).await?;
        Ok(json!({"events": evs.iter().take(50).map(event_json).collect::<Vec<_>>()}))
    }

    async fn tool_next(&self, args: &Value) -> Result<Value, String> {
        let n = args["count"].as_u64().unwrap_or(3).clamp(1, 10) as usize;
        let now = Utc::now();
        let evs = self.list_window(now, now + chrono::Duration::days(30)).await?;
        Ok(json!({"events": evs.iter().filter(|e| e.end > now).take(n).map(event_json).collect::<Vec<_>>()}))
    }

    async fn tool_free(&self, args: &Value) -> Result<Value, String> {
        let from = time::parse_when(&req_str(args, "start")?)?.with_timezone(&Utc);
        let to = time::parse_when(&req_str(args, "end")?)?.with_timezone(&Utc);
        if to <= from || to - from > chrono::Duration::days(14) {
            return Err("Pick an end after the start, at most 14 days later.".into());
        }
        let min = args["min_minutes"].as_u64().unwrap_or(30).clamp(5, 24 * 60);
        let url = format!("{}/freeBusy", self.api);
        let body = json!({"timeMin": rfc(from), "timeMax": rfc(to), "items": [{"id": "primary"}]});
        let v = self.send(|| self.http.post(&url).json(&body)).await?;
        let busy: Vec<_> = v["calendars"]["primary"]["busy"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|b| Some((utc_of(b["start"].as_str()?)?, utc_of(b["end"].as_str()?)?)))
            .collect();
        let free = time::free_slots(&busy, from, to, std::time::Duration::from_secs(min * 60));
        Ok(json!({"free": free.iter().map(|(a, b)| json!({"start": local(*a), "end": local(*b)})).collect::<Vec<_>>()}))
    }

    fn prepare_create(&self, args: &Value) -> Result<Value, String> {
        let title = req_str(args, "title")?;
        if title.chars().count() > 200 {
            return Err("The title is too long (200 characters at most).".into());
        }
        let all_day = args["all_day"].as_bool().unwrap_or(false);
        let (start, end) = if all_day {
            let s = time::parse_day(&req_str(args, "start")?)?;
            let e = match opt_str(args, "end") { Some(e) => time::parse_day(&e)?, None => s + chrono::Duration::days(1) };
            if e <= s {
                return Err("The end date must be after the start date (the end is not included).".into());
            }
            (s.format("%Y-%m-%d").to_string(), e.format("%Y-%m-%d").to_string())
        } else {
            let s = time::parse_when(&req_str(args, "start")?)?;
            let e = match opt_str(args, "end") { Some(e) => time::parse_when(&e)?, None => s + chrono::Duration::minutes(30) };
            if e <= s {
                return Err("The end time must be after the start time.".into());
            }
            (s.to_rfc3339(), e.to_rfc3339())
        };
        check_lengths(args)?;
        let attendees = emails(args)?;
        let reminder = match &args["reminder_minutes"] {
            Value::Null => Value::Null,
            v => match v.as_u64() {
                Some(m) if m <= 40_320 => json!(m),
                _ => return Err("reminder_minutes must be a whole number from 0 to 40320.".into()),
            },
        };
        Ok(json!({
            "title": title,
            "start": start,
            "end": end,
            "all_day": all_day,
            "location": opt_str(args, "location"),
            "description": opt_str(args, "description"),
            "attendees": attendees,
            "reminder_minutes": reminder,
            "time_zone": tz(),
            "send_invites": !attendees.is_empty(),
        }))
    }

    async fn prepare_update(&self, args: &Value) -> Result<Value, String> {
        let id = event_id(args)?;
        check_lengths(args)?;
        let ev = self.get_event(&id).await?;
        let mut changes = serde_json::Map::new();
        for k in ["title", "location", "description"] {
            if let Some(v) = opt_str(args, k) {
                changes.insert(k.into(), json!(v));
            }
        }
        let new_start = opt_str(args, "start").map(|s| time::parse_when(&s)).transpose()?;
        let new_end = opt_str(args, "end").map(|s| time::parse_when(&s)).transpose()?;
        if new_start.is_some() || new_end.is_some() {
            if ev["start"]["date"].is_string() {
                return Err("This is an all-day event. Change its title, place or notes here, or delete it and add a new one.".into());
            }
            let old_start = ev["start"]["dateTime"].as_str().and_then(|s| DateTime::parse_from_rfc3339(s).ok()).ok_or("The event has no start time.")?;
            let old_end = ev["end"]["dateTime"].as_str().and_then(|s| DateTime::parse_from_rfc3339(s).ok()).ok_or("The event has no end time.")?;
            let start = new_start.unwrap_or(old_start);
            let end = new_end.unwrap_or_else(|| if new_start.is_some() { start + (old_end - old_start) } else { old_end });
            if end <= start {
                return Err("The end time must be after the start time.".into());
            }
            changes.insert("start".into(), json!(start.to_rfc3339()));
            changes.insert("end".into(), json!(end.to_rfc3339()));
        }
        if changes.is_empty() {
            return Err("Nothing to change. Give a new title, time, place or notes.".into());
        }
        Ok(json!({"event_id": id, "before": brief(&ev), "changes": changes, "time_zone": tz(), "notify": attendee_count(&ev) > 0}))
    }

    async fn prepare_delete(&self, args: &Value) -> Result<Value, String> {
        let id = event_id(args)?;
        let ev = self.get_event(&id).await?;
        let mut b = brief(&ev);
        b["attendee_count"] = json!(attendee_count(&ev));
        Ok(json!({"event_id": id, "event": b, "notify": attendee_count(&ev) > 0}))
    }

    async fn prepare_respond(&self, args: &Value) -> Result<Value, String> {
        let response = req_str(args, "response")?;
        if !["accepted", "declined", "tentative"].contains(&response.as_str()) {
            return Err("response must be accepted, declined or tentative.".into());
        }
        let id = event_id(args)?;
        let ev = self.get_event(&id).await?;
        if !ev["attendees"].as_array().into_iter().flatten().any(|a| a["self"] == true) {
            return Err("You are not on the guest list of this event, so there is nothing to reply to.".into());
        }
        let mut b = brief(&ev);
        b.as_object_mut().unwrap().remove("location");
        b["organizer_untrusted"] = json!(ev["organizer"]["displayName"].as_str().or(ev["organizer"]["email"].as_str()));
        Ok(json!({"event_id": id, "event": b, "response": response}))
    }

    async fn execute_create(&self, p: &Value) -> Result<Value, String> {
        let all_day = p["all_day"].as_bool().unwrap_or(false);
        let zone = p["time_zone"].as_str().unwrap_or("UTC");
        let mut body = json!({
            "summary": p["title"],
            "start": when(&p["start"], all_day, zone),
            "end": when(&p["end"], all_day, zone),
            "reminders": match p["reminder_minutes"].as_u64() {
                Some(m) => json!({"useDefault": false, "overrides": [{"method": "popup", "minutes": m}]}),
                None => json!({"useDefault": true}),
            },
        });
        for (k, g) in [("location", "location"), ("description", "description")] {
            if p[k].is_string() {
                body[g] = p[k].clone();
            }
        }
        let att: Vec<Value> = p["attendees"].as_array().into_iter().flatten().map(|e| json!({"email": e})).collect();
        if !att.is_empty() {
            body["attendees"] = json!(att);
        }
        let url = self.events_url();
        let q = [("sendUpdates", if att.is_empty() { "none" } else { "all" })];
        let v = self.send(|| self.http.post(&url).query(&q).json(&body)).await?;
        Ok(json!({"id": v["id"], "html_link": v["htmlLink"], "title": p["title"], "start": p["start"], "end": p["end"]}))
    }

    async fn execute_update(&self, p: &Value) -> Result<Value, String> {
        let id = event_id(p)?;
        let zone = p["time_zone"].as_str().unwrap_or("UTC");
        let c = &p["changes"];
        let mut body = json!({});
        for (k, g) in [("title", "summary"), ("location", "location"), ("description", "description")] {
            if c[k].is_string() {
                body[g] = c[k].clone();
            }
        }
        for k in ["start", "end"] {
            if c[k].is_string() {
                body[k] = when(&c[k], false, zone);
            }
        }
        let url = format!("{}/{id}", self.events_url());
        let q = [("sendUpdates", notify(p))];
        let v = self.send(|| self.http.patch(&url).query(&q).json(&body)).await?;
        Ok(json!({"id": v["id"], "updated": true}))
    }

    async fn execute_delete(&self, p: &Value) -> Result<Value, String> {
        let id = event_id(p)?;
        let url = format!("{}/{id}", self.events_url());
        let q = [("sendUpdates", notify(p))];
        self.send(|| self.http.delete(&url).query(&q)).await?;
        Ok(json!({"deleted": true}))
    }

    async fn execute_respond(&self, p: &Value) -> Result<Value, String> {
        let id = event_id(p)?;
        let response = p["response"].as_str().ok_or("No response given.")?;
        let ev = self.get_event(&id).await?;
        let mut attendees = ev["attendees"].as_array().cloned().unwrap_or_default();
        let me = attendees.iter_mut().find(|a| a["self"] == true).ok_or("You are no longer on the guest list of this event.")?;
        me["responseStatus"] = json!(response);
        let url = format!("{}/{id}", self.events_url());
        let body = json!({"attendees": attendees});
        let q = [("sendUpdates", "all")];
        self.send(|| self.http.patch(&url).query(&q).json(&body)).await?;
        Ok(json!({"responded": response}))
    }
}

const MAX_PAGES: usize = 5;

fn rfc(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn local(t: DateTime<Utc>) -> String {
    t.with_timezone(&Local).to_rfc3339_opts(SecondsFormat::Secs, false)
}

fn utc_of(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s).ok().map(|d| d.with_timezone(&Utc))
}

fn tz() -> String {
    iana_time_zone::get_timezone().unwrap_or_else(|_| "UTC".into())
}

fn notify(p: &Value) -> &'static str {
    if p["notify"].as_bool().unwrap_or(false) { "all" } else { "none" }
}

fn when(v: &Value, all_day: bool, zone: &str) -> Value {
    if all_day { json!({"date": v}) } else { json!({"dateTime": v, "timeZone": zone}) }
}

fn opt_str(args: &Value, k: &str) -> Option<String> {
    args[k].as_str().map(str::trim).filter(|s| !s.is_empty()).map(String::from)
}

fn req_str(args: &Value, k: &str) -> Result<String, String> {
    opt_str(args, k).ok_or_else(|| format!("{k} is required."))
}

/// Google event ids are letters, digits, `_` and `-`. Anything else never reaches a URL.
fn event_id(args: &Value) -> Result<String, String> {
    let id = req_str(args, "event_id")?;
    if id.len() > 1024 || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return Err("That is not a calendar event id.".into());
    }
    Ok(id)
}

/// Cards show text in full, so it has to be short enough to read.
fn check_lengths(args: &Value) -> Result<(), String> {
    if opt_str(args, "description").is_some_and(|d| d.chars().count() > 2000) {
        return Err("The notes are too long (2000 characters at most).".into());
    }
    if opt_str(args, "location").is_some_and(|l| l.chars().count() > 200) {
        return Err("The place is too long (200 characters at most).".into());
    }
    Ok(())
}

fn emails(args: &Value) -> Result<Vec<String>, String> {
    let list = match &args["attendees"] {
        Value::Null => return Ok(vec![]),
        Value::Array(a) => a,
        _ => return Err("attendees must be a list of email addresses.".into()),
    };
    list.iter()
        .map(|v| {
            let e = v.as_str().map(str::trim).unwrap_or_default();
            let ok = e.len() <= 254 && !e.contains(char::is_whitespace) && matches!(e.split_once('@'), Some((l, d)) if !l.is_empty() && d.contains('.') && !d.starts_with('.') && !d.ends_with('.') && !d.contains('@'));
            if ok { Ok(e.to_lowercase()) } else { Err(format!("\"{e}\" is not an email address.")) }
        })
        .collect()
}

fn attendee_count(ev: &Value) -> usize {
    ev["attendees"].as_array().map_or(0, Vec::len)
}

/// What a card shows about an existing event.
fn brief(ev: &Value) -> Value {
    let start = ev["start"]["dateTime"].as_str().or(ev["start"]["date"].as_str());
    let end = ev["end"]["dateTime"].as_str().or(ev["end"]["date"].as_str());
    json!({"title": ev["summary"].as_str().unwrap_or("(No title)"), "start": start, "end": end, "location": ev["location"]})
}

fn day_start(s: &str) -> Option<DateTime<Utc>> {
    let d = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()?;
    Local.from_local_datetime(&d.and_hms_opt(0, 0, 0)?).earliest().map(|t| t.with_timezone(&Utc))
}

fn to_event(v: &Value) -> Option<CalEvent> {
    if v["status"] == "cancelled" {
        return None;
    }
    let all_day = v["start"]["date"].is_string();
    let (start, end) = if all_day {
        (day_start(v["start"]["date"].as_str()?)?, day_start(v["end"]["date"].as_str()?)?)
    } else {
        (utc_of(v["start"]["dateTime"].as_str()?)?, utc_of(v["end"]["dateTime"].as_str()?)?)
    };
    let attendees = v["attendees"].as_array().into_iter().flatten();
    let meet_link = v["hangoutLink"].as_str().map(String::from).or_else(|| {
        v["conferenceData"]["entryPoints"].as_array()?.iter().find(|e| e["entryPointType"] == "video")?["uri"].as_str().map(String::from)
    });
    Some(CalEvent {
        event_id: v["id"].as_str()?.to_string(),
        title: v["summary"].as_str().unwrap_or("(No title)").to_string(),
        start,
        end,
        all_day,
        attendees: attendees.clone().filter_map(|a| a["email"].as_str().map(String::from)).collect(),
        meet_link,
        my_response: attendees.clone().find(|a| a["self"] == true).and_then(|a| a["responseStatus"].as_str().map(String::from)),
        html_link: v["htmlLink"].as_str().map(String::from),
        description_untrusted: v["description"].as_str().map(|d| d.chars().take(500).collect()),
    })
}

/// How the model sees an event: local times, untrusted text labelled.
pub fn event_json(e: &CalEvent) -> Value {
    json!({
        "event_id": e.event_id,
        "title": e.title,
        "start": local(e.start),
        "end": local(e.end),
        "all_day": e.all_day,
        "attendees": e.attendees,
        "meet_link": e.meet_link,
        "my_response": e.my_response,
        "description_untrusted": e.description_untrusted,
    })
}

#[async_trait]
impl Connector for CalendarConnector {
    fn id(&self) -> &'static str {
        "calendar"
    }

    fn tools(&self) -> Vec<ToolSpec> {
        let when = json!({"type": "string", "description": "Date and time, like 2026-11-12T18:00 (this Mac's time zone) or with an offset."});
        let id = json!({"type": "string", "description": "event_id from calendar_list or calendar_next."});
        vec![
            ToolSpec { name: "calendar_list", description: "List events on the user's calendar between two times. Defaults to today.", schema: json!({"type": "object", "properties": {"start": when, "end": when}}), risk: Risk::Read },
            ToolSpec { name: "calendar_next", description: "The next events on the user's calendar.", schema: json!({"type": "object", "properties": {"count": {"type": "integer", "minimum": 1, "maximum": 10}}}), risk: Risk::Read },
            ToolSpec { name: "calendar_free", description: "Free time on the user's calendar between two times (at most 14 days apart).", schema: json!({"type": "object", "properties": {"start": when, "end": when, "min_minutes": {"type": "integer", "minimum": 5}}, "required": ["start", "end"]}), risk: Risk::Read },
            ToolSpec { name: "calendar_create", description: "Add an event or reminder to the user's calendar. The user sees a card and must approve. End defaults to 30 minutes after start. For all-day events set all_day and give dates.", schema: json!({"type": "object", "properties": {"title": {"type": "string"}, "start": when, "end": when, "all_day": {"type": "boolean"}, "location": {"type": "string"}, "description": {"type": "string"}, "attendees": {"type": "array", "items": {"type": "string"}, "description": "Email addresses. They get an invite."}, "reminder_minutes": {"type": "integer", "minimum": 0}}, "required": ["title", "start"]}), risk: Risk::Write },
            ToolSpec { name: "calendar_update", description: "Change an event's title, time, place or notes. The user approves a card first. A new start without an end keeps the length.", schema: json!({"type": "object", "properties": {"event_id": id, "title": {"type": "string"}, "start": when, "end": when, "location": {"type": "string"}, "description": {"type": "string"}}, "required": ["event_id"]}), risk: Risk::Write },
            ToolSpec { name: "calendar_delete", description: "Delete an event. The user approves a card first.", schema: json!({"type": "object", "properties": {"event_id": id}, "required": ["event_id"]}), risk: Risk::Write },
            ToolSpec { name: "calendar_respond", description: "Accept, decline or say maybe to an invite. The user approves a card first.", schema: json!({"type": "object", "properties": {"event_id": id, "response": {"type": "string", "enum": ["accepted", "declined", "tentative"]}}, "required": ["event_id", "response"]}), risk: Risk::Write },
        ]
    }

    fn status(&self) -> ConnectorStatus {
        self.auth.status()
    }

    async fn call(&self, tool: &str, args: Value) -> Result<Value, String> {
        match tool {
            "calendar_list" => self.tool_list(&args).await,
            "calendar_next" => self.tool_next(&args).await,
            "calendar_free" => self.tool_free(&args).await,
            _ => Err(format!("Unknown tool: {tool}")),
        }
    }

    async fn prepare(&self, tool: &str, args: Value) -> Result<Value, String> {
        // No card for a change bleep cannot make.
        if !matches!(self.auth.status(), ConnectorStatus::Connected { .. }) {
            return Err(crate::google::NOT_CONNECTED.into());
        }
        match tool {
            "calendar_create" => self.prepare_create(&args),
            "calendar_update" => self.prepare_update(&args).await,
            "calendar_delete" => self.prepare_delete(&args).await,
            "calendar_respond" => self.prepare_respond(&args).await,
            _ => Err(format!("Unknown tool: {tool}")),
        }
    }

    async fn execute(&self, tool: &str, payload: Value) -> Result<Value, String> {
        match tool {
            "calendar_create" => self.execute_create(&payload).await,
            "calendar_update" => self.execute_update(&payload).await,
            "calendar_delete" => self.execute_delete(&payload).await,
            "calendar_respond" => self.execute_respond(&payload).await,
            _ => Err(format!("Unknown tool: {tool}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::GoogleConfig;
    use crate::google::mock::{fixture, MockGoogle};
    use crate::google::secrets::{MemorySecrets, Secrets};
    use crate::google::{NOT_CONNECTED, REFRESH_KEY};
    use crate::opener::RecordingOpener;

    const EVENTS: &str = "GET /calendar/v3/calendars/primary/events";
    const EV1: &str = "GET /calendar/v3/calendars/primary/events/ev1";

    struct Rig {
        mock: MockGoogle,
        cal: Arc<CalendarConnector>,
    }

    async fn rig(signed_in: bool) -> Rig {
        let mock = MockGoogle::start().await;
        mock.on("POST /token", vec![(200, fixture("token_refresh.json"))]);
        let secrets = Arc::new(MemorySecrets::default());
        if signed_in {
            secrets.set(REFRESH_KEY, "rt-1").unwrap();
        }
        let cfg = GoogleConfig { client_id: "cid".into(), client_secret: "cs".into() };
        let auth = GoogleAuth::new(cfg, mock.endpoints(), secrets, Arc::new(RecordingOpener::default()));
        let cal = CalendarConnector::new(auth, reqwest::Client::new(), format!("{}/calendar/v3", mock.base));
        Rig { mock, cal }
    }

    fn utc(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    fn body(h: &crate::google::mock::Hit) -> Value {
        serde_json::from_str(&h.body).unwrap()
    }

    #[tokio::test]
    async fn list_maps_meet_link_all_day_declined_and_offsets() {
        let r = rig(true).await;
        r.mock.on(EVENTS, vec![(200, fixture("events_list.json"))]);
        let evs = r.cal.list_window(utc("2026-11-12T00:00:00Z"), utc("2026-11-14T00:00:00Z")).await.unwrap();
        assert_eq!(evs.iter().map(|e| e.event_id.as_str()).collect::<Vec<_>>(), vec!["ev1", "hol1", "ev2"]);
        let dr = &evs[0];
        assert_eq!((dr.start, dr.end), (utc("2026-11-12T09:30:00Z"), utc("2026-11-12T10:15:00Z")));
        assert_eq!(dr.meet_link.as_deref(), Some("https://meet.google.com/abc-defg-hij"));
        assert_eq!(dr.my_response.as_deref(), Some("accepted"));
        assert!(dr.attendees.contains(&"priya@example.com".to_string()));
        assert!(dr.description_untrusted.as_deref().unwrap().starts_with("Agenda"));
        assert!(!dr.all_day);
        assert!(evs[1].all_day);
        let vs = &evs[2];
        assert_eq!(vs.start, utc("2026-11-12T17:30:00Z"));
        assert_eq!(vs.my_response.as_deref(), Some("declined"));
        assert_eq!(vs.meet_link.as_deref(), Some("https://zoom.us/j/123"));
        // Tool output carries the same events, with the untrusted text labelled.
        let out = r.cal.call("calendar_list", json!({"start": "2026-11-12T00:00:00Z", "end": "2026-11-14T00:00:00Z"})).await.unwrap();
        assert_eq!(out["events"].as_array().unwrap().len(), 3);
        assert!(out["events"][0]["description_untrusted"].is_string());
        assert!(out["events"][0].get("description").is_none());
    }

    #[tokio::test]
    async fn list_window_uses_single_events_and_order_by_start() {
        let r = rig(true).await;
        r.mock.on(EVENTS, vec![(200, fixture("events_list.json"))]);
        r.cal.list_window(utc("2026-11-12T00:00:00Z"), utc("2026-11-14T00:00:00Z")).await.unwrap();
        let h = &r.mock.hits_for(EVENTS)[0];
        for part in ["singleEvents=true", "orderBy=startTime", "timeMin=2026-11-12T00%3A00%3A00Z", "timeMax=2026-11-14T00%3A00%3A00Z"] {
            assert!(h.query.contains(part), "{part} missing in {}", h.query);
        }
        assert_eq!(h.auth.as_deref(), Some("Bearer at-refresh"));
    }

    #[tokio::test]
    async fn create_prepare_fills_end_and_time_zone() {
        let r = rig(true).await;
        let p = r.cal.prepare("calendar_create", json!({"title": " Pay rent ", "start": "2026-11-12T18:00:00+05:30"})).await.unwrap();
        assert_eq!(p["title"], "Pay rent");
        assert_eq!(p["start"], "2026-11-12T18:00:00+05:30");
        assert_eq!(p["end"], "2026-11-12T18:30:00+05:30");
        assert_eq!(p["all_day"], false);
        assert_eq!(p["send_invites"], false);
        assert_eq!(p["attendees"], json!([]));
        assert_eq!(p["time_zone"], iana_time_zone::get_timezone().unwrap());
        assert!(r.mock.hits.lock().unwrap().is_empty(), "prepare must not call Google for a create");
    }

    #[tokio::test]
    async fn create_prepare_rejects_end_before_start_and_bad_email() {
        let r = rig(true).await;
        let e = r.cal.prepare("calendar_create", json!({"title": "x", "start": "2026-11-12T18:00:00Z", "end": "2026-11-12T17:00:00Z"})).await.unwrap_err();
        assert!(e.contains("end"), "{e}");
        let e = r.cal.prepare("calendar_create", json!({"title": "x", "start": "2026-11-12T18:00:00Z", "attendees": ["not-an-email"]})).await.unwrap_err();
        assert!(e.contains("email"), "{e}");
        assert!(r.cal.prepare("calendar_create", json!({"title": "  ", "start": "2026-11-12T18:00:00Z"})).await.is_err());
        assert!(r.cal.prepare("calendar_create", json!({"start": "2026-11-12T18:00:00Z"})).await.is_err());
    }

    #[tokio::test]
    async fn create_execute_posts_exact_payload() {
        let r = rig(true).await;
        let created = json!({"id": "new1", "htmlLink": "https://calendar.google.com/x", "summary": "Pay rent", "start": {"dateTime": "2026-11-12T18:00:00+05:30"}, "end": {"dateTime": "2026-11-12T18:30:00+05:30"}});
        r.mock.on("POST /calendar/v3/calendars/primary/events", vec![(200, created.to_string())]);
        let p = r.cal.prepare("calendar_create", json!({"title": "Pay rent", "start": "2026-11-12T18:00:00+05:30", "reminder_minutes": 15})).await.unwrap();
        let out = r.cal.execute("calendar_create", p.clone()).await.unwrap();
        assert_eq!(out["id"], "new1");
        let h = &r.mock.hits_for("POST /calendar/v3/calendars/primary/events")[0];
        assert!(h.query.contains("sendUpdates=none"), "{}", h.query);
        let b = body(h);
        assert_eq!(b["summary"], "Pay rent");
        assert_eq!(b["start"]["dateTime"], p["start"]);
        assert_eq!(b["end"]["dateTime"], p["end"]);
        assert_eq!(b["start"]["timeZone"], p["time_zone"]);
        assert_eq!(b["reminders"], json!({"useDefault": false, "overrides": [{"method": "popup", "minutes": 15}]}));
        assert!(b.get("attendees").is_none());

        let p2 = r.cal.prepare("calendar_create", json!({"title": "Lunch", "start": "2026-11-12T13:00:00+05:30", "attendees": ["priya@example.com"]})).await.unwrap();
        assert_eq!(p2["send_invites"], true);
        r.cal.execute("calendar_create", p2).await.unwrap();
        let h2 = &r.mock.hits_for("POST /calendar/v3/calendars/primary/events")[1];
        assert!(h2.query.contains("sendUpdates=all"), "{}", h2.query);
        assert_eq!(body(h2)["attendees"], json!([{"email": "priya@example.com"}]));
        assert_eq!(body(h2)["reminders"], json!({"useDefault": true}));
    }

    #[tokio::test]
    async fn all_day_create_uses_dates() {
        let r = rig(true).await;
        r.mock.on("POST /calendar/v3/calendars/primary/events", vec![(200, json!({"id": "d1"}).to_string())]);
        let p = r.cal.prepare("calendar_create", json!({"title": "Rent due", "start": "2026-11-15", "all_day": true})).await.unwrap();
        assert_eq!((p["start"].as_str(), p["end"].as_str()), (Some("2026-11-15"), Some("2026-11-16")));
        r.cal.execute("calendar_create", p).await.unwrap();
        let b = body(&r.mock.hits_for("POST /calendar/v3/calendars/primary/events")[0]);
        assert_eq!((b["start"].clone(), b["end"].clone()), (json!({"date": "2026-11-15"}), json!({"date": "2026-11-16"})));
    }

    #[tokio::test]
    async fn update_prepare_shows_before_and_keeps_duration() {
        let r = rig(true).await;
        r.mock.on(EV1, vec![(200, fixture("event_get.json"))]);
        let p = r.cal.prepare("calendar_update", json!({"event_id": "ev1", "start": "2026-11-12T16:00:00+05:30"})).await.unwrap();
        assert_eq!(p["event_id"], "ev1");
        assert_eq!(p["before"]["title"], "Design review");
        assert_eq!(p["before"]["start"], "2026-11-12T15:00:00+05:30");
        assert_eq!(p["changes"]["start"], "2026-11-12T16:00:00+05:30");
        assert_eq!(p["changes"]["end"], "2026-11-12T16:45:00+05:30");
        assert_eq!(p["notify"], true);
        let e = r.cal.prepare("calendar_update", json!({"event_id": "ev1"})).await.unwrap_err();
        assert!(e.contains("Nothing to change"), "{e}");
    }

    #[tokio::test]
    async fn update_execute_patches_only_changes() {
        let r = rig(true).await;
        r.mock.on("PATCH /calendar/v3/calendars/primary/events/ev1", vec![(200, fixture("event_get.json"))]);
        let payload = json!({"event_id": "ev1", "before": {"title": "Design review"}, "changes": {"title": "Design review v2"}, "time_zone": "Asia/Kolkata", "notify": true});
        r.cal.execute("calendar_update", payload).await.unwrap();
        let h = &r.mock.hits_for("PATCH /calendar/v3/calendars/primary/events/ev1")[0];
        assert_eq!(body(h), json!({"summary": "Design review v2"}));
        assert!(h.query.contains("sendUpdates=all"));
    }

    #[tokio::test]
    async fn delete_prepare_shows_event_and_execute_deletes() {
        let r = rig(true).await;
        r.mock.on(EV1, vec![(200, fixture("event_get.json"))]);
        r.mock.on("DELETE /calendar/v3/calendars/primary/events/ev1", vec![(204, String::new())]);
        let p = r.cal.prepare("calendar_delete", json!({"event_id": "ev1"})).await.unwrap();
        assert_eq!((p["event"]["title"].as_str(), p["event"]["attendee_count"].as_u64(), p["notify"].as_bool()), (Some("Design review"), Some(3), Some(true)));
        assert_eq!(r.cal.execute("calendar_delete", p).await.unwrap(), json!({"deleted": true}));
        assert!(r.mock.hits_for("DELETE /calendar/v3/calendars/primary/events/ev1")[0].query.contains("sendUpdates=all"));
    }

    #[tokio::test]
    async fn respond_execute_sets_only_my_response() {
        let r = rig(true).await;
        r.mock.on(EV1, vec![(200, fixture("event_get.json"))]);
        r.mock.on("PATCH /calendar/v3/calendars/primary/events/ev1", vec![(200, fixture("event_get.json"))]);
        let p = r.cal.prepare("calendar_respond", json!({"event_id": "ev1", "response": "declined"})).await.unwrap();
        assert_eq!(p["event"]["organizer_untrusted"], "Priya");
        assert_eq!(p["response"], "declined");
        r.cal.execute("calendar_respond", p).await.unwrap();
        let b = body(&r.mock.hits_for("PATCH /calendar/v3/calendars/primary/events/ev1")[0]);
        let att = b["attendees"].as_array().unwrap();
        assert_eq!(att.len(), 3);
        assert_eq!(att[0], json!({"email": "me@example.com", "self": true, "responseStatus": "declined"}));
        assert_eq!(att[1]["responseStatus"], "accepted");
        assert_eq!(att[2], json!({"email": "arjun@example.com", "responseStatus": "needsAction", "comment": "maybe"}));
        assert!(r.cal.prepare("calendar_respond", json!({"event_id": "ev1", "response": "maybe"})).await.is_err());
    }

    #[tokio::test]
    async fn respond_prepare_refuses_events_i_am_not_invited_to() {
        let r = rig(true).await;
        let solo = json!({"id": "ev9", "summary": "Focus", "start": {"dateTime": "2026-11-12T10:00:00Z"}, "end": {"dateTime": "2026-11-12T11:00:00Z"}});
        r.mock.on("GET /calendar/v3/calendars/primary/events/ev9", vec![(200, solo.to_string())]);
        let e = r.cal.prepare("calendar_respond", json!({"event_id": "ev9", "response": "accepted"})).await.unwrap_err();
        assert!(e.contains("guest list"), "{e}");
    }

    #[tokio::test]
    async fn bad_event_id_is_refused() {
        let r = rig(true).await;
        assert!(r.cal.prepare("calendar_delete", json!({"event_id": "../../users/me"})).await.is_err());
        assert!(r.mock.hits.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn free_reports_gaps() {
        let r = rig(true).await;
        r.mock.on("POST /calendar/v3/freeBusy", vec![(200, fixture("freebusy.json"))]);
        let out = r.cal.call("calendar_free", json!({"start": "2026-11-12T04:00:00Z", "end": "2026-11-12T08:00:00Z"})).await.unwrap();
        assert_eq!(out["free"].as_array().unwrap().len(), 2);
        let b = body(&r.mock.hits_for("POST /calendar/v3/freeBusy")[0]);
        assert_eq!(b["items"], json!([{"id": "primary"}]));
    }

    #[tokio::test]
    async fn unauthorized_once_refreshes_and_retries() {
        let r = rig(true).await;
        r.mock.on(EVENTS, vec![(401, json!({"error": {"message": "Invalid Credentials"}}).to_string()), (200, fixture("events_list.json"))]);
        assert_eq!(r.cal.list_window(utc("2026-11-12T00:00:00Z"), utc("2026-11-14T00:00:00Z")).await.unwrap().len(), 3);
        assert_eq!(r.mock.hits_for("POST /token").len(), 2);
        assert_eq!(r.mock.hits_for(EVENTS).len(), 2);
    }

    #[tokio::test]
    async fn google_errors_are_plain() {
        let r = rig(true).await;
        r.mock.on(EVENTS, vec![(403, json!({"error": {"message": "Calendar API has not been used in project 1"}}).to_string())]);
        let e = r.cal.call("calendar_next", json!({})).await.unwrap_err();
        assert!(e.starts_with("Google Calendar error 403: Calendar API has not been used"), "{e}");
    }

    #[tokio::test]
    async fn create_prepare_caps_long_fields() {
        let r = rig(true).await;
        let long = "x".repeat(2001);
        assert!(r.cal.prepare("calendar_create", json!({"title": "a", "start": "2026-11-12T18:00:00Z", "description": long})).await.unwrap_err().contains("notes"));
        let place = "y".repeat(201);
        assert!(r.cal.prepare("calendar_create", json!({"title": "a", "start": "2026-11-12T18:00:00Z", "location": place})).await.unwrap_err().contains("place"));
        assert!(r.cal.prepare("calendar_update", json!({"event_id": "ev1", "description": "z".repeat(2001)})).await.unwrap_err().contains("notes"));
    }

    #[tokio::test]
    async fn not_connected_tools_say_so() {
        let r = rig(false).await;
        // No card for a change bleep cannot make.
        assert_eq!(r.cal.prepare("calendar_create", json!({"title": "Pay rent", "start": "2026-11-12T18:00:00Z"})).await, Err(NOT_CONNECTED.to_string()));
        assert_eq!(r.cal.call("calendar_list", json!({})).await, Err(NOT_CONNECTED.to_string()));
        assert_eq!(r.cal.prepare("calendar_delete", json!({"event_id": "ev1"})).await, Err(NOT_CONNECTED.to_string()));
    }

    #[test]
    fn tools_have_the_planned_names_and_risks() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let r = rt.block_on(rig(false));
        let got: Vec<(&str, Risk)> = r.cal.tools().iter().map(|t| (t.name, t.risk)).collect();
        assert_eq!(
            got,
            vec![
                ("calendar_list", Risk::Read),
                ("calendar_next", Risk::Read),
                ("calendar_free", Risk::Read),
                ("calendar_create", Risk::Write),
                ("calendar_update", Risk::Write),
                ("calendar_delete", Risk::Write),
                ("calendar_respond", Risk::Write),
            ]
        );
    }
}
