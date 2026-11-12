# Phase 1: Droid and Chat Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A floating droid on the Mac that opens one continuous chat, backed by Claude Code on my subscription, with one bleep-owned tool (`bleep_now`) proving the tool path, and history that survives restarts.

**Architecture:** One Tauri 2 process. The Rust core holds a SQLite store, an in-process MCP server on `127.0.0.1`, and an `AgentRuntime` trait with one backend that keeps a `claude` child process warm over stream-json. Two see-through React web views: the droid and the chat panel. A ⌥⌥ listener (CGEventTap) summons the chat.

**Tech Stack:** Rust 1.96, Tauri 2.12 (not 3.x, which is alpha), React 19 + TypeScript + Vite, rusqlite 0.42 (bundled), rmcp 3.9, axum 0.8, tokio 1.54, vitest, Playwright (headless Chromium only).

**Spec:** [docs/build-plan.md](../../build-plan.md), sections 6, 7, 8.1, 8.2, 8.8, 8.9, 9 (Phase 1), 10.

## Global Constraints

- macOS 15+ on Apple Silicon. Test machine: M1 MacBook.
- Crates: `tauri = "2.12"`, `tauri-build = "2.7"`, `tauri-plugin-autostart = "2.7"`, `tauri-plugin-global-shortcut = "2.4"`, `tauri-plugin-single-instance = "2.5"`, `rmcp = "3.9"` (features `server`, `macros`, `transport-streamable-http-server`), `axum = "0.8"`, `rusqlite = "0.42"` (feature `bundled`), `tokio = "1"` (full). npm: `@tauri-apps/cli@2.12.4`, `@tauri-apps/api@2.12.4`.
- Tauri config: `"macOSPrivateApi": true`. App runs as an accessory app (no Dock icon): `ActivationPolicy::Accessory`.
- All data under `~/.bleep/`. `BLEEP_HOME` env var overrides the root (tests use a temp dir).
- Claude Code runs with `CLAUDE_CONFIG_DIR=~/.bleep/claude-home` and a fixed working dir `~/.bleep/claude-work` (sessions are stored per working dir, so it must never change).
- Claude chat flags, exactly: `-p --input-format stream-json --output-format stream-json --verbose --include-partial-messages --model <main tier> --tools "" --setting-sources "" --strict-mcp-config --mcp-config <file> --allowedTools <pattern> --append-system-prompt-file <file>` plus `--resume <id>` when a session exists.
- Minimum Claude Code version: `2.1.0`. Tested: `2.1.319`.
- Model tiers: `main = "sonnet"`, `small = "haiku"`.
- MCP tool names are `snake_case`, no dots. MCP server binds `127.0.0.1` only, random port, bearer token.
- No em dashes in any user-facing copy, including the system prompt.
- Look: landing page tokens. Fonts Fredoka (display) + Nunito (body), self-hosted via `@fontsource`, never fetched from Google at runtime.
- Never open the user's browser or run `open <url>` during checks. UI checks use Playwright with headless Chromium.
- Commit only when the user asks. The commit steps below are the points where a commit belongs; batch them if the user says so.

## Review Focus

1. **Claude CLI missing or signed out of `~/.bleep/claude-home`:** the chat shows a clear banner with a "Sign in" button, the app does not crash, and sending is disabled. Test in Task 6.
2. **The child process dies mid-turn:** the turn ends with an error line in the chat, the partial reply is kept, and the next message restarts the child with `--resume`. Test in Task 6.
3. **A stale resume id** (session file gone from `claude-home`): the CLI fails at start. bleep starts a fresh session once, without `--resume`, and history stays on screen. Test in Task 8.
4. **Sending while a turn streams** (double Enter, chips): the second send is refused with `busy` and the input is disabled until `Done`. Test in Task 8.
5. **Quit during a stream:** on next launch, rows left in `streaming` show as `interrupted`, not a spinner forever. Test in Task 3.

---

## File Structure

```
app/
  package.json  vite.config.ts  tsconfig.json  index.html  playwright.config.ts
  src/
    main.tsx                 # routes by window label: droid | chat
    theme.css                # tokens from landing/index.html
    ipc.ts                   # typed invoke/listen wrappers + mock install for tests
    droid/Droid.tsx          # SVG droid, states, click, double-click tuck
    droid/droidSvg.ts        # DROID_FULL markup ported from landing
    chat/ChatPanel.tsx       # list, input, health banner, runtime badge
    chat/chatReducer.ts      # applies AgentEvent to messages (pure)
    chat/ToolSteps.tsx
    chat/chatReducer.test.ts
  e2e/chat.spec.ts           # Playwright, headless, mocked IPC
  src-tauri/
    Cargo.toml  tauri.conf.json  build.rs  capabilities/default.json
    src/
      main.rs  lib.rs        # builder, plugins, setup, commands wiring
      paths.rs               # BleepPaths
      config.rs              # Config (config.toml)
      store/mod.rs           # Store, migrations, messages, sessions
      runtime/mod.rs         # AgentRuntime trait + shared types
      runtime/claude/parse.rs    # stream-json line -> AgentEvent
      runtime/claude/args.rs     # flag builder, auth status parse
      runtime/claude/mod.rs      # ClaudeRuntime (process, warm child)
      runtime/fake.rs        # FakeRuntime for service tests
      mcp/mod.rs             # McpServer: axum + rmcp + bearer check
      mcp/tools.rs           # ToolSpec, Risk, registry, bleep_now
      chat.rs                # ChatService + Tauri commands
      windows.rs             # droid/chat windows, tray, positions
      hotkey/detector.rs     # OptionTapDetector (pure)
      hotkey/macos.rs        # CGEventTap listener + permission
    tests/fixtures/claude/   # text_turn.jsonl, tool_turn.jsonl, rate_limited.jsonl, init_error.jsonl
```

