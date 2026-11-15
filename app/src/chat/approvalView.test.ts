import { describe, expect, it } from "vitest";
import { approvalView } from "./approvalView";
import type { Approval } from "../ipc";

const TZ = "Asia/Kolkata";
const card = (tool: string, payload: Record<string, unknown>, status: Approval["status"] = "pending", result: string | null = null): Approval => ({
  id: 1, tool, payload, status, turn_message_id: 2, result, created_at: "", decided_at: null,
});
const create = (extra: Record<string, unknown> = {}) =>
  card("calendar_create", {
    title: "Pay rent", start: "2026-11-12T18:00:00+05:30", end: "2026-11-12T18:30:00+05:30", all_day: false,
    location: null, description: null, attendees: [], reminder_minutes: null, time_zone: TZ, send_invites: false, ...extra,
  });

describe("approvalView", () => {
  it("remote tool card names the tool and server and shows every argument", () => {
    const payload = { server: "dayblocks", tool: "move_task", arguments: { id: "t1", day: "2026-11-16", tags: ["home"] } };
    const v = approvalView(card("dayblocks_move_task", payload));
    expect(v.heading).toBe("Run move_task on dayblocks?");
    expect(v.rows).toEqual([["id", "t1"], ["day", "2026-11-16"], ["tags", '["home"]']]);
    expect(v.details).toBe(JSON.stringify(payload.arguments, null, 2));
    expect(v.warn).toBe("A server you added asks to run this. bleep cannot check what it does.");
    expect(approvalView(card("dayblocks_move_task", payload, "done")).doneText).toBe("Done");
  });

  it("remote tool card with no arguments says so", () => {
    const v = approvalView(card("wiki_ask", { server: "wiki", tool: "ask", arguments: {} }));
    expect(v.rows).toEqual([["Arguments", "None"]]);
  });

  it("memory card shows the exact fact and where it goes", () => {
    const v = approvalView(card("memory_promote", { fact: "Sync means my 1:1 with Priya", file: "MEMORY.md" }));
    expect(v.heading).toBe("Remember this?");
    expect(v.rows).toEqual([["Fact", "Sync means my 1:1 with Priya"], ["Saved in", "MEMORY.md"]]);
    expect(approvalView(card("memory_promote", { fact: "x", file: "MEMORY.md" }, "done")).doneText).toBe("Saved");
  });

  it("create card shows title, day and time range", () => {
    const v = approvalView(create(), TZ);
    expect(v.heading).toBe("Add to your calendar?");
    expect(v.rows).toContainEqual(["What", "Pay rent"]);
    expect(v.rows).toContainEqual(["When", "Thu 12 Nov, 6:00 PM to 6:30 PM"]);
    expect(v.warn).toBeNull();
    expect(v.doneText).toBeNull();
  });

  it("all-day create shows the day only", () => {
    const v = approvalView(create({ all_day: true, start: "2026-11-15", end: "2026-11-16" }), TZ);
    expect(v.rows).toContainEqual(["When", "Sun 15 Nov, all day"]);
  });

  it("create with attendees warns about invites", () => {
    const v = approvalView(create({ attendees: ["priya@example.com", "arjun@example.com"], send_invites: true, reminder_minutes: 15 }), TZ);
    expect(v.warn).toBe("Invites go to: priya@example.com, arjun@example.com");
    expect(v.rows).toContainEqual(["Reminder", "15 min before"]);
  });

  it("update card shows before and after", () => {
    const v = approvalView(card("calendar_update", {
      event_id: "ev1",
      before: { title: "Design review", start: "2026-11-12T15:00:00+05:30", end: "2026-11-12T15:45:00+05:30", location: null },
      changes: { start: "2026-11-12T16:00:00+05:30", end: "2026-11-12T16:45:00+05:30" },
      time_zone: TZ, notify: true,
    }), TZ);
    expect(v.heading).toBe("Change this event?");
    expect(v.rows).toContainEqual(["Event", "Design review"]);
    expect(v.rows).toContainEqual(["Time", "3:00 PM to 3:45 PM → 4:00 PM to 4:45 PM"]);
    expect(v.warn).toBe("Guests will be told.");
  });

  it("update card shows an end-only change", () => {
    const v = approvalView(card("calendar_update", {
      event_id: "ev1",
      before: { title: "Sync", start: "2026-11-12T15:00:00+05:30", end: "2026-11-12T16:00:00+05:30", location: null },
      changes: { start: "2026-11-12T15:00:00+05:30", end: "2026-11-12T15:30:00+05:30" },
      time_zone: TZ, notify: false,
    }), TZ);
    expect(v.rows).toContainEqual(["Time", "3:00 PM to 4:00 PM → 3:00 PM to 3:30 PM"]);
  });

  it("long notes are shown in full, never cut", () => {
    const notes = "n".repeat(600);
    const v = approvalView(create({ description: notes }), TZ);
    expect(v.rows).toContainEqual(["Notes", notes]);
  });

  it("delete and respond cards", () => {
    const ev = { title: "Design review", start: "2026-11-12T15:00:00+05:30", end: "2026-11-12T15:45:00+05:30" };
    const d = approvalView(card("calendar_delete", { event_id: "ev1", event: { ...ev, attendee_count: 3 }, notify: true }), TZ);
    expect(d.heading).toBe("Delete this event?");
    expect(d.rows).toContainEqual(["When", "Thu 12 Nov, 3:00 PM to 3:45 PM"]);
    const r = approvalView(card("calendar_respond", { event_id: "ev1", event: { ...ev, organizer_untrusted: "Priya" }, response: "tentative" }), TZ);
    expect(r.heading).toBe("Reply to this invite?");
    expect(r.rows).toContainEqual(["Reply", "Maybe"]);
    expect(r.rows).toContainEqual(["From", "Priya"]);
  });

  it("send card shows the exact email and warns about new recipients", () => {
    const body = "Hi Priya,\n\nLooks good. Ship it.\n\nAviroop";
    const v = approvalView(card("gmail_send", {
      to: ["priya@example.com"], cc: ["stranger@other.example"], subject: "Re: Deck review", body,
      thread_id: "t2", in_reply_to: "<deck-2@example.com>", references: null, new_recipients: ["stranger@other.example"],
    }), TZ);
    expect(v.heading).toBe("Send this email?");
    expect(v.rows).toContainEqual(["To", "priya@example.com"]);
    expect(v.rows).toContainEqual(["Cc", "stranger@other.example"]);
    expect(v.rows).toContainEqual(["Subject", "Re: Deck review"]);
    expect(v.rows).toContainEqual(["Body", body]);
    expect(v.warn).toBe("New recipient: stranger@other.example");
    expect(approvalView({ ...card("gmail_send", { to: ["a@b.co"], subject: "x", body: "y", new_recipients: [] }), status: "done" }, TZ).doneText).toBe("Sent");
    expect(approvalView(card("gmail_send", { to: ["a@b.co"], subject: "x", body: "y", new_recipients: [] }), TZ).warn).toBeNull();
    expect(approvalView(card("gmail_send", { to: ["a@b.co", "c@d.co"], subject: "x", body: "y", new_recipients: ["a@b.co", "c@d.co"] }), TZ).warn).toBe("New recipients: a@b.co, c@d.co");
  });

  it("done text follows status", () => {
    expect(approvalView(create(), TZ).doneText).toBeNull();
    expect(approvalView({ ...create(), status: "approved" }, TZ).doneText).toBe("Working…");
    expect(approvalView({ ...create(), status: "done" }, TZ).doneText).toBe("Added");
    expect(approvalView({ ...create(), status: "declined" }, TZ).doneText).toBe("Declined");
    expect(approvalView({ ...create(), status: "expired" }, TZ).doneText).toBe("Expired, nothing changed");
    expect(approvalView({ ...create(), status: "failed", result: "Google Calendar error 403" }, TZ).doneText).toBe("Failed: Google Calendar error 403");
    expect(approvalView(card("calendar_delete", { event: {} }, "done"), TZ).doneText).toBe("Deleted");
    expect(approvalView(card("calendar_respond", { event: {}, response: "accepted" }, "done"), TZ).doneText).toBe("Sent");
  });

  it("no em dash in any heading or row", () => {
    const all = [approvalView(create({ attendees: ["a@b.co"] }), TZ), approvalView(card("unknown_tool", { a: 1 }), TZ)];
    for (const v of all) expect(JSON.stringify(v)).not.toMatch(/—/);
  });
});
