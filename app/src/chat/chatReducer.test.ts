import { describe, expect, it } from "vitest";
import { chatReducer, emptyChat, formatBackAt, stepLabel, type ChatState } from "./chatReducer";
import type { Approval, Message } from "../ipc";

const user = (id: number, content: string): Message => ({
  id, role: "user", kind: "chat", content, tool_steps: [], tokens: null, status: "done", session_id: 1, created_at: "",
});

function sent(): ChatState {
  return chatReducer(emptyChat, { type: "sent", user: user(1, "time?"), assistantId: 2 });
}

describe("chatReducer", () => {
  it("a fresh history drops the old context (after compaction)", () => {
    let s = chatReducer(sent(), { type: "event", messageId: 2, event: { type: "context", used: 250_000, window: 1_000_000 } });
    s = chatReducer(s, { type: "history", messages: [], mode: "replace" });
    expect(s.context).toBeNull();
  });

  it("context event keeps the message and sets the context", () => {
    let s = sent();
    s = chatReducer(s, { type: "event", messageId: 2, event: { type: "text_delta", text: "hi" } });
    s = chatReducer(s, { type: "event", messageId: 2, event: { type: "context", used: 42_000, window: 1_000_000 } });
    expect(s.messages[1].content).toBe("hi");
    expect(s.context).toEqual({ used: 42_000, window: 1_000_000 });
    expect(stepLabel("memory_promote")).toBe("Remembering a fact");
    expect(stepLabel("memory_search")).toBe("Searched your memory");
  });

  it("stop keeps the text, marks it interrupted and frees the input", () => {
    let s = chatReducer(sent(), { type: "event", messageId: 2, event: { type: "text_delta", text: "Half" } });
    s = chatReducer(s, { type: "stopped", messageId: 2 });
    expect(s.messages[1]).toMatchObject({ content: "Half", status: "interrupted" });
    expect(s.streamingId).toBeNull();
  });

  it("text deltas append to the streaming message", () => {
    let s = sent();
    expect(s.streamingId).toBe(2);
    s = chatReducer(s, { type: "event", messageId: 2, event: { type: "text_delta", text: "It is " } });
    s = chatReducer(s, { type: "event", messageId: 2, event: { type: "text_delta", text: "noon." } });
    expect(s.messages.map((m) => m.content)).toEqual(["time?", "It is noon."]);
    expect(s.messages[1].status).toBe("streaming");
  });

  it("tool start then result marks the step ok", () => {
    let s = sent();
    s = chatReducer(s, { type: "event", messageId: 2, event: { type: "tool_start", id: "t1", name: "bleep_now", args: {} } });
    expect(s.messages[1].tool_steps[0].ok).toBeNull();
    s = chatReducer(s, { type: "event", messageId: 2, event: { type: "tool_result", id: "t1", ok: true, preview: "{}" } });
    expect(s.messages[1].tool_steps[0]).toMatchObject({ name: "bleep_now", ok: true });
  });

  it("done clears streamingId", () => {
    let s = sent();
    s = chatReducer(s, { type: "event", messageId: 2, event: { type: "done", runtime_session_id: "s1" } });
    expect(s.streamingId).toBeNull();
    expect(s.messages[1].status).toBe("done");
  });

  it("error keeps partial text and marks error", () => {
    let s = sent();
    s = chatReducer(s, { type: "event", messageId: 2, event: { type: "text_delta", text: "half" } });
    s = chatReducer(s, { type: "event", messageId: 2, event: { type: "error", message: "lost", retryable: true } });
    s = chatReducer(s, { type: "event", messageId: 2, event: { type: "done", runtime_session_id: null } });
    expect(s.messages[1].content).toBe("half\nlost");
    expect(s.messages[1].status).toBe("error");
    expect(s.streamingId).toBeNull();
  });

  it("rate limited sets the back-at time", () => {
    let s = sent();
    s = chatReducer(s, { type: "event", messageId: 2, event: { type: "rate_limited", until: 1794493800 } });
    expect(s.rateLimitedUntil).toBe(1794493800);
    expect(formatBackAt(1794493800, "Asia/Kolkata")).toBe("Limit reached, back at 8:00 PM");
    expect(formatBackAt(1794493800, "Asia/Kolkata")).not.toMatch(/—/);
  });

  it("a turn that finishes fine clears the limit note, an error keeps it", () => {
    let s = chatReducer(sent(), { type: "event", messageId: 2, event: { type: "rate_limited", until: 1794493800 } });
    s = chatReducer(s, { type: "event", messageId: 2, event: { type: "done", runtime_session_id: null } });
    expect(s.rateLimitedUntil).toBeNull();
    let e = chatReducer(sent(), { type: "event", messageId: 2, event: { type: "rate_limited", until: 1794493800 } });
    e = chatReducer(e, { type: "event", messageId: 2, event: { type: "error", message: "limit", retryable: false } });
    e = chatReducer(e, { type: "event", messageId: 2, event: { type: "done", runtime_session_id: null } });
    expect(e.rateLimitedUntil).toBe(1794493800);
  });

  it("a ping lands in the log in id order", () => {
    const ping: Message = { ...user(3, "Standup in 10 min"), role: "system", kind: "ping" };
    const s = chatReducer(sent(), { type: "ping", message: ping });
    expect(s.messages.map((m) => [m.id, m.kind])).toEqual([[1, "chat"], [2, "chat"], [3, "ping"]]);
  });

  it("events that arrive before sent create the row, and sent does not duplicate it", () => {
    let s = chatReducer(emptyChat, { type: "event", messageId: 2, event: { type: "text_delta", text: "hi" } });
    s = chatReducer(s, { type: "sent", user: user(1, "hello"), assistantId: 2 });
    expect(s.messages.map((m) => [m.id, m.content])).toEqual([[1, "hello"], [2, "hi"]]);
  });

  it("history replace and prepend keep id order without duplicates", () => {
    let s = chatReducer(emptyChat, { type: "history", messages: [user(5, "e"), user(6, "f")], mode: "replace" });
    s = chatReducer(s, { type: "history", messages: [user(3, "c"), user(4, "d"), user(5, "e")], mode: "prepend" });
    expect(s.messages.map((m) => m.id)).toEqual([3, 4, 5, 6]);
  });
});

