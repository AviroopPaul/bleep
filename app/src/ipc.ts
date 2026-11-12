// Shapes shared with the Rust core (store::Message, runtime::AgentEvent, chat::ChatEmit).
export type Role = "user" | "assistant" | "system";
export type MsgStatus = "streaming" | "done" | "error" | "interrupted";

export interface ToolStep {
  id: string;
  name: string;
  args: unknown;
  ok: boolean | null;
  preview: string | null;
}

export interface Message {
  id: number;
  role: Role;
  kind: string;
  content: string;
  tool_steps: ToolStep[];
  tokens: number | null;
  status: MsgStatus;
  session_id: number | null;
  created_at: string;
}

export type AgentEvent =
  | { type: "text_delta"; text: string }
  | { type: "tool_start"; id: string; name: string; args: unknown }
  | { type: "tool_result"; id: string; ok: boolean; preview: string }
  | { type: "usage"; input: number; output: number }
  | { type: "rate_limited"; until: number }
  | { type: "done"; runtime_session_id: string | null }
  | { type: "error"; message: string; retryable: boolean };

export interface ChatEmit {
  message_id: number;
  event: AgentEvent;
}

export interface RuntimeHealth {
  runtime: string;
  installed: boolean;
  version: string | null;
  version_ok: boolean;
  signed_in: boolean;
  plan: string | null;
  hint: string | null;
}

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export const api = {
  history: (beforeId: number | null, limit = 50) => invoke<Message[]>("chat_history", { beforeId, limit }),
  send: (text: string) => invoke<number>("chat_send", { text }),
  health: () => invoke<RuntimeHealth>("runtime_health"),
  openLogin: () => invoke<void>("open_claude_login"),
  openChat: (anchor: "droid" | "center") => invoke<void>("chat_open", { anchor }),
  hideChat: () => invoke<void>("chat_hide"),
  toggleTuck: () => invoke<void>("droid_toggle_tuck"),
};

export const onChatEvent = (cb: (e: ChatEmit) => void): Promise<UnlistenFn> =>
  listen<ChatEmit>("chat://event", (e) => cb(e.payload));

export const healthOk = (h: RuntimeHealth | null) => !!h && h.installed && h.version_ok && h.signed_in;
