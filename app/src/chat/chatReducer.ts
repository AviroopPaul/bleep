import type { AgentEvent, Approval, Message } from "../ipc";

export interface ChatState {
  messages: Message[];
  streamingId: number | null;
  rateLimitedUntil: number | null;
  /** Cards whose turn is not on screen yet. */
  loose: Approval[];
  /** How full the session is after the last turn. */
  context: { used: number; window: number | null } | null;
}

export type ChatAction =
  | { type: "history"; messages: Message[]; mode?: "replace" | "prepend" }
  | { type: "sent"; user: Message; assistantId: number }
  | { type: "event"; messageId: number; event: AgentEvent }
  | { type: "approval"; approval: Approval }
  | { type: "ping"; message: Message };

export const emptyChat: ChatState = { messages: [], streamingId: null, rateLimitedUntil: null, loose: [], context: null };

const STEP_LABELS: Record<string, string> = {
  bleep_now: "Checked the time",
  calendar_list: "Checked your calendar",
  calendar_next: "Checked what is next",
  calendar_free: "Looked for free time",
  calendar_create: "Adding to your calendar",
  calendar_update: "Changing an event",
  calendar_delete: "Deleting an event",
  calendar_respond: "Replying to an invite",
  gmail_search: "Searched your mail",
  gmail_read_thread: "Read an email thread",
  gmail_draft: "Saved a draft",
  gmail_send: "Sending an email",
  memory_search: "Searched your memory",
  memory_note: "Saved a note",
  memory_promote: "Remembering a fact",
};
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

function upsert(list: Approval[] | undefined, a: Approval): Approval[] {
  const rest = (list ?? []).filter((x) => x.id !== a.id);
  return [...rest, a].sort((x, y) => x.id - y.id);
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
    case "context":
      return m;
  }
}

/** A turn that ends without an error proves the limit is not in force. */
const finishedOk = (list: Message[], id: number) => list.find((m) => m.id === id)?.status === "done";

export function chatReducer(state: ChatState, action: ChatAction): ChatState {
  switch (action.type) {
    case "history": {
      const merged = action.mode === "prepend" ? [...action.messages, ...state.messages] : [...action.messages];
      const streaming = merged.find((m) => m.status === "streaming");
      // A fresh page (after compaction, or on open) belongs to whatever session is current now.
      const context = action.mode === "prepend" ? state.context : null;
      return { ...state, messages: byId(merged), streamingId: streaming ? streaming.id : state.streamingId, context };
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
        ...state,
        messages,
        streamingId: event.type === "done" ? null : event.type === "rate_limited" ? state.streamingId : messageId,
        rateLimitedUntil: event.type === "rate_limited" ? event.until : event.type === "done" && finishedOk(messages, messageId) ? null : state.rateLimitedUntil,
        context: event.type === "context" ? { used: event.used, window: event.window } : state.context,
      };
    }
    case "ping":
      return { ...state, messages: byId([...state.messages, action.message]) };
    case "approval": {
      const a = action.approval;
      const loose = state.loose.filter((x) => x.id !== a.id);
      if (!state.messages.some((m) => m.id === a.turn_message_id)) return { ...state, loose: upsert(loose, a) };
      const messages = state.messages.map((m) => (m.id === a.turn_message_id ? { ...m, approvals: upsert(m.approvals, a) } : m));
      return { ...state, messages, loose };
    }
  }
}