---

### Task 1: Scaffold the Tauri app

**Files:**
- Create: everything under `app/` from the template, then trim.
- Modify: `.gitignore` (add `app/node_modules`, `app/dist`, `app/src-tauri/target`)

**Interfaces:**
- Produces: `npm run tauri dev` starts the app; `cargo test` runs in `app/src-tauri`; `npm test` runs vitest.

- [ ] **Step 1:** Run `npm create tauri-app@4.7.5 app -- --template react-ts --manager npm --identifier site.avirooppaul.bleep` from the repo root. Product name `bleep`.
- [ ] **Step 2:** Pin versions from Global Constraints in `Cargo.toml` and `package.json`. Add vitest, `@fontsource/fredoka`, `@fontsource/nunito`, `@playwright/test`. Scripts: `test` = `vitest run`, `e2e` = `playwright test`.
- [ ] **Step 3:** In `tauri.conf.json`: `app.macOSPrivateApi: true`, remove the default window (windows are made in code, Task 9), `bundle.macOS.minimumSystemVersion: "15.0"`.
- [ ] **Step 4:** In `lib.rs` setup: `app.set_activation_policy(tauri::ActivationPolicy::Accessory)`.
- [ ] **Step 5:** Verify. Run `cd app && npm install && npm run build && (cd src-tauri && cargo build)`. Expected: both succeed. Run `npm run tauri dev`: the app starts with no Dock icon and no window.
- [ ] **Step 6:** Commit: `chore(app): scaffold Tauri 2 + React app`.

---

### Task 2: Paths and config

**Files:**
- Create: `app/src-tauri/src/paths.rs`, `app/src-tauri/src/config.rs`

**Interfaces:**
- Produces:
  - `BleepPaths::from_env() -> BleepPaths` (root = `$BLEEP_HOME` or `~/.bleep`), fields `root, db, memory, claude_home, claude_work, run, logs, config_file: PathBuf`; `fn ensure(&self) -> io::Result<()>` creates dirs, `run` with mode `0700`.
  - `Config { runtime: String, tiers: Tiers { main: String, small: String }, droid_pos: Option<(f64, f64)>, summon_fallback: String, autostart_set: bool }`, `Config::load(&Path) -> Result<Config>` (defaults when missing), `Config::save(&self, &Path) -> Result<()>`.

- [ ] **Step 1: Failing tests** in `config.rs`:
  - `load_missing_file_gives_defaults`: runtime `"claude"`, tiers `sonnet`/`haiku`, `summon_fallback == "Alt+Space"`, `droid_pos == None`.
  - `save_then_load_round_trips`: set `droid_pos = Some((40.0, 600.0))`, save, load, equal.
  - `paths_respect_bleep_home`: with `BLEEP_HOME=/tmp/x`, `db == /tmp/x/bleep.db`, `claude_home == /tmp/x/claude-home`.
- [ ] **Step 2:** Run `cargo test config` . Expected: FAIL (not defined).
- [ ] **Step 3:** Implement with `serde` + `toml`. Unknown keys are ignored so later phases can add fields.
- [ ] **Step 4:** Run `cargo test config`. Expected: PASS.
- [ ] **Step 5:** Commit: `feat(core): paths and config`.

---

### Task 3: SQLite store

**Files:**
- Create: `app/src-tauri/src/store/mod.rs`

