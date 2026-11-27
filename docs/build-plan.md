# bleep: build plan

Status: draft for approval · 12 Nov 2026 · owner: Aviroop

bleep is a small droid that floats on my MacBook. It reads my mail and calendar, watches my Claude Code and Codex sessions, tells me what matters before I ask, and does small jobs after I say yes. The landing page is live at https://bleep-omega.vercel.app. This plan covers the app itself.

Prior work: `~/Documents/code/valet/docs/architecture.html` (9 Nov). It has the competitor study, the Claude Code headless research, the triage funnel and the threat model. Reuse its ideas. This plan replaces its shape (Python daemon + Telegram) with a native desktop app in Rust.

---

## 1. Why

- **The moment:** I miss things that sit in my mail and calendar. A meeting starts and I am not ready. A bill is due today and I forget. A reply waits for two days.
- **The other moment:** I run several Claude Code and Codex sessions at once. Half of them sit idle, waiting for me to type "yes" to `npm test` or to look at a screenshot. I find out when I come back.
- **The fix:** a companion that watches those sources for me and taps me at the right moment, in a small bar I can read at a glance.
- **Why a droid:** it should feel like a buddy on the screen, not a dashboard. Cute matters.
- **Why now:** proactive assistants (ChatGPT Dots, Grok Bot, minimi) shipped in the last three months. None of them runs on my own Mac with the AI plan I already pay for.

## 2. Goals

- **G1. Always there:** a floating droid on every Space. I can drag it, hide it, and summon it with a double-tap of ⌥ (Option).
- **G2. One chat:** one continuous conversation. No threads, no "new chat". History never disappears from view.
- **G3. Proactive:** it speaks first when something matters. Meeting in 1 hour. Bill due today. Mail that waits on me.
- **G4. Uses the plan I already pay for:** model calls go through a coding agent CLI I have installed and signed in: Claude Code first, Codex later. No API key, no extra bill.
- **G5. Not locked to one AI:** bleep owns its tools. Switching from Claude to Codex, or to an API key from any provider, is a setting, not a rewrite.
- **G5b. Bring your own key (later):** someone with no Claude or ChatGPT plan can paste an API key from any provider (Anthropic, OpenAI, Google, OpenRouter, or any OpenAI-compatible endpoint such as Ollama).
- **G6. Connectors:** Gmail and Google Calendar first. New sources plug in through one interface.
- **G7. Memory:** it learns how I work and I can read and edit what it knows.
- **G8. Light:** idle CPU near zero, low RAM, no battery drain.
- **G9. Unblock my coding agents:** bleep sees every Claude Code and Codex session that waits on me. It approves the safe asks my rules allow (tests, lint, reads, a screenshot that matches the ask), and puts everything else on my list with a short summary.

## 3. Non-goals (for now)

- **Other people:** single user, my Mac only. No accounts, no sync, no server.
- **Other platforms:** no Windows, no Linux, no phone app.
- **Browser or screen automation:** no clicking around websites, no reading the screen.
- **Payments:** it reminds me about bills. It never pays them.
- **Cloud voice:** no audio leaves the Mac. Since 0.4.0 bleep has an opt-in wake word ("hey bleep") with on-device speech to text (Moonshine) and spoken replies through the macOS voices.
- **Plugin marketplace:** connectors are first-party code only.

## 4. Anti-goals (fail even if it works)

- **Nagging:** if it pings so often that I hide it, it failed. Silence is the default.
- **Acting without asking:** sending mail, creating or moving events, or changing anything outside the Mac without my tap.
- **A bad auto-approve:** one approved `git push --force`, deploy, delete or secret read is a failure, even if every other call was right.
- **Burning my plan:** I am on the $20 Claude Pro plan and I share its limits with my own coding. bleep must not eat that budget.
- **Heavy:** an Electron-sized app, a spinning fan, or a slow summon.
- **Leaky:** my mail going anywhere except Google and the AI vendor I picked, through my own login.

## 5. Constraints

- **Personal use only, for now.** Anthropic's and OpenAI's terms allow my own subscription login in a tool I run for myself. If bleep is ever shared, other people use their own API key (Phase 9) or their own CLI login.
- **macOS 15+ on Apple Silicon.** Test machine: my M1 MacBook.
- **At least one agent CLI installed and signed in.** Tested here: Claude Code 2.1.319, Codex CLI 0.164.0. bleep checks the version at start and shows a clear message if the chosen one is missing or signed out.
- **bleep's own Google Cloud app** for Gmail and Calendar access, built into the app. A user only signs in with their Google account. Using your own Cloud app stays possible in settings (section 8.4).
- **Data stays local.** Everything on disk under `~/.bleep/`. Secrets in the macOS Keychain.
- **No em dashes in any user-facing copy**, including prompts that write copy.

---

## 6. Decisions

