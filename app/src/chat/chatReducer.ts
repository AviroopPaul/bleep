import type { AgentEvent, Message } from "../ipc";

export interface ChatState {
  messages: Message[];
  streamingId: number | null;
  rateLimitedUntil: number | null;
}

export type ChatAction =
  | { type: "history"; messages: Message[]; mode?: "replace" | "prepend" }
  | { type: "sent"; user: Message; assistantId: number }
  | { type: "event"; messageId: number; event: AgentEvent };

export const emptyChat: ChatState = { messages: [], streamingId: null, rateLimitedUntil: null };

const STEP_LABELS: Record<string, string> = { bleep_now: "Checked the time" };
export const stepLabel = (name: string) => STEP_LABELS[name] ?? name;

export function formatBackAt(until: number, timeZone?: string): string {
  const t = new Intl.DateTimeFormat("en-US", { hour: "numeric", minute: "2-digit", timeZone }).format(new Date(until * 1000));
  return `Limit reached, back at ${t}`;
}

function blankAssistant(id: number): Message {
  return { id, role: "assistant", kind: "chat", content: "", tool_steps: [], tokens: null, status: "streaming", session_id: null, created_at: "" };
}

function byId(list: Message[]): Message[] {
  const seen = new Map<number, Message>();
  for (const m of list) if (!seen.has(m.id)) seen.set(m.id, m);
  return [...seen.values()].sort((a, b) => a.id - b.id);
}

function applyEvent(m: Message, ev: AgentEvent): Message {
  switch (ev.type) {
    case "text_delta":
      return { ...m, content: m.content + ev.text };
    case "tool_start":
      return { ...m, tool_steps: [...m.tool_steps, { id: ev.id, name: ev.name, args: ev.args, ok: null, preview: null }] };
    case "tool_result":
      return { ...m, tool_steps: m.tool_steps.map((s) => (s.id === ev.id ? { ...s, ok: ev.ok, preview: ev.preview } : s)) };
    case "usage":
      return { ...m, tokens: ev.input + ev.output };
    case "error":
      return { ...m, content: m.content ? `${m.content}\n${ev.message}` : ev.message, status: "error" };
    case "done":
      return { ...m, status: m.status === "error" ? "error" : "done" };
    case "rate_limited":
      return m;
  }
}

export function chatReducer(state: ChatState, action: ChatAction): ChatState {
  switch (action.type) {
    case "history": {
      const merged = action.mode === "prepend" ? [...action.messages, ...state.messages] : [...action.messages];
      const streaming = merged.find((m) => m.status === "streaming");
      return { ...state, messages: byId(merged), streamingId: streaming ? streaming.id : state.streamingId };
    }
    case "sent": {
      const have = state.messages.some((m) => m.id === action.assistantId);
      const add = have ? [action.user] : [action.user, blankAssistant(action.assistantId)];
      return { ...state, messages: byId([...state.messages, ...add]), streamingId: action.assistantId };
    }
    case "event": {
      const { messageId, event } = action;
      const exists = state.messages.some((m) => m.id === messageId);
      const base = exists ? state.messages : byId([...state.messages, blankAssistant(messageId)]);
      const messages = base.map((m) => (m.id === messageId ? applyEvent(m, event) : m));
      return {
        messages,
        streamingId: event.type === "done" ? null : event.type === "rate_limited" ? state.streamingId : messageId,
        rateLimitedUntil: event.type === "rate_limited" ? event.until : state.rateLimitedUntil,
      };
    }
  }
}