**Interfaces:**
- Produces:
  - `Store::open(path: &Path) -> Result<Store>`: WAL, `foreign_keys=ON`, migrations by `PRAGMA user_version`.
  - Tables (spec 8.9 plus a `status` column):
    - `messages(id INTEGER PK, role TEXT, kind TEXT, content TEXT, tool_steps TEXT JSON, tokens INTEGER, status TEXT, session_id INTEGER, created_at TEXT)`; `status` in `streaming|done|error|interrupted`.
    - `sessions(id INTEGER PK, runtime TEXT, runtime_session_id TEXT NULL, started_at TEXT, ended_at TEXT NULL, summary TEXT NULL, tokens_at_end INTEGER NULL)`.
  - `insert_message(&self, m: NewMessage) -> Result<i64>`; `finish_message(&self, id: i64, content: &str, tool_steps: &Value, tokens: Option<i64>, status: MsgStatus) -> Result<()>`.
  - `page_messages(&self, before_id: Option<i64>, limit: u32) -> Result<Vec<Message>>`: returns oldest-first within the page.
  - `current_session(&self, runtime: &str) -> Result<Option<Session>>` (latest with `ended_at IS NULL`), `create_session(&self, runtime: &str) -> Result<Session>`, `set_runtime_session_id(&self, id: i64, rsid: &str) -> Result<()>`, `end_session(&self, id: i64) -> Result<()>`.
  - `mark_streaming_as_interrupted(&self) -> Result<usize>`, called once at startup.
  - `Message`, `Session` derive `Serialize` (sent to the UI as is).

- [ ] **Step 1: Failing tests:**
  - `history_survives_reopen`: open, insert 3, drop, reopen same file, `page_messages(None, 50).len() == 3`.
  - `paging_goes_backwards`: insert 120; `page_messages(None, 50)` ends with id 120 and starts with 71; `page_messages(Some(71), 50)` ends with 70.
  - `wal_mode_on`: `PRAGMA journal_mode` returns `wal`.
  - `streaming_rows_become_interrupted` (Review Focus 5): insert one `streaming`, call `mark_streaming_as_interrupted() == 1`, status now `interrupted`.
  - `current_session_ignores_ended`.
- [ ] **Step 2:** Run `cargo test store`. Expected: FAIL.
- [ ] **Step 3:** Implement. One `Mutex<Connection>`; calls run in `spawn_blocking` from async callers.
- [ ] **Step 4:** Run `cargo test store`. Expected: PASS.
- [ ] **Step 5:** Commit: `feat(store): messages and sessions in SQLite`.

---

### Task 4: Runtime trait and shared types

**Files:**
- Create: `app/src-tauri/src/runtime/mod.rs`, `app/src-tauri/src/runtime/fake.rs`

**Interfaces:**
- Produces (spec 8.1, with the Phase 1 rule: nothing assumes a child process):

```rust
#[async_trait]
pub trait AgentRuntime: Send + Sync {
    fn id(&self) -> &'static str;
    async fn health(&self) -> RuntimeHealth;
    async fn start_session(&self, cfg: SessionConfig) -> Result<SessionHandle>;
    async fn send(&self, s: &SessionHandle, msg: UserMessage) -> Result<EventStream>;
    async fn one_shot(&self, req: OneShot) -> Result<Value>;
}
pub type EventStream = Pin<Box<dyn Stream<Item = AgentEvent> + Send>>;

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    TextDelta { text: String },
    ToolStart { id: String, name: String, args: Value },
    ToolResult { id: String, ok: bool, preview: String },   // preview <= 200 chars
    Usage { input: u64, output: u64 },                      // input = input + cache_creation + cache_read
    RateLimited { until: i64 },                             // unix seconds
    Done { runtime_session_id: Option<String> },
    Error { message: String, retryable: bool },
}
pub struct SessionConfig { pub system_prompt: String, pub mcp: McpEndpoint, pub tier: Tier, pub resume_id: Option<String> }
pub struct McpEndpoint { pub url: String, pub token: String }
pub enum Tier { Main, Small }
pub struct SessionHandle { pub resume_id: Option<String> }   // backend keeps its own state keyed by this
pub struct UserMessage { pub text: String }
pub struct OneShot { pub prompt: String, pub schema: Value, pub tier: Tier }
#[derive(Serialize)] pub struct RuntimeHealth { pub runtime: String, pub installed: bool, pub version: Option<String>, pub version_ok: bool, pub signed_in: bool, pub plan: Option<String>, pub hint: Option<String> }
```

- `FakeRuntime::new(script: Vec<Vec<AgentEvent>>)`: each `send` yields the next scripted turn; records every `SessionConfig` it got in `started: Mutex<Vec<SessionConfig>>`; `fail_start_with_resume: bool` makes `start_session` fail when `resume_id` is set (for Review Focus 3).

- [ ] **Step 1: Failing test** `fake_runtime_replays_turns`: two scripted turns, two sends, collected events equal the script.
- [ ] **Step 2:** Run `cargo test runtime::fake`. Expected: FAIL.
- [ ] **Step 3:** Implement types and `FakeRuntime`.
- [ ] **Step 4:** Run. Expected: PASS.
- [ ] **Step 5:** Commit: `feat(runtime): AgentRuntime trait and fake`.

---

### Task 5: Claude stream parser

**Files:**
- Create: `app/src-tauri/src/runtime/claude/parse.rs`, `app/src-tauri/src/runtime/contract.rs`
- Test fixtures: `app/src-tauri/tests/fixtures/claude/text_turn.jsonl`, `rate_limited.jsonl`, `init_error.jsonl`