| # | Decision | Instead of | Why |
|---|---|---|---|
| D1 | **Tauri 2** app: Rust core, web views for UI | Native Swift, Electron | Rust as asked. See-through windows on top of the desktop. Reuses the landing page look. 3D later is three.js in the same view. Small binary. |
| D2 | **React + TypeScript + Vite** for the web views | Vanilla JS | My default stack. Chat, cards and settings get complex. |
| D3 | **Swappable agent runtime.** One Rust trait, three backends: Claude Code CLI (now), Codex CLI (later), API key from any provider (later) | One vendor's SDK or API | Uses the subscription I already have. Codex for someone who only has ChatGPT. An API key for someone with neither. |
| D4 | **bleep owns every tool.** Gmail, Calendar, memory and more are served by an MCP server inside the app (HTTP on `127.0.0.1`, random port, bearer token) | claude.ai connectors, ChatGPT apps | Any runtime that speaks MCP gets the same tools. Nothing depends on one vendor's connector catalogue. |
| D5 | **Approval gate inside bleep's tool handlers** | The CLI's permission hooks, rules in the prompt | Works the same under every runtime. The model can ask for anything. Code decides. A write tool waits until I tap yes or no. |
| D6 | **Direct Google APIs** with my own OAuth client | Google's official MCP servers via claude.ai | Works with any runtime. Rust can poll for free, so new mail shows in about 2 minutes with no model call. |
| D7 | **Poll Google APIs**, no push | Gmail Pub/Sub push | Push needs a public URL. Polling with `historyId` and `syncToken` is cheap and catches up after sleep. |
| D8 | **SQLite + Markdown** | Postgres, vector DB | One user, small data. FTS5 search is enough. |
| D9 | **Own compaction** at a set token count | Each CLI's built-in compact | Same behaviour under every runtime. Facts saved to memory first, and a visible marker in the chat. |
| D10 | **Rules first, small model second, main model last** | One big model for everything | Protects my plan. Most events are noise and cost nothing. |
| D11 | **Watch coding agents through their own hooks and protocols:** Claude Code hooks, the Codex app-server | Reading the screen, typing into terminals | Both tools expose "waiting on you" as data. Approvals go back through the same channel the tool already trusts. No screen automation (non-goal). |
| D13 | **Every backend is only the model.** bleep runs the agent loop, runs every tool, and keeps the one chat thread in SQLite. A backend (API key, Claude Code, Codex, Antigravity, any later one) answers one model step at a time, with no session of its own (27 Nov 2026) | Letting a CLI run its own loop, call the tools over MCP and keep the history (`--resume`, thread ids) | One thread under every backend. A CLI's session file can be missing (a copied database gave "No conversation found" errors), and its loop and limits differ per vendor. A backend is a `ModelAdapter` under `runtime/api/agent.rs`. |
| D12 | **Agent approvals are decided by rules in code.** The model may only make a call stricter | Letting the model decide | The ask text comes from an agent that may have read untrusted pages. A model can be talked into "yes". A rule cannot. |

Considered and dropped on 12 Nov 2026: using Google's official Gmail and Calendar MCP servers through my claude.ai connectors. They work in headless Claude Code today, but only Claude can reach them, so they fail G5.

---

## 7. Architecture

### 7.1 Processes

```
bleep.app (Tauri, one process)
├── Rust core (tokio)
│   ├── Agent runtime (trait)
│   │     ├── Claude Code backend ──spawns──▶ claude CLI  ─┐
│   │     ├── Codex backend       ──spawns──▶ codex CLI   ─┤  (one active at a time)
│   │     │                                               └──HTTP──▶ bleep MCP server
│   │     └── API backend (BYOK)  ──HTTPS──▶ provider API; bleep runs the tool loop itself
│   │                                         and calls the connector registry directly
│   ├── MCP server (in-process, 127.0.0.1) ── tools from connectors, through the approval gate
│   ├── Connectors: Gmail, Google Calendar (later Dayblocks, others)
│   ├── Agent watch: Claude Code hooks ◀── bleep-hook ◀── my claude sessions
│   │                Codex app-server  ◀── JSON-RPC client ── my codex sessions
│   ├── Watchers + scheduler ── events ── triage ── notifier
│   ├── Store: SQLite (~/.bleep/bleep.db) + memory files (~/.bleep/memory/)
│   └── Keychain: Google refresh token, MCP bearer token
└── Web views (React)
    ├── droid window    (see-through, always on top, draggable)
    ├── chat panel      (opens next to the droid, or from ⌥⌥)
    └── meeting bar     (top center, under the menu bar)
```

Menu bar icon (tray): show or hide droid, open chat, pause bleep (stops all proactive pings), settings, quit.

### 7.2 Folder layout on disk

```
~/.bleep/
  config.toml          # runtime choice, models, check-in times, quiet hours, ping budget, compaction threshold
  bleep.db             # SQLite, WAL mode
  memory/
    USER.md            # who I am, key people
    RULES.md           # VIPs, muted senders, my rules
    MEMORY.md          # learned facts, approved by me
    daily/2026-11-12.md
  agent-rules.toml     # what bleep may auto-approve in my coding agents, and what always asks
  bleep.sock           # local socket the bleep-hook binary talks to
  claude-home/         # CLAUDE_CONFIG_DIR for bleep's Claude Code (separate from ~/.claude)
  codex-home/          # CODEX_HOME for bleep's Codex (separate from ~/.codex)
  logs/bleep.log
```

### 7.3 Repo layout

```
bleep/
  landing/             # the live landing page (keep as is)
  app/
    src-tauri/         # Rust: main.rs, runtime/, mcp/, connectors/, proactive/, store/, policy/, hotkey/, agentwatch/
    bleep-hook/        # tiny binary that Claude Code hooks call; forwards to ~/.bleep/bleep.sock
    src/               # React: droid/, chat/, bar/, settings/
  docs/build-plan.md   # this file
```

`vercel.json` keeps pointing at `landing/`. The app never deploys anywhere.

---

## 8. Core parts

### 8.1 Agent runtime

One trait. Everything above it (chat, scheduler, compaction) never knows which vendor runs.

```rust
#[async_trait]
pub trait AgentRuntime: Send + Sync {
    fn id(&self) -> &'static str;                          // "claude", "codex"
    async fn health(&self) -> RuntimeHealth;               // installed, version, signed in
    async fn start_session(&self, cfg: SessionConfig) -> Result<SessionHandle>; // or resume
    async fn send(&self, s: &SessionHandle, msg: UserMessage) -> Result<EventStream>;
    async fn one_shot(&self, req: OneShot) -> Result<Value>; // no tools, JSON schema answer
}
// EventStream yields: TextDelta, ToolStart{name,args}, ToolResult, Usage{in,out}, RateLimited{until}, Done, Error
```

