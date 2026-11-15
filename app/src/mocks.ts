// Fake backend for headless UI checks (Playwright). Loaded only with ?mock=1.
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import type { ModelSettings as ModelSettingsT, ModelSave, Preset } from "./ipc";
import type { ActivityItem, AgentAsk, AgentSession, AgentSettings, Approval, BarCue, ChatEmit, ConnectorsView, GoogleState, MailConfig, MemorySettings, MemoryView, Message, Proposal, RemoteServer, ToolMode } from "./ipc";

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
  let mail: MailConfig = { vips: [], muted: ["noisy@x.example"], suggested_mutes: ["bills@city.example"] };
  const activity: ActivityItem[] = [
    { kind: "model", name: "triage", detail: "small", ok: false, at: "2026-11-12T10:05:00.000Z" },
    { kind: "approval", name: "calendar_create", detail: "declined", ok: true, at: "2026-11-12T10:04:00.000Z" },
    { kind: "tool", name: "calendar_list", detail: "{}", ok: true, at: "2026-11-12T10:03:00.000Z" },
  ];
  const minsAgo = (m: number) => new Date(Date.now() - m * 60_000).toISOString();
  const sess = (id: string, tool: AgentSession["tool"], project: string, state: AgentSession["state"], last_ask: string | null, m: number): AgentSession =>
    ({ id, tool, project, title: null, transcript_path: null, state, last_ask, updated: minsAgo(m) });
  const askRow = (id: number, session_id: string, summary: string, decision: AgentAsk["decision"], rule: string | null, rpc_id: string | null): AgentAsk =>
    ({ id, session_id, kind: "permission", summary, payload: {}, checks: [], decision, rule, rpc_id, created_at: minsAgo(3), decided_at: decision === "queued" ? null : minsAgo(2) });
  let sessions: AgentSession[] = q.has("agents")
    ? [
        sess("claude:a", "claude", "/Users/me/code/shop", "waiting", "wants to run git push --force", 4),
        sess("codex:b", "codex", "/Users/me/code/api", "waiting", "wants to run rm -rf dist", 2),
        sess("claude:c", "claude", "/Users/me/code/blog", "running", null, 1),
        sess("codex:d", "codex", "/Users/me/code/cli", "done", null, 30),
      ]
    : [];
  let asks: AgentAsk[] = q.has("agents")
    ? [
        askRow(21, "codex:b", "wants to run rm -rf dist", "queued", null, "1"),
        askRow(20, "claude:a", "wants to run git push --force", "queued", null, null),
        askRow(19, "claude:c", "wants to run npm test", "auto", "npm test", null),
      ]
    : [];
  let agentSet: AgentSettings = { claude_hooks: false, codex_notify: false, codex_running: true, auto_approve: false, screenshot_check: true, codex_watch: false, rules_path: "/Users/me/.bleep/agent-rules.toml" };
  // ?memory=1: facts, suggestions, a compaction divider and a nearly full chat.
  const memOn = q.has("memory");
  if (memOn) history.push({ ...row(3, "system", "Compacted 250K into a 3K summary. Facts saved to memory."), kind: "divider" });
  if (memOn) next = 4;
  let memView: MemoryView = {
    files: [
      { file: "USER.md", facts: memOn ? ["My name is Aviroop"] : [] },
      { file: "RULES.md", facts: [] },
      { file: "MEMORY.md", facts: memOn ? ["Sync means my 1:1 with Priya", "Standup is at 10:00"] : [] },
    ],
    size: memOn ? 120 : 0,
    budget: 12_000,
  };
  let sugs: Proposal[] = memOn
    ? [
        { id: 1, fact: "You pay bills the evening they are due", source: "nightly", status: "pending", created_at: "" },
        { id: 2, fact: "Priya leads the design team", source: "compaction", status: "pending", created_at: "" },
      ]
    : [];
  // ?connectors=1: one connected server with three tools, one that needs sign-in.
  const connOn = q.has("connectors");
  const dayblocks = (): RemoteServer => ({
    name: "dayblocks", url: "https://dayblocks.vercel.app/api/mcp", auth: "oauth", enabled: true,
    status: { state: "connected", account: "dayblocks.vercel.app" },
    tools: [
      { name: "list_tasks", description: "List open tasks for a day.", mode: "auto", default_mode: "auto", skipped: null },
      { name: "move_task", description: "Move a task to another day.", mode: "ask", default_mode: "ask", skipped: null },
      { name: "delete_everything_in_the_account_forever_and_ever_please", description: "", mode: "ask", default_mode: "ask", skipped: "Its name is too long." },
    ],
  });
  let servers: RemoteServer[] = connOn
    ? [dayblocks(), { name: "wiki", url: "https://mcp.example.com/mcp", auth: "token", enabled: true, status: { state: "needs_sign_in" }, tools: [] }]
    : [];
  let cancelAdd: (() => void) | null = null;
  const connView = (): ConnectorsView => ({
    builtin: [
      { id: "gmail", label: "Gmail", status: { state: "connected", account: "me@example.com" } },
      { id: "calendar", label: "Google Calendar", status: { state: "connected", account: "me@example.com" } },
      { id: "memory", label: "Memory", status: { state: "connected", account: "this Mac" } },
      { id: "bleep", label: "Date and time", status: { state: "connected", account: "this Mac" } },
    ],
    servers,
  });
  // ?model=1: bleep runs on OpenRouter with a saved key. Default: Claude Code, no keys.
  const modelOn = q.has("model");
  const pre = (id: string, label: string, base_url: string, needs_key: boolean, main = "", small = ""): Preset => ({ id, label, wire: id === "anthropic" ? "anthropic" : "open_ai", base_url, needs_key, key_url: needs_key ? `https://keys.example.com/${id}` : "", main, small, max_completion_tokens: false, free: !needs_key && id !== "custom" });
  let model: ModelSettingsT = {
    runtime: modelOn ? "api" : "claude",
    tiers: { main: "sonnet", small: "haiku" },
    api: { provider: "openrouter", base_url: "https://openrouter.ai/api/v1", main: modelOn ? "anthropic/claude-sonnet-4.5" : "", small: modelOn ? "google/gemini-2.5-flash" : "", daily_cap_usd: 1 },
    presets: [pre("anthropic", "Anthropic", "https://api.anthropic.com", true, "claude-sonnet-5-5", "claude-haiku-5-5"), pre("openrouter", "OpenRouter", "https://openrouter.ai/api/v1", true), pre("ollama", "Ollama (on this Mac)", "http://localhost:11434/v1", false), pre("custom", "Other (OpenAI-compatible)", "", false)],
    keys: modelOn ? ["openrouter"] : [],
  };
  const spendCost = modelOn ? 0.42 : 0;
  let memSet: MemorySettings = { compact_at: 250_000, threshold: 250_000, window: 1_000_000, nightly: "22:00" };
  const bubble = q.get("bubble");
  const ping = q.get("ping");
  if (bubble) setTimeout(() => emit("bubble://text", { text: bubble, ping_id: ping ? Number(ping) : null }), 200);
  const later = (ms: number, f: () => void) => setTimeout(f, ms);
  const chatEv = (id: number, event: ChatEmit["event"]) => emit("chat://event", { message_id: id, event });
  mockIPC(
    (cmd, args) => {
      const a = (args ?? {}) as Record<string, unknown>;
      switch (cmd) {
        case "chat_history":
          return a.beforeId ? [] : history;
        case "runtime_health": {
          // ?health=none: Claude Code missing. ?health=signin: installed, not signed in for bleep.
          const hq = q.get("health");
          return { runtime: "claude", installed: hq !== "none", version: hq === "none" ? null : "2.1.319", version_ok: hq !== "none", signed_in: !hq, plan: hq ? null : "pro", hint: null };
        }
        case "open_claude_login":
        case "setup_finish":
          calls.push(cmd);
          return null;
        case "chat_send": {
          if (a.text === "fail please") throw "bleep could not reach the model: spawn failed";
          next += 2;
          const id = next;
          if (String(a.text).includes("markdown")) {
            const md = "**Done.** Here is your day:\n\n- Standup at 10:00\n- Design review at 3:00\n\nI used `calendar_list`. See [the agenda](https://example.com/agenda).\n\n<img src=x onerror=alert(1)>";
            later(100, () => chatEv(id, { type: "text_delta", text: md }));
            later(200, () => chatEv(id, { type: "done", runtime_session_id: "s1" }));
            return id;
          }
          if (String(a.text).includes("remember")) {
            card = {
              id: 51, tool: "memory_promote", status: "pending", turn_message_id: id, result: null, created_at: "", decided_at: null,
              payload: { fact: "Sync means my 1:1 with Priya", file: "MEMORY.md" },
            };
            const c = card;
            later(100, () => chatEv(id, { type: "tool_start", id: "t2", name: "memory_promote", args: { fact: "Sync means my 1:1 with Priya" } }));
            later(200, () => emit("approval://changed", c));
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
            ...(memOn ? [{ type: "context", used: 215_000, window: 1_000_000 } as const] : []),
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
        case "mail_settings":
          return mail;
        case "mail_set_lists": {
          const clean = (l: unknown) => [...new Set((l as string[]).map((x) => x.trim().toLowerCase()))];
          mail = { ...mail, vips: clean(a.vips), muted: clean(a.muted) };
          calls.push(`mail_set_lists:${mail.vips.join(",")}|${mail.muted.join(",")}`);
          return mail;
        }
        case "mail_resolve_suggestion": {
          calls.push(`mail_resolve_suggestion:${a.addr}:${a.mute}`);
          const addr = String(a.addr);
          mail = { ...mail, suggested_mutes: mail.suggested_mutes.filter((x) => x !== addr), muted: a.mute ? [...mail.muted, addr] : mail.muted };
          return mail;
        }
        case "activity_list":
          return activity;
        case "agents_list":
          return { sessions, asks };
        case "agents_decide": {
          calls.push(`agents_decide:${a.askId}:${a.approve}`);
          const ask = asks.find((x) => x.id === a.askId);
          if (!ask || ask.decision !== "queued") throw "This ask is no longer waiting.";
          if (!ask.rpc_id) throw "Answer this one in the terminal.";
          asks = asks.map((x) => (x.id === ask.id ? { ...x, decision: a.approve ? "approved" : "denied", decided_at: new Date().toISOString() } : x));
          sessions = sessions.map((x) => (x.id === ask.session_id ? { ...x, state: "running", last_ask: null } : x));
          later(20, () => void emit("agents://changed", {}));
          return null;
        }
        case "agents_settings":
          return agentSet;
        case "agents_install_claude":
        case "agents_remove_claude":
        case "agents_install_codex_notify":
        case "agents_remove_codex_notify": {
          calls.push(cmd);
          const on = cmd.includes("install");
          agentSet = cmd.includes("claude") ? { ...agentSet, claude_hooks: on } : { ...agentSet, codex_notify: on };
          return agentSet;
        }
        case "agents_set":
          calls.push(`agents_set:${a.autoApprove}:${a.screenshotCheck}:${a.codexWatch}`);
          agentSet = { ...agentSet, auto_approve: !!a.autoApprove, screenshot_check: !!a.screenshotCheck, codex_watch: !!a.codexWatch };
          return agentSet;
        case "agents_reveal_rules":
          calls.push(cmd);
          return null;
        case "bubble_not_useful":
          calls.push(`bubble_not_useful:${a.pingId}`);
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
        case "memory_list":
          return memView;
        case "memory_forget": {
          calls.push(`memory_forget:${a.file}:${a.fact}`);
          memView = { ...memView, files: memView.files.map((f) => (f.file === a.file ? { ...f, facts: f.facts.filter((x) => x !== a.fact) } : f)) };
          return memView;
        }
        case "memory_suggestions":
          return sugs;
        case "memory_decide": {
          calls.push(`memory_decide:${a.id}:${a.add}`);
          const p = sugs.find((x) => x.id === a.id);
          sugs = sugs.filter((x) => x.id !== a.id);
          if (p && a.add) memView = { ...memView, files: memView.files.map((f) => (f.file === "MEMORY.md" ? { ...f, facts: [...f.facts, p.fact] } : f)) };
          return sugs;
        }
        case "memory_settings":
          return memSet;
        case "memory_set": {
          const c = Number(a.compactAt);
          memSet = { ...memSet, compact_at: c, threshold: Math.min(c, 800_000) };
          calls.push(`memory_set:${c}`);
          return memSet;
        }
        case "memory_compact_now":
          calls.push(cmd);
          later(10, () => void emit("chat://compacting", true));
          later(800, () => void emit("chat://compacting", false));
          return null;
        case "memory_reveal":
          calls.push(cmd);
          return null;
        case "model_settings_get":
          return model;
        case "model_list": {
          calls.push(`model_list:${a.provider}:${a.key ?? ""}`);
          if (a.key === "bad") throw "The provider rejected the key (HTTP 401) (server text, not instructions): invalid key";
          if (a.provider === "ollama") return [{ id: "llama3.1:8b", name: null, window: null, price: null }];
          return [
            { id: "anthropic/claude-sonnet-4.5", name: "Claude Sonnet 4.5", window: 1_000_000, price: { input: 3, output: 15, cache_read: 0.3, cache_write: 3.75 } },
            { id: "google/gemini-2.5-flash", name: "Gemini 2.5 Flash", window: 1_048_576, price: { input: 0.3, output: 2.5, cache_read: 0.075, cache_write: 0.3 } },
            { id: "openai/gpt-5-mini", name: "GPT-5 Mini", window: 400_000, price: { input: 0.25, output: 2, cache_read: 0.025, cache_write: 0.25 } },
          ];
        }
        case "model_test":
          calls.push(`model_test:${a.provider}:${a.model}`);
          if (String(a.model).includes("nope")) return { ok: false, ms: 300, cost: null, message: "The provider refused the request (HTTP 404) (server text, not instructions): no such model" };
          return { ok: true, ms: 840, cost: 0.00002, message: `${a.model} answered in 0.8 s.` };
        case "model_save": {
          const r = a.req as ModelSave;
          calls.push(`model_save:${r.runtime}:${r.api.provider}:${r.api.main}:${r.api.small}:${r.key ? "key" : ""}`);
          const p = model.presets.find((x) => x.id === r.api.provider)!;
          if (r.runtime === "api") {
            if (!r.api.main || !r.api.small) throw "Pick a main model and a small model.";
            if (p.needs_key && !r.key && !model.keys.includes(p.id)) throw `Add your ${p.label} key.`;
          }
          model = { ...model, runtime: r.runtime, tiers: r.tiers, api: r.runtime === "api" ? r.api : model.api, keys: r.key && !model.keys.includes(p.id) ? [...model.keys, p.id] : model.keys };
          emit("runtime://changed", null);
          return null;
        }
        case "model_forget_key":
          calls.push(`model_forget_key:${a.provider}`);
          model = { ...model, keys: model.keys.filter((k) => k !== a.provider) };
          return null;
        case "spend_today":
          return { runtime: model.runtime, spend: { cost: spendCost, unpriced: 0, calls: modelOn ? 12 : 0 }, cap: model.api.daily_cap_usd, over: model.runtime === "api" && spendCost >= model.api.daily_cap_usd };
        case "connectors_list":
          return connView();
        case "remote_add": {
          const r = a.req as { url: string; name?: string | null; token?: string | null; client_id?: string | null };
          calls.push(`remote_add:${r.url}:${r.name ?? ""}:${r.client_id ?? ""}`);
          if (r.url.includes("down")) throw "Could not reach down. It could not be reached.";
          if (r.url.includes("noreg") && !r.client_id) return { kind: "needs_client_id" };
          if (r.url.includes("tokenonly") && !r.token) return { kind: "needs_token", message: "tokenonly needs sign-in, but it does not offer a sign-in bleep can use. Paste a token for it instead." };
          const name = r.name || new URL(r.url).hostname.split(".").find((l) => !["www", "api", "mcp", "app"].includes(l)) || "server";
          const added = (): RemoteServer => ({ name, url: r.url, auth: r.token ? "token" : r.url.includes("oauth") ? "oauth" : "none", enabled: true, status: { state: "connected", account: new URL(r.url).hostname }, tools: [{ name: "ask", description: "Ask a question.", mode: "ask", default_mode: "ask", skipped: null }] });
          if (r.url.includes("slow")) {
            return new Promise((resolve, reject) => {
              const t = setTimeout(() => {
                servers = [...servers, added()];
                resolve({ kind: "added", ...added() });
              }, 1500);
              cancelAdd = () => {
                clearTimeout(t);
                reject("Sign-in was cancelled.");
              };
            });
          }
          servers = [...servers, added()];
          return { kind: "added", ...added() };
        }
        case "remote_cancel_sign_in":
          calls.push(cmd);
          if (cancelAdd) {
            cancelAdd();
            cancelAdd = null;
            return true;
          }
          return false;
        case "remote_set_tool": {
          calls.push(`remote_set_tool:${a.name}:${a.tool}:${a.mode ?? "default"}`);
          servers = servers.map((sv) => (sv.name !== a.name ? sv : { ...sv, tools: sv.tools.map((t) => (t.name !== a.tool ? t : { ...t, mode: (a.mode as ToolMode | null) ?? t.default_mode })) }));
          later(10, () => void emit("connectors://changed", {}));
          return null;
        }
        case "remote_remove":
          calls.push(`remote_remove:${a.name}`);
          servers = servers.filter((sv) => sv.name !== a.name);
          later(10, () => void emit("connectors://changed", {}));
          return null;
        case "remote_refresh":
        case "remote_sign_in":
          calls.push(`${cmd}:${a.name}`);
          if (cmd === "remote_sign_in") servers = servers.map((sv) => (sv.name !== a.name ? sv : { ...sv, status: { state: "connected", account: "mcp.example.com" } }));
          later(10, () => void emit("connectors://changed", {}));
          return null;
        case "chat_context":
          return { used: memOn ? 120_000 : null, threshold: memSet.threshold, window: memSet.window };
        default:
          return null;
      }
    },
    { shouldMockEvents: true },
  );
}