**Interfaces:**
- Produces: `ClaudeParser::default()`, `fn feed(&mut self, line: &str) -> Vec<AgentEvent>`. Unknown types return `vec![]`. Bad JSON returns `vec![]` and logs.

Mapping (shapes recorded from Claude Code 2.1.319 on 12 Nov 2026):

| Line | Event |
|---|---|
| `type:"stream_event"`, `event.type:"content_block_delta"`, `event.delta.type:"text_delta"` | `TextDelta{event.delta.text}` |
| `type:"assistant"`, each `message.content[]` with `type:"tool_use"` | `ToolStart{id, name with "mcp__bleep__" removed, args: input}` |
| `type:"user"`, each `message.content[]` with `type:"tool_result"` | `ToolResult{id: tool_use_id, ok: !is_error, preview}` |
| `type:"rate_limit_event"`, `rate_limit_info.status != "allowed"` | `RateLimited{until: rate_limit_info.resetsAt}` |
| `type:"result"`, `is_error:false` | `Usage{...from usage}` then `Done{runtime_session_id: session_id}` |
| `type:"result"`, `is_error:true` | `Error{message: result or api_error_status, retryable: false}` then `Done{session_id}` |
| `type:"assistant"` text blocks | nothing (text already came as deltas) |

- [ ] **Step 1: Record fixtures.** `text_turn.jsonl`: pipe `{"type":"user","message":{"role":"user","content":"Reply with exactly: hi there"}}` into the chat flags with `--model haiku --no-session-persistence` and no MCP flags. `rate_limited.jsonl`: copy `text_turn.jsonl`, set the `rate_limit_event` status to `"rejected"`. `init_error.jsonl`: run the same command with `--resume 00000000-0000-0000-0000-000000000000` and save stdout and the exit code in a comment line at the top (`# exit=N`); the parser skips lines starting with `#`.
- [ ] **Step 2: Failing tests:**
  - `text_turn_yields_text_usage_done`: concatenated `TextDelta` == `"hi there"`; one `Usage` with `output == 5`; last event `Done` with the fixture's session id.
  - `rate_limit_rejected_emits_rate_limited`: contains `RateLimited{until: 1794493800}`.
  - `allowed_rate_limit_is_silent`: `text_turn.jsonl` has no `RateLimited`.
  - `tool_use_name_is_unprefixed`: hand-written line with `name:"mcp__bleep__bleep_now"` yields `name == "bleep_now"`.
  - `garbage_line_is_ignored`: `feed("not json")` is empty.
  - Contract checks (spec section 10, reused by the Codex and API backends later) in `runtime/contract.rs`: `assert_text_turn(&[AgentEvent], expected_text: &str)` (deltas join to the text, one `Usage` with `output > 0`, last is `Done{Some(_)}`), `assert_tool_turn(&[AgentEvent], tool: &str)` (a `ToolStart` and an ok `ToolResult` with the same id, before the last `TextDelta`), `assert_rate_limited(&[AgentEvent])`. The parser tests call these on the fixtures.
- [ ] **Step 3:** Run `cargo test claude::parse`. Expected: FAIL.
- [ ] **Step 4:** Implement with `serde_json::Value` lookups (no full typed model; the CLI adds fields often).
- [ ] **Step 5:** Run. Expected: PASS.
- [ ] **Step 6:** Commit: `feat(runtime): parse Claude stream-json`.

---

### Task 6: Claude backend (process, health, one-shot)

**Files:**
- Create: `app/src-tauri/src/runtime/claude/args.rs`, `app/src-tauri/src/runtime/claude/mod.rs`

**Interfaces:**
- Consumes: Task 4 types, `ClaudeParser`, `BleepPaths`.
- Produces:
  - `chat_args(cfg: &SessionConfig, files: &RunFiles, tiers: &Tiers) -> Vec<String>`; `RunFiles { mcp_config: PathBuf, system_prompt: PathBuf }`. Writes nothing.
  - `write_run_files(paths: &BleepPaths, cfg: &SessionConfig) -> Result<RunFiles>`: `run/mcp.json` (mode `0600`) = `{"mcpServers":{"bleep":{"type":"http","url":<url>,"headers":{"Authorization":"Bearer <token>"}}}}`, `run/system-prompt.md`.
  - `parse_auth_status(json: &str) -> (bool /*signed_in*/, Option<String> /*plan*/)` from `loggedIn` and `subscriptionType`.
  - `ClaudeRuntime::new(paths: BleepPaths, tiers: Tiers, bin: PathBuf)`, implements `AgentRuntime`, `id() == "claude"`.
  - `send` writes one line `{"type":"user","message":{"role":"user","content":<text>}}` to the warm child's stdin and returns events until `Done`. If the child exits before `Done`, emit `Error{message:"bleep lost the connection to Claude Code. Send again to continue.", retryable:true}` then `Done{None}`, and drop the child; the next `send` respawns with `--resume <last session id>`.
  - `one_shot`: separate process, `-p <prompt> --output-format json --json-schema <schema> --model <small> --tools "" --setting-sources "" --strict-mcp-config --no-session-persistence`; returns `structured_output` (recorded: `{"result":"{\"prime\":true}","structured_output":{"prime":true}}`).
  - `health`: `claude --version` (parse leading semver, compare to `2.1.0`) and `claude auth status --json` with `CLAUDE_CONFIG_DIR` set. Hint copy when signed out: `Sign in to Claude Code for bleep. It uses its own login, separate from your terminal.` Hint when missing: `Install Claude Code, then reopen bleep.`
  - Binary lookup: `which claude` from a login shell (`zsh -lc 'command -v claude'`), because a GUI app does not get the terminal PATH.