- **SessionConfig** holds: system prompt (persona + memory files + date and time), bleep MCP server URL and token, model tier (`main` or `small`), and a resume id.
- **Model tiers, not names:** `main` for chat and drafts, `small` for triage, summaries and fact extraction. `config.toml` maps them per runtime (Claude: Sonnet and Haiku).
- **Normalised events:** each backend parses its own stream into the same events. The chat UI shows tool calls as steps, like the landing page demo.
- **Usage:** every turn reports input and output tokens. They feed the context meter and the compaction trigger.
- **Plan limits:** a `RateLimited` event shows in the chat ("Limit reached, back at 4:10 PM") and pauses proactive model work until the reset. Rules-only reminders keep working.
- **Switching:** settings has a runtime picker. Every backend runs on bleep's loop and history (D13), so a switch continues the same thread: the new backend gets the same conversation. A carry-over (compaction summary plus the last turns) opens a new conversation as its first message, never in the system prompt, because it can quote mail and web text. A "prompt is too long" error compacts the chat after the turn.

#### Claude Code backend (Phase 1, reworked 27 Nov 2026 under D13)

- **Only the model:** bleep's own loop (`runtime/api/agent.rs`) calls `ClaudeCodeAdapter` (`runtime/claude/adapter.rs`) once per model step. Each step is one stateless `claude -p` with `--input-format stream-json --output-format stream-json --verbose --include-partial-messages`, plus:
  - `--model` from the tier,
  - `--tools ""`, `--strict-mcp-config` (and no `--mcp-config`), `--setting-sources ""` and `--disable-slash-commands`: Claude Code runs no tool and loads nothing of mine,
  - `--system-prompt-file <file>`: bleep's prompt replaces Claude Code's, with the protocol and bleep's tool list after it,
  - `--no-session-persistence`, and never `--resume`,
  - `--json-schema` with `{text, tool_calls}` when tools are on. The reply streams as `input_json_delta`, so bleep reads the `text` field as it arrives.
- **History:** the whole conversation goes in one user message as JSON lines (user, assistant with tool_calls, tool_result), from bleep's `api_messages` table. Prompt caching still hits, because each step only adds lines at the end.
- **Env:** `CLAUDE_CONFIG_DIR=~/.bleep/claude-home`. Working dir: an empty folder. I sign in once with `CLAUDE_CONFIG_DIR=~/.bleep/claude-home claude auth login`; settings has a button that opens this in Terminal.
- **One-shots:** the same adapter with the forced tool's schema as `--json-schema`; images go in the user message.
- **Learned on 27 Nov 2026 (Claude Code 2.1.325):** a `--json-schema` call is one model call (`num_turns` 2 counts the StructuredOutput tool). Haiku needs to be told that bleep's tools are real and reached through `tool_calls`, or it says the tool "isn't available". A step takes 3 to 4 s on Haiku.
- **Learned in Phase 9:** OpenRouter reserves credit for `max_tokens` before a call and answers `402` when the balance is short, so a low balance fails a large `max_tokens` even for a tiny reply. Gemini's OpenAI-compatible endpoint takes tools and `tool_choice`, gives ids like `function-call-<n>`, lists models without context windows, and counts thinking against `max_tokens` (a tool call came back with `finish_reason: length` at 100 tokens). OpenAI-format usage counts cached tokens inside `prompt_tokens`; Anthropic counts them apart. A forced tool call (`answer` with the JSON schema) gives structured one-shots on both formats. Tested live on 15 Nov 2026 with OpenRouter (`google/gemini-2.5-flash`) and Gemini direct.
- **Learned in Phase 7:** rmcp 3.9 has the whole MCP client OAuth flow (`AuthorizationManager`, `AuthorizationSession`, `AuthClient`), on reqwest 0.13, so bleep carries reqwest 0.12 and 0.13 side by side. rmcp refreshes a token 30 s before it expires, and it erases error types on the way out, so "auth required" is known by its text. It has no token revocation; bleep calls the `revocation_endpoint` itself. Real servers vary: Linear answers `401` with `resource_metadata` and offers registration, S256 and revocation; DeepWiki needs no auth, answers as SSE, keeps no session, and sends no tool annotations, so every DeepWiki tool asks first until I set it to run at once.
- **Learned in Phase 5:** Claude Code 2.1.325 fixes the system prompt when a session is created: `--resume` keeps the first `--append-system-prompt-file` and ignores a new one. The `result` line's `usage` adds up every model call in a turn, so the context size is the last `assistant` message's usage; `result.modelUsage.<model>.contextWindow` gives the window (1M for Sonnet on my plan, so the 250K default holds). `DISABLE_AUTO_COMPACT=1` turns off both auto-compact and idle compaction. One-shot prompts travel as a command-line argument, so the compaction transcript is capped at 150 KB.
- **Learned in Phase 4:** Claude Code 2.1.319 sends `PermissionRequest` only when it would prompt me, also in headless runs; `{"hookSpecificOutput":{"hookEventName":"PermissionRequest","decision":{"behavior":"allow"}}}` skips the prompt and no output leaves it. `Stop` input carries `last_assistant_message`. The Codex daemon's control socket (`~/.codex/app-server-control/app-server-control.sock`) speaks JSON-RPC inside WebSocket frames, not newline JSON, and `codex app-server proxy` does not start the daemon. A client that subscribes (`thread/resume`) after an approval request gets it replayed, the first answer wins, the other client gets `serverRequest/resolved`, and `thread/unsubscribe` does not unload a thread another client holds. Codex wraps commands as `/bin/zsh -lc '<cmd>'`. An image one-shot needs `--input-format stream-json --output-format stream-json --verbose`.
- **Answering from bleep (Claude Code 2.1.325):** in an interactive session the terminal permission dialog shows while `PermissionRequest` hooks run, and the first answer wins. So bleep holds the hook open (timeout 3600 s) until I tap Approve or Deny in the panel, and the terminal is never blocked. `AskUserQuestion` also goes through `PermissionRequest`; `{"behavior":"allow","updatedInput":{...input,"answers":{"<question>":"<label>"}}}` answers it (several labels joined with ", "). A Stop hook with `"asyncRewake": true` runs in the background; exit 2 wakes the idle session with its stderr. `bleep-hook claude-reply Stop` waits there for my reply. Typing in the terminal (`UserPromptSubmit`) lets it go. Rules never answer an `AskUserQuestion`. bleep updates an older hook install at start.
- **Learned in Phase 3:** Gmail `history.list` answers 404 when the stored `historyId` is too old; bleep then reseeds from the last 2 days and those mails never ping. Rules sort most mail for free (categories, bill words plus an amount, VIPs, mailing lists); only unclear mail reaches the small model, 10 per call, at most 20 calls an hour.
- **Learned in Phase 2:** Claude Code 2.1.319 aborts an MCP tool call that sends nothing for 300 s, and `MCP_TOOL_TIMEOUT` does not lift that. A per-server `"timeout": 660000` in the MCP config does: a live probe held a write call on an approval card for 371 s and the answer reached the model.
- **Learned in Phase 1:** a stale `--resume` id fails within about 0.5 s with no input, so `start_session` waits up to 1.5 s when resuming. `ANTHROPIC_API_KEY` and the `CLAUDECODE` markers are removed from the child's env so it always uses the subscription. bleep's MCP server runs stateless with plain JSON replies, and Claude Code accepts that.

