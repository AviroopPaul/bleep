import { describe, expect, it } from "vitest";
import { chatReducer, emptyChat, formatBackAt, stepLabel, type ChatState } from "./chatReducer";
import type { Message } from "../ipc";

const user = (id: number, content: string): Message => ({
  id, role: "user", kind: "chat", content, tool_steps: [], tokens: null, status: "done", session_id: 1, created_at: "",
});

function sent(): ChatState {
  return chatReducer(emptyChat, { type: "sent", user: user(1, "time?"), assistantId: 2 });
}

describe("chatReducer", () => {
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
    expect(stepLabel("gmail_search")).toBe("gmail_search");
  });
});