- [ ] **Step 1: Failing tests:**
  - `chat_args_exact`: for `resume_id: Some("abc")`, the vector has `--tools` followed by an empty string `""`, `--setting-sources` followed by `""`, `--strict-mcp-config`, `--resume`, `abc`, `--model`, `sonnet`, `--include-partial-messages`. For `None`, no `--resume`.
  - `auth_status_signed_out`: `{"loggedIn":false,"authMethod":"none"}` gives `(false, None)`.
  - `auth_status_signed_in`: `{"loggedIn":true,"subscriptionType":"pro"}` gives `(true, Some("pro"))`.
  - `child_death_mid_turn` (Review Focus 2): point `bin` at a test script (`tests/fixtures/claude/die_mid_turn.sh`) that prints the first 4 lines of `text_turn.jsonl` and exits 1. Events end with `Error{retryable:true}` then `Done{None}`.
  - `missing_binary_health` (Review Focus 1): `bin = /nonexistent/claude` gives `installed:false`, `hint` set, no panic.
- [ ] **Step 2:** Run `cargo test claude`. Expected: FAIL.
- [ ] **Step 3:** Implement. Child: `tokio::process::Command`, `kill_on_drop(true)`, env `CLAUDE_CONFIG_DIR`, cwd `claude_work`, stdout read with `BufReader::lines()` into an `mpsc` per turn, stderr to `logs/claude.log`.
- [ ] **Step 4:** Run `cargo test claude`. Expected: PASS.
- [ ] **Step 5:** Commit: `feat(runtime): Claude Code backend`.

---

### Task 7: MCP server with `bleep_now`

**Files:**
- Create: `app/src-tauri/src/mcp/mod.rs`, `app/src-tauri/src/mcp/tools.rs`, `app/src-tauri/examples/mcp_probe.rs`
- Test fixture: `app/src-tauri/tests/fixtures/claude/tool_turn.jsonl`

**Interfaces:**
- Produces:
  - `enum Risk { Read, Safe, Write, Never }`; `struct ToolSpec { name: &'static str, description: &'static str, schema: Value, risk: Risk }`.
  - `McpServer::start(token: String) -> Result<McpServer>`: binds `127.0.0.1:0`; `fn url(&self) -> String` (`http://127.0.0.1:<port>/mcp`); `fn token(&self) -> &str`; dropping it stops the server.
  - Token: 32 random bytes, hex, made fresh at each launch and kept in memory. (Spec says Keychain; a per-launch token never needs storing, because each launch writes a new `mcp.json`. The Keychain lands in Phase 2 with the Google token.)
  - Requests without `Authorization: Bearer <token>` get `401`.
  - Tool `bleep_now` (risk `Read`, no args) returns `{"iso": "...", "date": "2026-11-12", "time": "14:05", "weekday": "Thursday", "timezone": "Asia/Kolkata", "utc_offset": "+05:30"}` from `chrono::Local` and `iana-time-zone`.
  - Calls to tools with risk `Write` return the tool error `Not available yet.` (the approval gate arrives in Phase 2).

- [ ] **Step 1: Failing tests** (rmcp client in dev-dependencies, feature `transport-streamable-http-client-reqwest`):
  - `rejects_missing_token`: raw `reqwest` POST to `url()` without the header gives status 401.
  - `lists_bleep_now`: client with the header, `list_tools` contains `bleep_now`.
  - `bleep_now_has_timezone`: `call_tool("bleep_now")` result JSON has non-empty `timezone` and `utc_offset` matching `^[+-]\d{2}:\d{2}$`.
  - `binds_loopback_only`: `url()` starts with `http://127.0.0.1:`.