#### Codex backend (later, under D13)

- **Only the model:** a `ModelAdapter` like Claude Code's. Each step is one stateless `codex exec --json --output-schema <file>` call with bleep's prompt, the conversation as JSON lines and the `{text, tool_calls}` schema. No `app-server` thread, no resume, no MCP.
- **Built-in tools off:** Codex has its own shell and patch tools. Turn them off with config overrides (`shell_tool`, `include_apply_patch_tool`; confirm the keys on the installed version), with a read-only sandbox, no network and an empty working dir as a second guard.
- **Env:** `CODEX_HOME=~/.bleep/codex-home`, signed in once with my ChatGPT account.
- **Source to read:** the Codex repo is cloned at `~/Documents/code/codex` (`codex-rs/exec`).
- **Antigravity:** the same pattern, tracked in issue #51.

#### API backend, bring your own key (Phase 9)

- **No CLI.** bleep runs the agent loop itself: send messages and tool specs, stream the reply, run tool calls through the same registry and approval gate, send results back, repeat (max 8 steps).
- **Providers:** Anthropic, OpenAI, Google Gemini, OpenRouter, and any OpenAI-compatible endpoint (Ollama, LM Studio, Groq and others). Two wire formats cover most of them: Anthropic Messages and OpenAI Chat Completions. Pick a maintained multi-provider Rust crate at build time, or write thin clients for those two formats.
- **Keys:** pasted in settings, stored in the Keychain, never in `config.toml` or logs. Base URL field for OpenAI-compatible endpoints.
- **Tiers:** settings maps `main` and `small` to the provider's models. A local model through Ollama is allowed but must support tool calls.
- **Session:** there is no CLI session to resume. bleep keeps the message list in SQLite and sends it each turn. Use prompt caching where the provider supports it.
- **Real money:** API calls are billed. Show a running cost estimate in the Activity tab, and a daily spend cap in settings (default $1) that pauses proactive work when hit.

#### Phase 1 rule for all of this

The `AgentRuntime` trait must not assume a child process or a CLI-owned session. `start_session` may just create a row in SQLite. The contract tests (section 10) run against every backend.

### 8.2 MCP server and the approval gate

- **Server:** Rust MCP server (the `rmcp` crate) on `127.0.0.1:<random port>`, streamable HTTP, bearer token stored in the Keychain and handed to the runtime at start.
- **Tool names:** `snake_case` with a connector prefix, for example `gmail_search`. No dots, because some model APIs reject them in tool names.
- **Every tool has a risk level:**

