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

export type ApprovalStatus = "pending" | "approved" | "declined" | "expired" | "done" | "failed";

export interface Approval {
  id: number;
  tool: string;
  payload: Record<string, unknown>;
  status: ApprovalStatus;
  turn_message_id: number | null;
  result: string | null;
  created_at: string;
  decided_at: string | null;
}

export type ConnectorStatus =
  | { state: "connected"; account: string }
  | { state: "needs_setup" }
  | { state: "needs_sign_in" }
  | { state: "error"; message: string };

export interface GoogleState {
  client_id: string;
  has_secret: boolean;
  status: ConnectorStatus;
  expired: boolean;
  /** This build carries bleep's own Google app: users only sign in. */
  builtin: boolean;
  using_own: boolean;
}

export type LinkName = "console" | "calendar_api" | "gmail_api" | "consent" | "credentials";

export interface MailConfig {
  vips: string[];
  muted: string[];
  suggested_mutes: string[];
}

export interface ActivityItem {
  kind: "tool" | "approval" | "model" | "agent" | string;
  name: string;
  detail: string | null;
  ok: boolean;
  at: string;
  /** The agent rule that approved an ask. */
  rule?: string | null;
}

export interface AgentSession {
  id: string;
  tool: "claude" | "codex";
  project: string;
  title: string | null;
  transcript_path: string | null;
  state: "running" | "waiting" | "done";
  /** Agent text: show as plain text only. */
  last_ask: string | null;
  updated: string;
}

export interface AgentAsk {
  id: number;
  session_id: string;
  kind: "permission" | "question";
  summary: string;
  payload: Record<string, unknown>;
  checks: string[];
  decision: "auto" | "approved" | "denied" | "queued" | "expired";
  rule: string | null;
  rpc_id: string | null;
  created_at: string;
  decided_at: string | null;
}

export interface AgentsView {
  sessions: AgentSession[];
  asks: AgentAsk[];
}

export interface AgentSettings {
  claude_hooks: boolean;
  codex_notify: boolean;
  codex_running: boolean;
  auto_approve: boolean;
  screenshot_check: boolean;
  codex_watch: boolean;
  rules_path: string;
}

export interface BarCue {
  key: string;
  kind: "hour" | "ten_min";
  event_id: string;
  title: string;
  start: string;
  end: string;
  meet_link: string | null;
  attendees: string[];
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
  approvals?: Approval[];
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
  decide: (id: number, approve: boolean) => invoke<void>("approval_decide", { id, approve }),
  barCurrent: () => invoke<BarCue | null>("bar_current"),
  barPrep: () => invoke<void>("bar_prep"),
  barSnooze: () => invoke<void>("bar_snooze"),
  barJoin: () => invoke<void>("bar_join"),
  barDismiss: () => invoke<void>("bar_dismiss"),
  bubbleClick: () => invoke<void>("bubble_click"),
  bubbleNotUseful: (pingId: number) => invoke<void>("bubble_not_useful", { pingId }),
  googleState: () => invoke<GoogleState>("google_state"),
  googleSaveClient: (clientId: string, clientSecret: string) => invoke<GoogleState>("google_save_client", { clientId, clientSecret }),
  googleConnect: () => invoke<string>("google_connect"),
  googleUseBuiltin: () => invoke<GoogleState>("google_use_builtin"),
  googleDisconnect: () => invoke<void>("google_disconnect"),
  openLink: (which: LinkName) => invoke<void>("open_link", { which }),
  openSettings: () => invoke<void>("settings_open"),
  openUrl: (url: string) => invoke<void>("open_url", { url }),
  mailSettings: () => invoke<MailConfig>("mail_settings"),
  mailSetLists: (vips: string[], muted: string[]) => invoke<MailConfig>("mail_set_lists", { vips, muted }),
  mailResolveSuggestion: (addr: string, mute: boolean) => invoke<MailConfig>("mail_resolve_suggestion", { addr, mute }),
  activityList: (limit: number) => invoke<ActivityItem[]>("activity_list", { limit }),
  agentsList: () => invoke<AgentsView>("agents_list"),
  agentsDecide: (askId: number, approve: boolean) => invoke<void>("agents_decide", { askId, approve }),
  agentsSettings: () => invoke<AgentSettings>("agents_settings"),
  agentsInstallClaude: () => invoke<AgentSettings>("agents_install_claude"),
  agentsRemoveClaude: () => invoke<AgentSettings>("agents_remove_claude"),
  agentsInstallCodexNotify: () => invoke<AgentSettings>("agents_install_codex_notify"),
  agentsRemoveCodexNotify: () => invoke<AgentSettings>("agents_remove_codex_notify"),
  agentsSet: (autoApprove: boolean, screenshotCheck: boolean, codexWatch: boolean) =>
    invoke<AgentSettings>("agents_set", { autoApprove, screenshotCheck, codexWatch }),
  agentsRevealRules: () => invoke<void>("agents_reveal_rules"),
};

export const onAgents = (cb: () => void): Promise<UnlistenFn> => listen("agents://changed", () => cb());

export const onGoogleStatus = (cb: (s: ConnectorStatus) => void): Promise<UnlistenFn> =>
  listen<ConnectorStatus>("google://status", (e) => cb(e.payload));

export const onBarCue = (cb: (c: BarCue) => void): Promise<UnlistenFn> => listen<BarCue>("bar://cue", (e) => cb(e.payload));
export const onBarHide = (cb: () => void): Promise<UnlistenFn> => listen("bar://hide", () => cb());
export interface BubbleText {
  text: string;
  /** Set for mail pings, which can be marked "Not useful". */
  ping_id?: number | null;
}
export const onBubbleText = (cb: (b: BubbleText) => void): Promise<UnlistenFn> =>
  listen<BubbleText>("bubble://text", (e) => cb(e.payload));
export const onDroidMood = (cb: (mood: "alert" | "idle") => void): Promise<UnlistenFn> =>
  listen<{ mood: "alert" | "idle" }>("droid://mood", (e) => cb(e.payload.mood));
export const onNotice = (cb: (text: string) => void): Promise<UnlistenFn> =>
  listen<{ text: string }>("chat://notice", (e) => cb(e.payload.text));

export const onApproval = (cb: (a: Approval) => void): Promise<UnlistenFn> =>
  listen<Approval>("approval://changed", (e) => cb(e.payload));

export const onChatEvent = (cb: (e: ChatEmit) => void): Promise<UnlistenFn> =>
  listen<ChatEmit>("chat://event", (e) => cb(e.payload));

export const healthOk = (h: RuntimeHealth | null) => !!h && h.installed && h.version_ok && h.signed_in;