- [ ] **Step 2:** Run `cargo test mcp`. Expected: FAIL.
- [ ] **Step 3:** Implement with rmcp's `StreamableHttpService` mounted in an axum `Router` at `/mcp`, plus a bearer-check middleware layer.
- [ ] **Step 4:** Run `cargo test mcp`. Expected: PASS.
- [ ] **Step 5: Pin the allowed-tools pattern.** Start the server from a small test binary (`cargo run --example mcp_probe`, prints url and token), write `mcp.json`, and run one real turn: "Use bleep_now and tell me the time." Try `--allowedTools mcp__bleep` first. If the result has `permission_denials` not empty, use `mcp__bleep__*`. Put the working pattern in `args.rs` as `ALLOWED_TOOLS` and in `chat_args_exact`. Save stdout as `tool_turn.jsonl`.
- [ ] **Step 6: Failing test** `tool_turn_yields_tool_steps` in `parse.rs`: `tool_turn.jsonl` passes `assert_tool_turn(&events, "bleep_now")`. Run it, then make it pass if the mapping needs a fix.
- [ ] **Step 7:** Commit: `feat(mcp): in-app MCP server with bleep_now`.

---

### Task 8: Chat service and commands

**Files:**
- Create: `app/src-tauri/src/chat.rs`
- Modify: `app/src-tauri/src/lib.rs` (state, commands, startup)

**Interfaces:**
- Consumes: `Store`, `AgentRuntime`, `McpServer`, `Config`.
- Produces:
  - `ChatService::new(store: Arc<Store>, runtime: Arc<dyn AgentRuntime>, mcp: McpEndpoint, emit: Arc<dyn Fn(ChatEmit) + Send + Sync>)`.
  - `ChatEmit { message_id: i64, event: AgentEvent }`, emitted to the web views as Tauri event `chat://event`.
  - `async fn ensure_session(&self) -> Result<()>`: uses `current_session("claude")` and its `runtime_session_id` as `resume_id`. If `start_session` fails with a resume id (Review Focus 3), end that session, create a new one, start without resume, and insert a `system` message: `Started a fresh Claude session. Your history is still here.`
  - `async fn send(&self, text: String) -> Result<i64, ChatError>`: refuses with `ChatError::Busy` while a turn runs (Review Focus 4). Inserts the user row (`done`) and an assistant row (`streaming`), returns the assistant id at once, and streams in a spawned task. Accumulates text and tool steps (`[{id,name,args,ok,preview}]`). On `Done`: `finish_message(..., done)`, `set_runtime_session_id` if given. On `Error`: keep partial text, status `error`, error text appended on a new line.
  - Tauri commands: `chat_history(before_id: Option<i64>, limit: u32) -> Vec<Message>`, `chat_send(text: String) -> Result<i64, String>`, `runtime_health() -> RuntimeHealth`, `open_claude_login()` (runs `osascript` to open Terminal with `CLAUDE_CONFIG_DIR=<claude_home> claude auth login`; this opens Terminal, not a browser).
  - System prompt (Phase 1, in `chat.rs` as a const):
    `You are bleep, a small droid assistant that lives on the user's Mac. Be brief and warm. Use the bleep_now tool for the current date, time and time zone. Never guess them. You have no other tools yet. Do not use em dashes.`
  - Startup order in `lib.rs`: paths ensure, config load, store open, `mark_streaming_as_interrupted`, MCP start, runtime, chat service, `ensure_session` in the background.

- [ ] **Step 1: Failing tests** (with `FakeRuntime` and a temp store):
  - `send_persists_both_rows`: scripted `[TextDelta "hi", Usage, Done{Some("s1")}]`; after the stream ends, history has user + assistant `"hi"`, status `done`; session's `runtime_session_id == "s1"`.
  - `second_service_resumes`: new `ChatService` on the same store; `ensure_session` passes `resume_id: Some("s1")` (check `FakeRuntime.started`).
  - `stale_resume_starts_fresh` (Review Focus 3): `fail_start_with_resume = true`; `ensure_session` succeeds, the old session is ended, a system row exists, last start had `resume_id: None`.
  - `busy_while_streaming` (Review Focus 4): a turn that never sends `Done`; second `send` returns `Err(Busy)`.
  - `tool_steps_saved`: scripted `ToolStart` + `ToolResult` + `Done`; row `tool_steps[0].name == "bleep_now"`, `ok == true`.
- [ ] **Step 2:** Run `cargo test chat`. Expected: FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run `cargo test`. Expected: all PASS.
- [ ] **Step 5:** Commit: `feat(chat): chat service, persistence and commands`.

---

### Task 9: Windows, tray, single instance

**Files:**
- Create: `app/src-tauri/src/windows.rs`
- Modify: `app/src-tauri/src/lib.rs`, `app/src-tauri/capabilities/default.json`

