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

export type ToolMode = "ask" | "auto" | "off";
export type McpAuth = "none" | "oauth" | "token";

export interface RemoteTool {
  name: string;
  description: string;
  mode: ToolMode;
  default_mode: ToolMode;
  skipped: string | null;
}

export interface RemoteServer {
  name: string;
  url: string;
  auth: McpAuth;
  enabled: boolean;
  status: ConnectorStatus;
  tools: RemoteTool[];
}

export interface BuiltinConnector {
  id: string;
  label: string;
  status: ConnectorStatus;
}

export interface ConnectorsView {
  builtin: BuiltinConnector[];
  servers: RemoteServer[];
}

export interface AddServer {
  url: string;
  name?: string | null;
  token?: string | null;
  client_id?: string | null;
  client_secret?: string | null;
}

export type AddOutcome =
  | ({ kind: "added" } & { name: string; url: string; auth: McpAuth })
  | { kind: "needs_client_id" }
  | { kind: "needs_token"; message: string };

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
  decision: "auto" | "approved" | "denied" | "answered" | "queued" | "expired";
  rule: string | null;
  rpc_id: string | null;
  created_at: string;
  decided_at: string | null;
}

export interface AgentsView {
  sessions: AgentSession[];
  asks: AgentAsk[];
  /** Claude Code asks whose hook waits for my answer in bleep. */
  held: number[];
  /** Claude Code sessions that take a reply from bleep. */
  replyable: string[];
}