| Level | What happens | Tools |
|---|---|---|
| `read` | runs at once | `gmail_search`, `gmail_read_thread`, `calendar_list`, `calendar_next`, `calendar_free`, `memory_search`, `bleep_now` |
| `safe` | runs at once; nothing leaves my account and it is easy to undo | `gmail_draft` (save or update a draft), `memory_note` (write to today's daily note) |
| `write` | asks me first | `gmail_send` (new mail or reply, from a draft or direct), `calendar_create`, `calendar_update`, `calendar_delete`, `calendar_respond`, `memory_promote` (add a fact to `MEMORY.md`) |
| `never` | not built | forward mail, trash or delete mail, labels, payments, shell, web fetch |

- **Asking:** a `write` tool call puts an approval card in the chat with the exact payload (recipient, subject, body, event time). The tool handler waits for my answer for up to 10 minutes. Approve runs the API call and returns the result. Decline or timeout returns "declined by user" to the model. The payload I saw is the payload that runs; any change makes a new card.
- **Limits:** max 8 tool steps per turn, max 10 sends per day, 120 s per turn excluding time spent waiting on me.

### 8.3 Connector interface

Internal Rust trait. Not shown on the landing page.

```rust
#[async_trait]
pub trait Connector: Send + Sync {
    fn id(&self) -> &'static str;                 // "gmail", "calendar"
    fn tools(&self) -> Vec<ToolSpec>;             // name, description, JSON schema, risk level
    async fn call(&self, tool: &str, args: Value) -> Result<Value>;
    async fn changes(&self, since: Cursor) -> Result<(Vec<Event>, Cursor)>; // feeds proactivity, no model
    fn status(&self) -> ConnectorStatus;          // connected, needs sign-in, error
}
```

- A registry holds the connectors. The MCP server lists all their tools and routes calls through the approval gate. The watcher loop calls `changes()` on a timer per connector.
- Adding a source means one new module that implements the trait, plus a settings row to connect it.

### 8.4 Google sign-in (shared by Calendar and Gmail)

- **bleep's own client (decided 12 Nov 2026):** one OAuth client of type "Desktop app" in my Google Cloud project, compiled into the app from `BLEEP_GOOGLE_CLIENT_ID` and `BLEEP_GOOGLE_CLIENT_SECRET` (kept in my shell secrets, never in git). Google treats a desktop client's secret as public; PKCE protects each sign-in. No auth server: it would add a dependency that must stay up and put every user's tokens through it, for no security gain.
- **For users:** open Settings, press Connect Google, sign in. Nothing else.
- **One-time setup (me, about 15 min):** enable the Gmail API and the Calendar API, create the Desktop app client, set the consent screen to "In production". "Testing" expires refresh tokens every 7 days.
- **Own client (advanced):** settings can hold a user's own client ID and secret, which win over the built-in one. Changing the client forgets the sign-in, because a refresh token only works with the client that issued it.
- **Sign-in:** loopback redirect with PKCE (the `oauth2` crate). bleep opens the Google consent page in my default browser once.
- **Scopes:** `calendar.events`, `gmail.readonly`, `gmail.compose`. Never the full `mail.google.com` scope.
- **Storage:** an own client in `config.toml` (mode 600), refresh token in the Keychain (`keyring` crate), access token in memory only.
- **HTTP:** plain `reqwest` calls to the REST APIs. Skip the heavy generated Google crates.
- **Google's review:** until the app is verified, Google shows an "unverified app" warning and allows at most 100 users. The Gmail scopes are restricted: a public launch needs Google's verification plus a yearly paid security assessment. Calendar alone needs only the lighter, free verification.

### 8.5 Proactive engine

Three triggers feed one funnel:

1. **Watchers (no model):** Calendar every 5 min (`events.list` with `syncToken`), Gmail every 2 min (`history.list` from the last `historyId`). After sleep, one call catches up.
2. **Scheduler:** fixed jobs from `config.toml` (morning brief 9:00, check-ins 13:00 and 18:00) and timers set from the calendar cache (meeting bar at 60 and 10 min before each event).
3. **Open-loop check:** every 30 min in active hours. Code first; it calls a model only when something new exists.

The funnel: **rules in Rust** (free: `RULES.md` VIPs and muted senders, Gmail categories, known bill senders) → **`small` one-shot** (only for unclear mail, batched up to 10 per call) → **main chat turn** (only for `needs_reply`, `bill`, and scheduled check-ins).

Mail labels from triage: `ignore`, `fyi`, `important`, `needs_reply`, `bill` (with amount and due date).

Calm rules:
- **Quiet hours:** 23:00 to 08:00. Held items go into the morning brief.
- **Ping budget:** max 4 pings an hour. Extra items merge into the next one.
- **Dedup:** one ping per thread or event per 6 hours.
- **Pause:** one click in the tray stops all proactive output.
- **Feedback:** each ping has "not useful". Three of those for one sender proposes a rule for `RULES.md`.

Where proactive output shows up:
- **Meeting bar:** no model call. Built from the calendar cache. Buttons: "Prep me" (runs a chat turn), "Snooze" (until 10 min before), "Join" (opens the meeting link if the event has one).
- **Droid bubble:** a short line over the droid, plus the antenna light. Clicking it opens the chat.
- **Chat:** check-ins and briefs are turns in the one continuous chat. The scheduler sends a tagged prompt (for example `[scheduled 13:00 check-in]`) into the main session. The chat shows a small "scheduled" label and bleep's reply, not the raw prompt.

### 8.6 One chat and compaction

- **What I see:** every message ever, loaded from SQLite in pages as I scroll up. Compaction never removes anything from view.
- **What the model sees:** bleep's own record of the current session (`api_messages`), sent in full each step, under every backend. Nothing depends on a CLI's session files.
- **Trigger:** when the session's input tokens pass the threshold (default 250K, setting allows 200K to 300K):
  1. A `small` one-shot extracts durable facts. They go to today's daily note and become proposals for `MEMORY.md`.
  2. A `main` one-shot writes a summary of the session (target about 3K tokens): open loops, decisions, people, pending approvals.
  3. bleep starts a new session. Its first message holds the summary plus the last 10 turns.
  4. The chat shows a divider: "Compacted 250K into a 3K summary. Facts saved to memory."
- **Context window:** the threshold only works if the model's window is larger than it. In Phase 5, check the window each runtime actually gets on my plan. If it is 200K, the threshold becomes 80% of the window (160K) and settings says so.
- **Built-in compact:** both CLIs may compact on their own near their limit. Check this in Phase 5 and turn it off for bleep's sessions where a setting exists, so only bleep's compaction runs.

### 8.7 Memory

- **Always in the prompt:** `USER.md`, `RULES.md`, `MEMORY.md`, kept under 3K tokens together.
- **Searched on demand:** daily notes and old chat through `memory_search` (SQLite FTS5).
- **Writing:** `memory_note` writes new facts to today's daily note freely. `memory_promote` into `MEMORY.md` needs my yes (an approval card, or a nightly batch of proposals).
- **Patterns:** a nightly job (one `small` call) reads the day's events and decisions and proposes patterns, for example "You pay bills the evening they are due."
- **Editing:** a memory view in settings lists facts with "forget". The files are plain Markdown and I can edit them in any editor.
- **Runtime-neutral:** memory lives in bleep, not in either CLI. Switching runtimes keeps everything.

### 8.8 UI

- **Droid window:** small see-through window, always on top, visible on all Spaces and over full-screen apps, never steals focus. Drag to move; position saved. Click opens the chat. Double-click tucks it into the screen edge as a glowing tab. Phase 1 droid is the 2D SVG from the landing page with the same states: idle, thinking, alert, happy.
- **⌥⌥ summon:** a global listener for two Option presses within 400 ms with no other key between. It needs the Input Monitoring permission; bleep asks for it on first run and explains why. Fallback if denied: a normal global shortcut (⌥Space by default).
- **Chat panel:** frosted panel next to the droid (or centered when summoned). Streaming replies, tool steps, approval cards, suggestion chips, context meter, and a small label with the active runtime. Esc closes it.
- **Meeting bar:** slim dark bar at the top center, under the menu bar. Slides in, stays until dismissed or the meeting starts.
- **Look:** the landing page tokens: Fredoka + Nunito, muted blue and sand, frosted glass. Light and dark from the system.
- **Launch at login:** on by default (Tauri autostart plugin).

### 8.9 Storage (SQLite)

```
messages      id, role, kind (chat|scheduled|system|approval|divider), content, tool_steps, tokens, session_id, created_at
sessions      id, runtime, runtime_session_id, started_at, ended_at, summary, tokens_at_end
approvals     id, tool, payload, status (pending|approved|declined|expired|done|failed), created_at, decided_at
events        id, source, kind, payload, status, idem_key, created_at
cal_cache     event_id, start, end, title, attendees, meet_link, updated
mail_cache    thread_id, from, subject, snippet, labels, triage, amount, due, last_seen
pings         id, kind, ref_id, sent_at, feedback
llm_calls     id, runtime, tier, purpose, tokens_in, tokens_out, created_at
cursors       connector, value
agent_sessions id, tool (claude|codex), project, transcript_path, state (running|waiting|done), last_ask, updated
agent_asks    id, session_id, kind (permission|question), payload, checks, decision (auto|approved|denied|queued|expired), rule, decided_at
mem_fts       FTS5 over memory/*.md and messages
```

### 8.10 Security

- **Prompt injection through coding agents:** an agent's ask and transcript can carry text from web pages or files it read. Rules decide every auto-approve (D12). Transcripts go to the model labelled as untrusted. The model can move an ask to "needs you", never to "approved".
- **Prompt injection in mail** is the main risk. Mail text goes to the model wrapped and labelled as untrusted. There is no forward tool. Every send needs my tap on a card that shows the real recipient and body. A new recipient gets a red warning on the card.
- **No other way out:** each runtime runs with its built-in tools off (or locked down, section 8.1). Only bleep's MCP tools.
- **Local only:** MCP server binds to `127.0.0.1` with a bearer token. No other port opens.
- **Secrets:** Keychain only. Never in logs, never in prompts.
- **Audit:** every tool call, approval and model call is in SQLite and visible in a settings "Activity" tab.

### 8.11 Agent watch (Claude Code and Codex sessions)

This is about my own coding sessions, not bleep's brain. bleep's own runtime (8.1) runs with `--setting-sources ""`, so it never triggers these hooks.

**Seeing who waits**
- **Claude Code:** bleep installs hooks in my user settings (`~/.claude/settings.json`), with my consent and a one-click remove. `Notification` fires when a session waits for permission or sits idle. `Stop` fires when a turn ends. Each hook runs a tiny `bleep-hook` binary that sends the hook JSON to `~/.bleep/bleep.sock`.
- **Codex:** my Codex sessions share one local app-server daemon (`codex agents` lists them). bleep connects as one more JSON-RPC client and receives approval requests for commands and patches. Codex's `notify` setting already points at Codex Computer Use, so bleep chains after it and never replaces it.
- **Context:** transcripts are JSONL (`~/.claude/projects/`, `~/.codex/sessions/`). bleep reads the last turn, the ask, test output and any screenshot. Read-only.

**Deciding**
- **Rules first:** `agent-rules.toml` has two lists: `auto` (tests, lint, typecheck, file reads, git status and diff, screenshot checks) and `ask` (push, deploy, delete, `rm`, secrets and `.env`, network installs, payments, anything outside the repo). Anything on neither list asks.
- **Checks before an auto-approve:** the command matches an `auto` rule exactly, runs inside the session's repo, and touches no `ask` pattern. For "looks right?" questions with a screenshot, a `small` one-shot compares the image with the original ask and answers yes or no with a reason. A "no" or a low-confidence "yes" goes to me.
- **The model can only make it stricter** (D12).

**Acting**
- **Permission prompts (clean):** the `PreToolUse` or `PermissionRequest` hook waits for bleep's answer and returns allow or deny. Codex approvals are answered over the app-server.
- **Fail closed to me:** if bleep is not running, slow (over 20 s), or unsure, the hook returns no decision and the normal prompt shows in the terminal. bleep never approves by default.
- **Idle questions (harder):** a live terminal has no API for typing a reply. The `Stop` hook can return a reason that continues the session, but only at that moment. Later replies go through `claude --resume` or the Codex app-server, or wait for me.
- **Everything else:** goes on my list with a one-line summary, and shows in the chat, the droid bubble and the next check-in ("2 agents need you").

**Where it shows up:** an "Agents" view in the chat panel (waiting, handled, needs you), approval cards for queued asks, and a log of every auto-approve with the rule that allowed it.

---

## 9. Phases

Each phase ends with something I run every day. Each later phase needs the one before it.

### Phase 1. Droid and chat

Deliverables:
- Tauri 2 app scaffold in `app/`, builds and runs with `cargo tauri dev`.
- Droid window (see-through, on top, all Spaces, draggable, tuck to edge, position saved).
- Tray menu (show or hide, open chat, pause, quit).
- ⌥⌥ summon with the Input Monitoring prompt and the ⌥Space fallback.
- `AgentRuntime` trait and the Claude Code backend (section 8.1).
- Chat panel with streaming replies.
- MCP server with one harmless tool (`bleep_now`: current date, time and time zone) to prove the tool path end to end.
- SQLite store with `messages` and `sessions`. History survives restarts. The session resumes.
- First-run check: CLI installed, signed in to `~/.bleep/claude-home`. Clear message if not.
- Launch at login.

Done when: I can double-tap ⌥ anywhere, ask "what time is it in Tokyo", see the `bleep_now` tool step, get an answer, quit the app, reopen it, and see the whole conversation still there.

### Phase 2. Calendar and the meeting bar

Deliverables:
- Google sign-in (section 8.4) with bleep's built-in client: Connect Google in settings. Own client as an advanced option.
- Calendar connector: `calendar_list`, `calendar_next`, `calendar_free` (read), `calendar_create`, `calendar_update`, `calendar_delete`, `calendar_respond` (write).
- Approval cards in the chat and the waiting write gate (section 8.2).
- Calendar watcher + cache + scheduler timers.
- Meeting bar at 60 and 10 min before, with Prep me, Snooze, Join.
- Droid alert state and bubble when the bar appears.

Done when: the bar shows up 1 hour before a real meeting on my calendar, "Prep me" answers with a short prep note, and "add a reminder to pay rent at 6 PM" shows an approval card and creates the event only after I approve.

### Phase 3. Gmail and check-ins

Deliverables:
- Gmail connector: `gmail_search`, `gmail_read_thread` (read), `gmail_draft` (safe), `gmail_send` (write).
  - **Rest of Phase 3 (13 Nov 2026):** Gmail watch every 2 minutes, rules-then-small-model sorting, bill amounts and due dates, waiting-on-me, calm pings with "Not useful" and mute suggestions, check-ins at 9:00, 13:00 and 18:00 (empty ones skipped, late ones run once within 2 hours), and the Activity tab. VIPs and muted senders live in `config.toml` until Phase 5.
  - **Built early (12 Nov 2026, with Phase 2):** all four tools. `gmail_send` shows a card with the exact email, warns about recipients I have never mailed, and stops after 10 sends a day. Replies keep the thread (`In-Reply-To`, `References`). The rest of Phase 3 (watcher, triage, check-ins) is still to do.
- Gmail watcher (history polling) + rules triage in Rust + `small` one-shot for unclear mail.
- Scheduled turns: morning brief 9:00, check-ins 13:00 and 18:00 (times in settings).
- Bill and due-date detection, with an offer to add a calendar reminder.
- "Waiting on me" detection (threads where the last message is to me and is over 24 hours old).
- Calm rules: quiet hours, ping budget, dedup, pause, "not useful".
- Activity tab (tool calls, approvals, model calls).

Done when: a real bill mail leads to a reminder offer at the next check-in, a draft reply lands in Gmail drafts, the send happens only after I approve the card, and a promo mail never pings me.

### Phase 4. Agent watch: Claude Code and Codex

Status (13 Nov 2026): built on branch `phase-4-agent-watch` (plan: `docs/superpowers/plans/2026-11-13-phase-4-agent-watch.md`). Auto-approve and the Codex watch are off until turned on in Settings. Screenshot "go on" is Claude Code only for now.

Deliverables:
- `bleep-hook` binary and the local socket. Install and remove the Claude Code hooks from settings, with a clear consent screen.
- Codex app-server client for my own sessions, chained after the existing `notify` program.
- Session list from hooks and transcripts: waiting, running, done. An "Agents" view in the chat panel.
- Step 1, read-only: waiting sessions go on my list with a summary. Droid bubble and check-ins say "N agents need you". No approvals yet.
- Step 2, approvals: `agent-rules.toml`, the checks, allow or deny through the hook and the app-server, fail closed to the normal prompt.
- Screenshot check with a `small` one-shot.
- Log of every auto-approve and denial in the Activity tab.

Done when: I leave four sessions running and walk away. When I come back, `npm test` was approved and passed, a matching screenshot got "go on", a `git push --force` is waiting for me with a summary, and the terminal still shows the normal prompt for anything bleep was not sure about.

### Phase 5. Memory and compaction

Status (15 Nov 2026): merged (#15). Built on branch `phase-5-memory-and-compaction` (plan: `docs/superpowers/plans/2026-11-15-phase-5-memory-and-compaction.md`). The carry-over after compaction goes in the new session's system prompt, not a first message. Memory changes reach a running session as a `[memory changed]` block on the next message.

Deliverables:
- `~/.bleep/memory/` files, loaded into the system prompt.
- `memory_search` (read), `memory_note` (safe), `memory_promote` (write).
- Nightly pattern proposals.
- Memory view in settings with "forget".
- Compaction (section 8.6) with the divider in the chat, and the context meter in the panel header.

Done when: I can tell it "Sync means my 1:1 with Priya" and it uses that a week later, and a forced compaction (threshold set low for testing) keeps the chat working with no lost open loops.

### Phase 6. Codex runtime

Status (15 Nov 2026): skipped for now. Phase 7 came first.

Deliverables:
- Codex backend for `AgentRuntime` (section 8.1): app-server for chat, `codex exec --json` for one-shots, bleep's MCP server passed in by config override.
- Built-in Codex tools off or locked down.
- Runtime picker in settings, with the switch path (summary plus last 10 turns into a fresh session).
- Tier mapping for Codex models in `config.toml`.

Done when: I switch to Codex in settings, the chat carries on with context, the 13:00 check-in runs on Codex, and an email send still shows the same approval card.

### Phase 7. Remote MCP servers

Status (15 Nov 2026): built on branch `phase-7-remote-mcp` (plan: `docs/superpowers/plans/2026-11-15-phase-7-remote-mcp.md`). Changed on 15 Nov 2026: instead of a Dayblocks connector inside bleep, bleep can add any MCP server by URL. The Dayblocks MCP server (with its own auth) is built in the Dayblocks repo, and bleep reaches it through this feature.

Deliverables:
- Settings, Connectors tab: add a server by URL, see every connector's status, sign in again, refresh tools, set each tool to Ask first, Run at once or Off, remove.
- Auth discovery like Claude Code: no auth, a pasted token, or OAuth (RFC 9728 resource metadata, RFC 8414 server metadata, RFC 7591 registration or a typed client ID, PKCE in the browser, refresh, revoke on remove). Secrets in the Keychain.
- Remote tools go through bleep's own MCP server and the approval gate. Only `readOnlyHint` tools (or ones I set) run without a card. Descriptions and results are marked untrusted.
- A changed tool set starts a fresh session with the chat carried over in code.

Done when: I add the Dayblocks MCP URL in Settings, sign in through whatever auth it offers, and the chat reads my tasks and moves one after I approve the card. The to-do check-ins (13:00 and 18:00 with open tasks) wait for a later phase.

### Phase 8. 3D droid

Deliverables:
- A 3D droid model (GLB) with idle, think, alert and happy animations, rendered with three.js in the droid window.
- Same behaviour as the 2D droid. The 2D droid stays as a low-power fallback (setting, and on battery under 20%).
- Update the landing page with the 3D droid.

Done when: the 3D droid idles at under 3% CPU on my M1 and reacts to every state the 2D droid has.

### Phase 9. Bring your own key

Status (15 Nov 2026): built on branch `phase-9-byok` (plan: `docs/superpowers/plans/2026-11-15-phase-9-byok.md`). Settings has a Model page: Claude Code or an API key, with provider, key, base URL and a main and small model. Gemini goes through Google's OpenAI-compatible endpoint, so two adapters cover every provider. The cap stops proactive work only; chat keeps working.

Deliverables:
- API backend for `AgentRuntime` (section 8.1): bleep's own tool loop, Anthropic and OpenAI-compatible formats first, then Gemini.
- Provider picker, key field (Keychain), base URL field, tier mapping, and a "test key" button in settings.
- Cost estimate in the Activity tab and a daily spend cap.

Done when: with Claude Code and Codex both signed out, I paste an OpenRouter key, pick a model, and the chat, the 13:00 check-in and an approved email send all work.

Later, if bleep is shared with other people (not planned): each user needs their own Google sign-in through a Google-verified app (Gmail restricted-scope review), a signed and notarized build, and a first-run setup that does not assume my machine. BYOK is the runtime path that makes sharing possible, because subscription logins are for personal use only.

---

## 10. Testing

- **Rust unit tests:** each runtime's stream parser (recorded CLI output as fixtures), the normalised event mapping, policy gate, triage rules, scheduler timing, compaction trigger.
- **Runtime contract tests:** one test suite that every `AgentRuntime` backend must pass against recorded fixtures (stream a reply, call `bleep_now`, report usage, report a rate limit).
- **Connector tests:** recorded Google API responses as fixtures. No live calls in tests.
- **Agent watch tests:** recorded hook payloads and Codex app-server messages as fixtures. A rule test suite with known-bad asks (force push, `rm -rf`, `.env` reads, commands outside the repo) that must never auto-approve.
- **UI tests:** Playwright with headless Chromium against the Vite dev server. Never open my own browser.
- **Manual check per phase:** the "done when" line above, on my real accounts.
- **Where tests run (decided 12 Nov 2026):** locally, run only the tests for the feature being changed (for example `cargo test --lib connectors::gmail`, `npx vitest run src/chat/approvalView.test.ts`, `npx playwright test e2e/approval.spec.ts`). The full suites run in CI on every pull request (`.github/workflows/ci.yml`). A red PR check is fixed on the branch before merge.

## 11. Risks

- **CLI flags and protocols change.** Both CLIs update often. Mitigation: version check at start, flags in one place per backend, a smoke test that runs one turn, contract tests.
- **Plan limits.** bleep shares them with my coding. Mitigation: rules first, the `small` tier for background work, max 20 model calls an hour, pause on limit.
- **Google review.** Unverified, the app warns users and caps at 100 of them; Gmail scopes need a paid yearly assessment to go public. Mitigation: decide before a public launch whether Gmail ships in the public build.
- **Input Monitoring prompt.** macOS may reset it after each rebuild if the app is not signed with a stable identity. Mitigation: sign local builds with my local signing identity (worked for ScreenRecz).
- **See-through windows on macOS.** Tauri needs `macOSPrivateApi` for transparency. Fine for a personal app that never goes to the App Store.
- **Sleep.** Nothing runs while the Mac sleeps. Mitigation: catch up on wake; skip reminders for meetings that already ended; say "Back online" if the gap was over 2 hours.
- **API spend (BYOK).** Unlike subscriptions, API calls cost money per token. Mitigation: cost meter, daily cap, `small` tier for background work.
- **Codex built-in tools.** If the off switches do not work on the installed version, the shell tool stays in a read-only, no-network sandbox. Mitigation: verify first in Phase 6; if it is not safe enough, Codex stays off.
- **Prompt injection.** See 8.10. No send without my tap.
- **Hooks and the Codex protocol change.** Mitigation: version check, a smoke test per release, and fail closed to the normal prompt.
- **A wrong auto-approve.** Mitigation: short `auto` list by default, rules decide, the known-bad test suite, and a log I can read.

## 12. Left to the builder

- Exact crate versions, module split inside `src-tauri/`, and React state library.
- Exact CLI flag spellings and protocol messages (verify against the installed `claude --help`, `codex --help` and the Codex source).
- Window sizes, animation timings and copy inside the app, within the landing page style.
- SQLite migration tool.