**Interfaces:**
- Produces:
  - Droid window: label `droid`, 132x132, transparent, no decorations, no shadow, always on top, `visible_on_all_workspaces(true)`, `skip_taskbar(true)`, `focused(false)`, URL `index.html#droid`. On macOS also set the NSWindow collection behavior to include `FullScreenAuxiliary` (via `ns_window()` and `objc2-app-kit`) so it shows over full-screen apps.
  - Chat window: label `chat`, 400x600, transparent, no decorations, always on top, hidden at start, URL `index.html#chat`. Loses focus does not hide it; Esc hides it (frontend calls `chat_hide`).
  - Pure helpers with tests: `chat_pos(droid: Rect, screen: Rect, chat: Size) -> Point` (right of the droid if it fits, else left; clamped inside `screen` with 12 px margin), `tuck_pos(droid: Rect, screen: Rect) -> Point` (nearest left or right edge, 28 px still visible), `center_pos(screen: Rect, chat: Size) -> Point`.
  - Commands: `chat_open(anchor: "droid" | "center")`, `chat_hide()`, `droid_toggle_tuck()`, `droid_set_visible(bool)`.
  - Droid position: on `WindowEvent::Moved`, save to `config.droid_pos` (debounced 500 ms). Restore on launch; if off every screen, use the bottom-right of the main screen.
  - Tray: items `Show droid` / `Hide droid` (toggles label), `Open chat`, `Pause bleep` (check item; stored in app state for Phase 2), separator, `Quit bleep`.
  - `tauri-plugin-single-instance`: a second launch opens the chat instead.

- [ ] **Step 1: Failing tests:** `chat_goes_right_when_room`, `chat_goes_left_at_right_edge`, `chat_clamped_to_screen`, `tuck_snaps_to_nearest_edge`, `offscreen_pos_resets` (a saved position outside all screens returns the default).
- [ ] **Step 2:** Run `cargo test windows`. Expected: FAIL.
- [ ] **Step 3:** Implement helpers, windows, tray, plugin.
- [ ] **Step 4:** Run `cargo test windows`. Expected: PASS. Run `npm run tauri dev`: droid shows, drag moves it, tray items work, a second `npm run tauri dev` launch focuses the chat.
- [ ] **Step 5:** Commit: `feat(ui): droid and chat windows, tray`.

---

### Task 10: ⌥⌥ summon

**Files:**
- Create: `app/src-tauri/src/hotkey/detector.rs`, `app/src-tauri/src/hotkey/macos.rs`, `app/src-tauri/src/hotkey/mod.rs`

**Interfaces:**
- Produces:
  - `enum KeyEv { OptionDown, OptionUp, OtherKey }`; `OptionTapDetector::default()`; `fn on(&mut self, ev: KeyEv, now_ms: u64) -> bool` (true when a double tap completes).
    - A tap is `OptionDown` then `OptionUp` within 300 ms with no `OtherKey` between.
    - A double tap is two taps whose `OptionUp`s are at most 400 ms apart. After a hit, state resets.
  - `fn listen_input_access() -> bool` (`CGPreflightListenEventAccess`), `fn request_input_access() -> bool` (`CGRequestListenEventAccess`), both via `extern "C"` from ApplicationServices.
  - `fn start_option_tap(on_hit: impl Fn() + Send + 'static) -> Result<()>`: a listen-only `CGEventTap` for `flagsChanged` and `keyDown` on its own thread with its own `CFRunLoop`. Option state from the event flags (`kCGEventFlagMaskAlternate`).
  - Startup: if access is granted, start the tap. If not, call `request_input_access()` once (first run), register `config.summon_fallback` (`Alt+Space`) with `tauri-plugin-global-shortcut`, and show a one-time system message in the chat: `To summon bleep with a double tap of Option, allow bleep in System Settings, Privacy and Security, Input Monitoring. Until then, use Option Space.`
  - On hit: `chat_open("center")`.

- [ ] **Step 1: Failing tests** in `detector.rs`:
  - `double_tap_within_400ms_hits`: Down 0, Up 80, Down 200, Up 280 gives true on the last.
  - `slow_second_tap_misses`: Up at 80 and at 600 gives false.
  - `other_key_between_misses`: Down, Up, OtherKey, Down, Up gives false.
  - `held_option_is_not_a_tap`: Down 0, Up 500, Down 600, Up 650 gives false.
  - `resets_after_hit`: a third tap right after a hit does not hit again alone.
- [ ] **Step 2:** Run `cargo test hotkey`. Expected: FAIL.
- [ ] **Step 3:** Implement detector, then the macOS listener and the startup wiring.
- [ ] **Step 4:** Run `cargo test hotkey`. Expected: PASS. Manual: in `tauri dev`, macOS attributes Input Monitoring to the terminal app that runs it; allow it there, then double-tap ⌥ in another app and the chat opens centered.
- [ ] **Step 5:** Commit: `feat(hotkey): double-tap Option summon with fallback`.

---

### Task 11: Droid and chat panel UI

**Files:**
- Create: `app/src/main.tsx`, `theme.css`, `ipc.ts`, `droid/Droid.tsx`, `droid/droidSvg.ts`, `chat/ChatPanel.tsx`, `chat/chatReducer.ts`, `chat/ToolSteps.tsx`, `chat/chatReducer.test.ts`, `app/e2e/chat.spec.ts`, `app/playwright.config.ts`

