// Fake backend for headless UI checks (Playwright). Loaded only with ?mock=1.
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import type { Approval, BarCue, ChatEmit, GoogleState, Message } from "./ipc";

const row = (id: number, role: Message["role"], content: string): Message => ({
  id, role, kind: "chat", content, tool_steps: [], tokens: null, status: "done", session_id: 1, created_at: "",
});

export function installMocks(label: string) {
  mockWindows(label);
  const history: Message[] = [row(1, "user", "hello"), row(2, "assistant", "Hi! I am bleep.")];
  let next = 3;
  const w = window as unknown as { __decideCalls: number };
  w.__decideCalls = 0;
  let card: Approval | null = null;
  const q = new URLSearchParams(window.location.search);
  const calls: string[] = [];
  (window as unknown as { __calls: string[] }).__calls = calls;
  const cueKind = q.get("cue");
  const mins = cueKind === "ten" ? 8 : 42;
  const cue: BarCue | null = cueKind
    ? {
        key: "m1@x:hour", kind: cueKind === "ten" ? "ten_min" : "hour", event_id: "m1",
        title: cueKind === "long" ? "Quarterly planning with the platform, design, data and growth teams plus partners from the vendor side, room 4B" : "Design review",
        start: new Date(Date.now() + mins * 60_000).toISOString(), end: new Date(Date.now() + (mins + 30) * 60_000).toISOString(),
        meet_link: cueKind === "nolink" ? null : "https://meet.google.com/abc-defg-hij", attendees: ["priya@example.com"],
      }
    : null;
  const gq = q.get("g");
  // setup: no app at all. builtin: bleep's app. own/ready/expired/connected: the user's own app.
  const own = !!gq && !["setup", "builtin"].includes(gq);
  let google: GoogleState = {
    client_id: own ? "cid.apps.googleusercontent.com" : "",
    has_secret: own,
    status: gq === "connected" ? { state: "connected", account: "me@example.com" } : gq && gq !== "setup" ? { state: "needs_sign_in" } : { state: "needs_setup" },
    expired: gq === "expired",
    builtin: gq !== "setup",
    using_own: own,
  };
  const bubble = q.get("bubble");
  if (bubble) setTimeout(() => emit("bubble://text", { text: bubble }), 200);
  const later = (ms: number, f: () => void) => setTimeout(f, ms);
  const chatEv = (id: number, event: ChatEmit["event"]) => emit("chat://event", { message_id: id, event });
  mockIPC(
    (cmd, args) => {
      const a = (args ?? {}) as Record<string, unknown>;
      switch (cmd) {
        case "chat_history":
          return a.beforeId ? [] : history;
        case "runtime_health":
          return { runtime: "claude", installed: true, version: "2.1.319", version_ok: true, signed_in: true, plan: "pro", hint: null };
        case "chat_send": {
          if (a.text === "fail please") throw "bleep could not reach Claude Code: spawn failed";
          next += 2;
          const id = next;
          if (String(a.text).includes("markdown")) {
            const md = "**Done.** Here is your day:\n\n- Standup at 10:00\n- Design review at 3:00\n\nI used `calendar_list`. See [the agenda](https://example.com/agenda).\n\n<img src=x onerror=alert(1)>";
            later(100, () => chatEv(id, { type: "text_delta", text: md }));
            later(200, () => chatEv(id, { type: "done", runtime_session_id: "s1" }));
            return id;
          }
          if (String(a.text).includes("rent")) {
            card = {
              id: 50, tool: "calendar_create", status: "pending", turn_message_id: id, result: null, created_at: "", decided_at: null,
              payload: { title: "Pay rent", start: "2026-11-12T18:00:00+05:30", end: "2026-11-12T18:30:00+05:30", all_day: false, location: null, description: null, attendees: [], reminder_minutes: null, time_zone: "Asia/Kolkata", send_invites: false },
            };
            const c = card;
            later(100, () => chatEv(id, { type: "tool_start", id: "t2", name: "calendar_create", args: { title: "Pay rent" } }));
            later(200, () => emit("approval://changed", c));
            return id;
          }
          const steps: ChatEmit["event"][] = [
            { type: "tool_start", id: "t1", name: "bleep_now", args: {} },
            { type: "tool_result", id: "t1", ok: true, preview: "{}" },
            { type: "text_delta", text: "It is 8:05 PM in Tokyo." },
            { type: "usage", input: 1200, output: 14 },
            { type: "done", runtime_session_id: "s1" },
          ];
          steps.forEach((event, i) => setTimeout(() => emit("chat://event", { message_id: id, event }), 150 * (i + 1)));
          return id;
        }
        case "google_state":
          return google;
        case "google_save_client":
          calls.push(`google_save_client:${a.clientId}`);
          google = { ...google, client_id: String(a.clientId), has_secret: true, using_own: true, status: { state: "needs_sign_in" } };
          return google;
        case "google_use_builtin":
          calls.push("google_use_builtin");
          google = { ...google, client_id: "", has_secret: false, using_own: false, status: { state: "needs_sign_in" } };
          return google;
        case "google_connect":
          return new Promise((ok, fail) =>
            setTimeout(() => {
              if (q.has("fail")) return fail("Sign-in was cancelled.");
              google = { ...google, status: { state: "connected", account: "me@example.com" }, expired: false };
              void emit("google://status", google.status);
              ok("me@example.com");
            }, 400),
          );
        case "google_disconnect":
          google = { ...google, status: { state: "needs_sign_in" } };
          return null;
        case "open_link":
          calls.push(`open_link:${a.which}`);
          return null;
        case "open_url":
          calls.push(`open_url:${a.url}`);
          return null;
        case "bar_current":
          return cue;
        case "bar_prep":
        case "bar_snooze":
        case "bar_join":
        case "bar_dismiss":
        case "bubble_click":
          calls.push(cmd);
          return null;
        case "approval_decide": {
          w.__decideCalls += 1;
          if (!card || card.status !== "pending" || a.id !== card.id) throw "This card is no longer waiting.";
          const yes = a.approve === true;
          const id = card.turn_message_id!;
          card = { ...card, status: yes ? "approved" : "declined" };
          const c1 = card;
          later(30, () => emit("approval://changed", c1));
          if (yes) {
            const c2 = { ...card, status: "done" as const, result: "{\"id\":\"new1\"}" };
            card = c2;
            later(120, () => emit("approval://changed", c2));
            later(160, () => chatEv(id, { type: "tool_result", id: "t2", ok: true, preview: "{\"id\":\"new1\"}" }));
            later(200, () => chatEv(id, { type: "text_delta", text: "Done. Pay rent is on your calendar at 6 PM." }));
          } else {
            later(160, () => chatEv(id, { type: "tool_result", id: "t2", ok: false, preview: "Declined by the user. Nothing was changed." }));
            later(200, () => chatEv(id, { type: "text_delta", text: "Okay, I did not add it." }));
          }
          later(260, () => chatEv(id, { type: "done", runtime_session_id: "s1" }));
          return null;
        }
        default:
          return null;
      }
    },
    { shouldMockEvents: true },
  );
}