/** One line of a coding session's chat. Agent text: show as plain text only. */
export interface AgentChatLine {
  role: "user" | "assistant" | "tool";
  text: string;
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
  | { type: "context"; used: number; window: number | null }
  | { type: "rate_limited"; until: number }
  | { type: "done"; runtime_session_id: string | null }
  | { type: "error"; message: string; retryable: boolean };

export interface MemoryFile {
  file: "USER.md" | "RULES.md" | "MEMORY.md";
  facts: string[];
}

export interface MemoryView {
  files: MemoryFile[];
  /** Characters the files take in the prompt, and the budget. */
  size: number;
  budget: number;
}

export interface Proposal {
  id: number;
  fact: string;
  source: "compaction" | "nightly";
  status: string;
  created_at: string;
}

export interface MemorySettings {
  compact_at: number;
  threshold: number;
  window: number | null;
  nightly: string;
}

export interface ChatContext {
  used: number | null;
  threshold: number;
  window: number | null;
}

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
import type { AvatarConfig } from "./avatar/avatars";
export type { AvatarConfig } from "./avatar/avatars";

export interface Preset {
  id: string;
  label: string;
  wire: "anthropic" | "open_ai";
  base_url: string;
  needs_key: boolean;
  key_url: string;
  main: string;
  small: string;
  max_completion_tokens: boolean;
  free: boolean;
}
export interface ApiConfig {
  provider: string;
  base_url: string;
  main: string;
  small: string;
  daily_cap_usd: number;
}
export interface Tiers {
  main: string;
  small: string;
}
export interface ModelSettings {
  runtime: "claude" | "api";
  tiers: Tiers;
  api: ApiConfig;
  presets: Preset[];
  keys: string[];
}
export interface Price {
  input: number;
  output: number;
  cache_read: number;
  cache_write: number;
}
export interface ModelInfo {
  id: string;
  name: string | null;
  window: number | null;
  price: Price | null;
}
export interface TestResult {
  ok: boolean;
  ms: number;
  cost: number | null;
  message: string;
}
export interface SpendView {
  runtime: string;
  spend: { cost: number; unpriced: number; calls: number };
  cap: number;
  over: boolean;
}
export interface ModelSave {
  runtime: "claude" | "api";
  tiers: Tiers;
  api: ApiConfig;
  key: string | null;
}

export const api = {
  history: (beforeId: number | null, limit = 50) => invoke<Message[]>("chat_history", { beforeId, limit }),
  send: (text: string) => invoke<number>("chat_send", { text }),
  health: () => invoke<RuntimeHealth>("runtime_health"),
  openLogin: () => invoke<void>("open_claude_login"),
  openChat: (anchor: "droid" | "center") => invoke<void>("chat_open", { anchor }),
  hideChat: () => invoke<void>("chat_hide"),
  setChatExpanded: (expanded: boolean) => invoke<void>("chat_set_expanded", { expanded }),
  toggleTuck: () => invoke<void>("droid_toggle_tuck"),
  decide: (id: number, approve: boolean) => invoke<void>("approval_decide", { id, approve }),
  barCurrent: () => invoke<BarCue | null>("bar_current"),
  barPrep: () => invoke<void>("bar_prep"),
  barSnooze: () => invoke<void>("bar_snooze"),
  barJoin: () => invoke<void>("bar_join"),
  barDismiss: () => invoke<void>("bar_dismiss"),
  bubbleClick: () => invoke<void>("bubble_click"),
  bubbleDismiss: () => invoke<void>("bubble_dismiss"),
  bubbleNotUseful: (pingId: number) => invoke<void>("bubble_not_useful", { pingId }),
  googleState: () => invoke<GoogleState>("google_state"),
  googleSaveClient: (clientId: string, clientSecret: string) => invoke<GoogleState>("google_save_client", { clientId, clientSecret }),
  googleConnect: () => invoke<string>("google_connect"),
  googleUseBuiltin: () => invoke<GoogleState>("google_use_builtin"),
  googleDisconnect: () => invoke<void>("google_disconnect"),
  openLink: (which: LinkName) => invoke<void>("open_link", { which }),
  openSettings: () => invoke<void>("settings_open"),
  setupOpen: () => invoke<void>("setup_open"),
  setupFinish: () => invoke<void>("setup_finish"),
  openUrl: (url: string) => invoke<void>("open_url", { url }),
  mailSettings: () => invoke<MailConfig>("mail_settings"),
  mailSetLists: (vips: string[], muted: string[]) => invoke<MailConfig>("mail_set_lists", { vips, muted }),
  mailResolveSuggestion: (addr: string, mute: boolean) => invoke<MailConfig>("mail_resolve_suggestion", { addr, mute }),
  activityList: (limit: number) => invoke<ActivityItem[]>("activity_list", { limit }),
  agentsList: () => invoke<AgentsView>("agents_list"),
  agentsDecide: (askId: number, approve: boolean) => invoke<void>("agents_decide", { askId, approve }),
  agentsChoose: (askId: number, answers: Record<string, string>) => invoke<void>("agents_choose", { askId, answers }),
  agentsReply: (sessionId: string, text: string) => invoke<void>("agents_reply", { sessionId, text }),
  agentsChat: (sessionId: string) => invoke<AgentChatLine[]>("agents_chat", { sessionId }),
  agentsSettings: () => invoke<AgentSettings>("agents_settings"),
  agentsInstallClaude: () => invoke<AgentSettings>("agents_install_claude"),
  agentsRemoveClaude: () => invoke<AgentSettings>("agents_remove_claude"),
  agentsInstallCodexNotify: () => invoke<AgentSettings>("agents_install_codex_notify"),
  agentsRemoveCodexNotify: () => invoke<AgentSettings>("agents_remove_codex_notify"),
  agentsSet: (autoApprove: boolean, screenshotCheck: boolean, codexWatch: boolean) =>
    invoke<AgentSettings>("agents_set", { autoApprove, screenshotCheck, codexWatch }),
  agentsRevealRules: () => invoke<void>("agents_reveal_rules"),
  connectorsList: () => invoke<ConnectorsView>("connectors_list"),
  remoteAdd: (req: AddServer) => invoke<AddOutcome>("remote_add", { req }),
  remoteSignIn: (name: string, token?: string | null) => invoke<void>("remote_sign_in", { name, token: token ?? null }),
  remoteCancelSignIn: () => invoke<boolean>("remote_cancel_sign_in"),
  remoteSetTool: (name: string, tool: string, mode: ToolMode | null) => invoke<void>("remote_set_tool", { name, tool, mode }),
  remoteRemove: (name: string) => invoke<void>("remote_remove", { name }),
  remoteRefresh: (name: string) => invoke<void>("remote_refresh", { name }),
  memoryList: () => invoke<MemoryView>("memory_list"),
  memoryForget: (file: string, fact: string) => invoke<MemoryView>("memory_forget", { file, fact }),
  memorySuggestions: () => invoke<Proposal[]>("memory_suggestions"),
  memoryDecide: (id: number, add: boolean) => invoke<Proposal[]>("memory_decide", { id, add }),
  memorySettings: () => invoke<MemorySettings>("memory_settings"),
  memorySet: (compactAt: number) => invoke<MemorySettings>("memory_set", { compactAt }),
  memoryCompactNow: () => invoke<void>("memory_compact_now"),
  memoryReveal: () => invoke<void>("memory_reveal"),
  chatContext: () => invoke<ChatContext>("chat_context"),
  modelSettings: () => invoke<ModelSettings>("model_settings_get"),
  modelList: (provider: string, baseUrl: string, key: string | null) => invoke<ModelInfo[]>("model_list", { provider, baseUrl, key }),
  modelTest: (provider: string, baseUrl: string, key: string | null, model: string) => invoke<TestResult>("model_test", { provider, baseUrl, key, model }),
  modelSave: (req: ModelSave) => invoke<void>("model_save", { req }),
  modelForgetKey: (provider: string) => invoke<void>("model_forget_key", { provider }),
  spendToday: () => invoke<SpendView>("spend_today"),
  updateState: () => invoke<UpdateView>("update_state"),
  updateCheck: () => invoke<UpdateView>("update_check"),
  updateInstall: () => invoke<UpdateView>("update_install"),
  updateSetAuto: (on: boolean) => invoke<UpdateView>("update_set_auto", { on }),
  updateRestart: () => invoke<void>("update_restart"),
  avatarGet: () => invoke<AvatarConfig>("avatar_get"),
  avatarSet: (config: AvatarConfig) => invoke<AvatarConfig>("avatar_set", { config }),
  voiceSettings: () => invoke<VoiceView>("voice_settings"),
  voiceStatus: () => invoke<VoiceStatus>("voice_status"),
  voiceSet: (config: VoiceConfig) => invoke<VoiceView>("voice_set", { config }),
  voiceSpeak: (text: string) => invoke<void>("voice_speak", { text }),
  voiceStopSpeaking: () => invoke<void>("voice_stop_speaking"),
  voiceTestSpeaker: (voice: string, speaker: string) => invoke<void>("voice_test_speaker", { voice, speaker }),
};

// config::VoiceConfig. Empty device names mean the system default.
export interface VoiceConfig {
  wake: boolean;
  phrase: string;
  sensitivity: "strict" | "normal" | "loose";
  mic: string;
  speak: boolean;
  speaker: string;
  voice: string;
}

// voice::Status.
export type VoicePhase = "off" | "downloading" | "loading" | "listening" | "awake" | "error";
export interface VoiceStatus {
  phase: VoicePhase;
  detail: string;
  mic: string | null;
  level: number;
  last_heard: string | null;
  progress: number | null;
}

// voice::VoiceView.
export interface VoiceView {
  config: VoiceConfig;
  status: VoiceStatus;
  model_ready: boolean;
  model_mb: number;
  mics: string[];
  default_mic: string | null;
  speakers: { id: string; name: string }[];
  voices: { name: string; locale: string }[];
}

// voice::VoiceEvent: the wake phrase was heard, then the request (or nothing).
export type VoiceEvent = { kind: "wake" } | { kind: "heard"; text: string } | { kind: "idle" };
export const onVoice = (cb: (e: VoiceEvent) => void): Promise<UnlistenFn> => listen<VoiceEvent>("voice://event", (e) => cb(e.payload));

// updater::Phase and updater::UpdateView.
export type UpdatePhase =
  | { state: "idle" }
  | { state: "checking" }
  | { state: "up_to_date" }
  | { state: "available"; version: string; notes: string | null }
  | { state: "downloading"; version: string; done: number; total: number | null }
  | { state: "ready"; version: string }
  | { state: "error"; message: string };

export interface UpdateView {
  current: string;
  auto_install: boolean;
  last_checked: string | null;
  phase: UpdatePhase;
}

export const onUpdate = (cb: (v: UpdateView) => void): Promise<UnlistenFn> => listen<UpdateView>("update://changed", (e) => cb(e.payload));

export const onCompacting = (cb: (on: boolean) => void): Promise<UnlistenFn> => listen<boolean>("chat://compacting", (e) => cb(e.payload));

export const onRuntime = (cb: () => void): Promise<UnlistenFn> => listen("runtime://changed", () => cb());

export const onConnectors = (cb: () => void): Promise<UnlistenFn> => listen("connectors://changed", () => cb());

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
/** The buddy changed in Settings. Every window redraws it. */
export const onAvatarChanged = (cb: (c: AvatarConfig) => void): Promise<UnlistenFn> =>
  listen<AvatarConfig>("avatar://changed", (e) => cb(e.payload));
export const onDroidMood = (cb: (mood: "alert" | "idle") => void): Promise<UnlistenFn> =>
  listen<{ mood: "alert" | "idle" }>("droid://mood", (e) => cb(e.payload.mood));
export const onNotice = (cb: (text: string) => void): Promise<UnlistenFn> =>
  listen<{ text: string }>("chat://notice", (e) => cb(e.payload.text));

export const onApproval = (cb: (a: Approval) => void): Promise<UnlistenFn> =>
  listen<Approval>("approval://changed", (e) => cb(e.payload));

/** A droid bubble, saved to the chat so it can be read after the bubble is gone. */
export const onChatExpanded = (cb: (on: boolean) => void): Promise<UnlistenFn> => listen<boolean>("chat://expanded", (e) => cb(e.payload));
export const onPing = (cb: (m: Message) => void): Promise<UnlistenFn> => listen<Message>("chat://ping", (e) => cb(e.payload));

export const onChatEvent = (cb: (e: ChatEmit) => void): Promise<UnlistenFn> =>
  listen<ChatEmit>("chat://event", (e) => cb(e.payload));

export const healthOk = (h: RuntimeHealth | null) => !!h && h.installed && h.version_ok && h.signed_in;
