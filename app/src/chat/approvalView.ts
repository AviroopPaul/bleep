import type { Approval } from "../ipc";

export interface CardView {
  heading: string;
  rows: [string, string][];
  warn: string | null;
  doneText: string | null;
}

type P = Record<string, any>;

const clean = (s: string) => s.replace(/ /g, " ");
const isDate = (s: string) => /^\d{4}-\d{2}-\d{2}$/.test(s);

function day(s: string, timeZone?: string): string {
  // A bare date is a calendar day, not an instant: format it in UTC so it never shifts.
  const d = isDate(s) ? new Date(`${s}T00:00:00Z`) : new Date(s);
  return clean(new Intl.DateTimeFormat("en-GB", { weekday: "short", day: "numeric", month: "short", timeZone: isDate(s) ? "UTC" : timeZone }).format(d));
}

function clock(s: string, timeZone?: string): string {
  return clean(new Intl.DateTimeFormat("en-US", { hour: "numeric", minute: "2-digit", timeZone }).format(new Date(s)));
}

function range(start?: string, end?: string, allDay = false, tz?: string): string {
  if (!start) return "";
  if (allDay || isDate(start)) return `${day(start, tz)}, all day`;
  if (!end) return `${day(start, tz)}, ${clock(start, tz)}`;
  const same = day(start, tz) === day(end, tz);
  return same ? `${day(start, tz)}, ${clock(start, tz)} to ${clock(end, tz)}` : `${day(start, tz)}, ${clock(start, tz)} to ${day(end, tz)}, ${clock(end, tz)}`;
}

/** "3:00 PM to 3:45 PM", with the end day when it ends on another day. */
function span(start: string, end: string | undefined, tz?: string): string {
  if (!end) return clock(start, tz);
  return day(start, tz) === day(end, tz) ? `${clock(start, tz)} to ${clock(end, tz)}` : `${clock(start, tz)} to ${day(end, tz)}, ${clock(end, tz)}`;
}

/** Old and new start and end, so any change the card runs is visible. Days show when the day moves. */
function moved(b: P, c: P, tz?: string): string {
  const s0: string | undefined = b.start;
  const s1: string | undefined = c.start ?? s0;
  const e1: string | undefined = c.end ?? b.end;
  if (!s0 || !s1) return "";
  const withDay = day(s0, tz) !== day(s1, tz);
  const side = (s: string, e?: string) => (withDay ? `${day(s, tz)}, ${span(s, e, tz)}` : span(s, e, tz));
  return `${side(s0, b.end)} → ${side(s1, e1)}`;
}

const cut = (s: string, n = 140) => (s.length > n ? `${s.slice(0, n)}…` : s);
const REPLY: Record<string, string> = { accepted: "Yes", declined: "No", tentative: "Maybe" };
const DONE: Record<string, string> = { calendar_create: "Added", calendar_update: "Changed", calendar_delete: "Deleted", calendar_respond: "Sent", gmail_send: "Sent" };

function doneText(a: Approval): string | null {
  switch (a.status) {
    case "pending": return null;
    case "approved": return "Working…";
    case "done": return DONE[a.tool] ?? "Done";
    case "declined": return "Declined";
    case "expired": return "Expired, nothing changed";
    case "failed": return `Failed: ${a.result ?? "unknown error"}`;
  }
}

export function approvalView(a: Approval, timeZone?: string): CardView {
  const p = (a.payload ?? {}) as P;
  const rows: [string, string][] = [];
  let heading: string;
  let warn: string | null = null;
  switch (a.tool) {
    case "calendar_create": {
      heading = "Add to your calendar?";
      rows.push(["What", String(p.title ?? "")], ["When", range(p.start, p.end, !!p.all_day, timeZone)]);
      if (p.location) rows.push(["Where", String(p.location)]);
      // Never cut: the card shows exactly what runs.
      if (p.description) rows.push(["Notes", String(p.description)]);
      if (p.reminder_minutes != null) rows.push(["Reminder", `${p.reminder_minutes} min before`]);
      const att: string[] = p.attendees ?? [];
      if (att.length) warn = `Invites go to: ${att.join(", ")}`;
      break;
    }
    case "calendar_update": {
      heading = "Change this event?";
      const b: P = p.before ?? {};
      const c: P = p.changes ?? {};
      rows.push(["Event", String(b.title ?? "")]);
      if (c.title) rows.push(["Title", `${b.title ?? ""} → ${c.title}`]);
      if (c.start || c.end) rows.push(["Time", moved(b, c, timeZone)]);
      if (c.location) rows.push(["Where", `${b.location ?? "none"} → ${c.location}`]);
      if (c.description) rows.push(["Notes", String(c.description)]);
      if (p.notify) warn = "Guests will be told.";
      break;
    }
    case "calendar_delete": {
      heading = "Delete this event?";
      const e: P = p.event ?? {};
      rows.push(["What", String(e.title ?? "")], ["When", range(e.start, e.end, false, timeZone)]);
      if (p.notify) warn = "Guests will be told.";
      break;
    }
    case "calendar_respond": {
      heading = "Reply to this invite?";
      const e: P = p.event ?? {};
      rows.push(["Event", String(e.title ?? "")], ["When", range(e.start, e.end, false, timeZone)]);
      if (e.organizer_untrusted) rows.push(["From", String(e.organizer_untrusted)]);
      rows.push(["Reply", REPLY[p.response] ?? String(p.response ?? "")]);
      break;
    }
    case "gmail_send": {
      heading = "Send this email?";
      const list = (k: string): string[] => (Array.isArray(p[k]) ? p[k] : []);
      rows.push(["To", list("to").join(", ")]);
      if (list("cc").length) rows.push(["Cc", list("cc").join(", ")]);
      rows.push(["Subject", String(p.subject ?? "")], ["Body", String(p.body ?? "")]);
      const fresh = list("new_recipients");
      if (fresh.length) warn = `${fresh.length === 1 ? "New recipient" : "New recipients"}: ${fresh.join(", ")}`;
      break;
    }
    default:
      heading = `Allow ${a.tool}?`;
      for (const [k, v] of Object.entries(p)) rows.push([k, cut(typeof v === "string" ? v : JSON.stringify(v))]);
  }
  return { heading, rows: rows.filter(([, v]) => v !== ""), warn, doneText: doneText(a) };
}