**Interfaces:**
- Consumes: commands and `chat://event` from Tasks 8 to 10. `Message` and `AgentEvent` JSON shapes from Tasks 3 and 4 (mirror them as TS types in `ipc.ts`).
- Produces:
  - `chatReducer(state: ChatState, action: ChatAction): ChatState` where `ChatAction` = `{type:"history", messages}` | `{type:"sent", user: Message, assistantId: number}` | `{type:"event", messageId: number, event: AgentEvent}`. `ChatState = { messages: UiMessage[], streamingId: number | null, rateLimitedUntil: number | null }`.
  - `ipc.ts`: `invoke`/`listen` wrappers; `installMocks()` uses `@tauri-apps/api/mocks` `mockIPC` when the URL has `?mock=1` (for Playwright only).
  - Droid states: `idle`, `thinking` (while `streamingId` is set), `alert`, `happy` (2 s after `Done`). Droid window and chat window share state through `chat://event`.
  - Port from `landing/index.html`: CSS tokens (`:root` colors, glass, radius), `DROID_FULL` SVG and its state classes, trace-row styles (the grid fix: `grid-template-columns: 18px minmax(0,1fr) auto`).
  - Chat panel: messages (user right, bleep left), tool steps above the reply text (`bleep_now` shows as "Checked the time"; unknown tools show their name), input (Enter sends, Shift+Enter new line, disabled while streaming or when health is not ok), health banner with `Sign in` button (`open_claude_login`) and `Check again` button (`runtime_health`), runtime badge `Claude Code` in the header, Esc calls `chat_hide`. Load the latest 50 on open; load 50 more when scrolled to the top.
  - Rate limit line: `Limit reached, back at 4:10 PM` (local time of `until`).

- [ ] **Step 1: Failing tests** `chatReducer.test.ts`:
  - `text deltas append to the streaming message`.
  - `tool start then result marks the step ok`.
  - `done clears streamingId`.
  - `error keeps partial text and marks error`.
  - `rate limited sets the back-at time`; a helper `formatBackAt(1794493800, "Asia/Kolkata")` returns `back at 7:20 PM` style text with no em dash.
- [ ] **Step 2:** Run `npm test`. Expected: FAIL.
- [ ] **Step 3:** Implement reducer, then components.
- [ ] **Step 4:** Run `npm test`. Expected: PASS.
- [ ] **Step 5: Playwright** `e2e/chat.spec.ts`, headless Chromium, against `vite` at `/?mock=1#chat`: mocked history of 2 messages renders; typing "what time is it in Tokyo" + Enter shows the user bubble, a "Checked the time" step, and the mocked reply; the input is disabled until the mocked `done`. Mobile width is irrelevant; one viewport 400x600. Run `npm run e2e`. Expected: PASS.
- [ ] **Step 6:** Commit: `feat(ui): droid and chat panel`.

---

### Task 12: First run, launch at login, signed build, done check

**Files:**
- Modify: `app/src-tauri/src/lib.rs`, `app/src-tauri/tauri.conf.json`
- Create: `app/README.md` (run, build, sign-in steps)

**Interfaces:**
- Consumes: everything above.

- [ ] **Step 1:** `tauri-plugin-autostart` (LaunchAgent mode). On first run (`config.autostart_set == false`), enable it and set the flag. Tray gets a `Launch at login` check item.
- [ ] **Step 2:** First run: if `runtime_health` is not ok, open the chat with the banner showing.
- [ ] **Step 3:** Signed build so Input Monitoring survives rebuilds: `bundle.macOS.signingIdentity = "ScreenRecz Local Signing"` (the existing local identity on this Mac; rename later if wanted). Run `npm run tauri build`. Expected: `app/src-tauri/target/release/bundle/macos/bleep.app`. Copy it to `/Applications`.
- [ ] **Step 4: Sign bleep's Claude in.** Use the in-app `Sign in` button. Then `CLAUDE_CONFIG_DIR=~/.bleep/claude-home claude auth status --json` shows `"loggedIn": true`.
- [ ] **Step 5: Done check (spec Phase 1).** Launch `/Applications/bleep.app`. In another app, double-tap ⌥. Ask "what time is it in Tokyo". Expected: a "Checked the time" step, then a correct Tokyo time. Quit from the tray. Reopen. Expected: the whole conversation is there, and a follow-up ("and in London?") answers in context, which proves the session resumed.
- [ ] **Step 6:** Run `cargo test` and `npm test` and `npm run e2e`. Expected: all PASS.
- [ ] **Step 7:** Update the project memory and `docs/build-plan.md` Phase 1 with any flag or pattern that changed (for example the `--allowedTools` pattern). Commit: `feat: Phase 1 droid and chat`.