describe("stepLabel", () => {
  it("names bleep_now in plain words and falls back to the tool name", () => {
    expect(stepLabel("bleep_now")).toBe("Checked the time");
    expect(stepLabel("dayblocks_tasks")).toBe("dayblocks_tasks");
  });
});

describe("approvals", () => {
  const ap = (id: number, turn: number | null, status: Approval["status"] = "pending"): Approval => ({
    id, tool: "calendar_create", payload: { title: "Pay rent" }, status, turn_message_id: turn, result: null, created_at: "", decided_at: null,
  });

  it("approval attaches to its turn message", () => {
    const s = chatReducer(sent(), { type: "approval", approval: ap(9, 2) });
    expect(s.messages[1].approvals?.map((a) => a.id)).toEqual([9]);
    expect(s.loose).toEqual([]);
  });

  it("approval update replaces the pending copy", () => {
    let s = chatReducer(sent(), { type: "approval", approval: ap(9, 2) });
    s = chatReducer(s, { type: "approval", approval: ap(9, 2, "done") });
    expect(s.messages[1].approvals).toHaveLength(1);
    expect(s.messages[1].approvals?.[0].status).toBe("done");
  });

  it("approval with unknown turn goes to loose, and moves in when its turn shows up", () => {
    let s = chatReducer(emptyChat, { type: "approval", approval: ap(9, 7) });
    expect(s.loose.map((a) => a.id)).toEqual([9]);
    s = chatReducer(s, { type: "approval", approval: ap(9, 7, "declined") });
    expect(s.loose).toHaveLength(1);
    expect(s.loose[0].status).toBe("declined");
    s = chatReducer(s, { type: "event", messageId: 7, event: { type: "text_delta", text: "ok" } });
    s = chatReducer(s, { type: "approval", approval: ap(9, 7, "declined") });
    expect(s.loose).toEqual([]);
    expect(s.messages.find((m) => m.id === 7)?.approvals?.[0].id).toBe(9);
  });

  it("calendar tools have step labels", () => {
    expect(stepLabel("calendar_create")).toBe("Adding to your calendar");
    expect(stepLabel("calendar_list")).toBe("Checked your calendar");
    expect(stepLabel("calendar_respond")).toBe("Replying to an invite");
    expect(stepLabel("gmail_search")).toBe("Searched your mail");
    expect(stepLabel("gmail_read_thread")).toBe("Read an email thread");
    expect(stepLabel("gmail_draft")).toBe("Saved a draft");
    expect(stepLabel("gmail_send")).toBe("Sending an email");
  });
});
