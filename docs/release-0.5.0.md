# bleep 0.5.0: feature list

Status: draft, open for more features · 17 Nov 2026 · owner: Aviroop

**Release gate:** 0.5.0 does not ship until Aviroop signs off in writing (section 9). Features can merge to `main` before that. No version bump, tag, GitHub release, `bleep-releases` upload or Homebrew tap change happens until sign-off.

Current version: 0.3.5. Background and decisions: `docs/build-plan.md`.

---

## 1. Summary

| # | Feature | Where it shows | New model calls |
|---|---|---|---|
| F1 | After-meeting follow-up | Droid bubble, chat | Only when I tap "Draft recap" |
| F2 | Travel cards | Chat card, bar, check-ins | One `small` call per booking mail |
| F3 | Connector marketplace, with Google Drive built in | Settings > Connectors | None (built, see 4) |
| F4 | Usage page (replaces the context bar) | Settings > Usage, opened from the gear in the chat header | None (built, see 5) |
| F5 | Native reminders | Reminders tab, bubble, macOS notification, chat | None to fire; the main chat sets them (built, see 6) |
| F6 | Files on this Mac | Settings > Connectors, chat | None (built, see 8) |
| F7 | PDF reading (Drive and Mac files) | Chat | None (built, see 8) |
| F8 | Offer a connector in chat | Chat card | None; the main chat calls a tool (built, see 8) |

All features keep the calm rules: quiet hours, ping budget, dedup, pause, and "Not useful".

---

## 2. F1. After-meeting follow-up

**Why:** after a meeting, the follow-up mail and the notes are what I forget.

**What it does**
- **Trigger:** 2 minutes after a meeting ends (from `cal_cache.end`), the droid shows a bubble: "Call with Priya ended. Follow up?"
- **Buttons:** "Draft recap", "Note to memory", "Skip".
- **Draft recap:** runs one main chat turn. bleep asks me 1 or 2 short questions ("Any decisions or next steps?"), then saves a draft with `gmail_draft` (safe). Sending still needs the approval card.
- **Note to memory:** a small text box. The text goes to today's daily note with `memory_note`, tagged with the meeting title and the people.
- **Which meetings:** only events with at least one other attendee that I accepted. Skip all-day events, declined events, focus blocks and events with no attendees.

**How**
- New cue kind `Ended` in `proactive/cues.rs`, next to `Hour` and `TenMin`. Built from the calendar cache. No model call.
- Dedup per event. If I am in another meeting right away, hold the bubble until that one ends, then merge.
- Settings > Mail and calendar: on or off, and a "only meetings with outside people" switch.

**Done when:** a real 1:1 ends, the bubble shows within 3 minutes, "Draft recap" puts a draft in Gmail, and a focus block or an all-day event never shows a bubble.

**Tests:** cue timing and the meeting filter in Rust (`cargo test --lib proactive::cues`), the bubble buttons in Playwright with mocked IPC.

---

## 3. F2. Travel cards

**Why:** flight times, check-in windows and PNRs sit in mail. I look for them at the last minute.

**What it does**
- **Find bookings:** the mail watcher spots booking mail (airlines, trains, hotels, travel sites).
- **Card in the chat:** route, date, local departure time, flight or train number, PNR, terminal if known. Buttons: "Add to calendar" (approval card), "Open mail".
- **Reminders:**
  - **Check-in open:** "Web check-in for 6E 2341 is open." Default is 48 h before departure for flights. A per-airline table can change it.
  - **Leave by:** a bar like the meeting bar. Leave time = departure minus a buffer (2 h domestic, 3 h international) minus my travel time to the airport (a setting, default 45 min).
  - **Day before:** the trip shows in the morning brief.
- **Changes:** a mail about a delay, a new gate or a cancellation updates the same card and pings once.

**How**
- **Rules first:** known travel senders and words like "PNR", "e-ticket", "boarding pass", "booking confirmed". Only matching mail goes to the model.
- **Extract:** one `small` one-shot per mail with a JSON schema (kind, carrier, number, from, to, depart and arrive with time zone, PNR, booking ref). Mail text is labelled untrusted.
- **Store:** new table `trips` (id, kind, ref, payload, depart_at, source_thread, status, updated).
- **Times:** keep the airport's local time and time zone. Show both if they differ from my time zone.
- **No new Google scope.** `gmail.readonly` is enough.

**Done when:** a real flight booking mail creates a card, the check-in reminder and the leave-by bar fire at the right times, and a promo mail from an airline never makes a card.

**Tests:** rules and the extract parser with recorded mail in `tests/fixtures/google/`, time math with time zones, the card in Playwright.

**Open:** travel time from a maps API is for later. 0.5.0 uses a fixed setting.

---

## 4. F3. Connector marketplace

**Why:** adding a server by URL works, but I have to find the URL first. People should pick from connectors we set up and tested, turn each on or off, and sign in.

**What it does**
- **Settings > Connectors becomes a catalog.** Every connector we configured is listed with an icon, a one-line description, an on or off switch, and its sign-in state.
- **Off by default, except the basics.** Date and time, memory and reminders are always on. Everything else is off until the person turns it on.
- **Turn on, then sign in.** Turning a connector on starts its sign-in (Google, OAuth or a token). Turning it off removes its tools from the model and stops its watcher. The sign-in can stay, so turning it back on is one click.
- **Native connectors (in bleep's code):**
  - **Gmail** and **Google Calendar** (today's connectors, now with a switch each).
  - **Google Drive (new):** `drive_search` and `drive_recent` (read), `drive_read` (read: Docs and Slides as text, Sheets as CSV, plain text files and PDFs (F7); cut at 60,000 characters). Other files return their link. File text is marked untrusted, like mail.
  - All Google connectors share one Google sign-in. Turning one on asks Google only for the scope it needs (incremental consent), so a person who only wants Calendar never grants mail or Drive access.
- **Remote connectors (MCP servers we configured):** Linear, Notion and GitHub (with a pasted token) to start. Slack is left out until its MCP server and sign-in are confirmed. We set the URL, auth type and default tool modes (read tools run at once, write tools ask first). They go through the same `remote::manager` path as a pasted URL.
- **Add by URL** stays, under "Advanced", for servers we did not configure.

**How**
- **Catalog:** a static list in Rust (`connectors/catalog.rs`): id, name, kind (native or remote), description, icon, Google scopes or URL and auth, default tool modes. The Connectors tab renders it.
- **State:** `config.toml` keeps which connectors are on. A changed tool set already starts a fresh session with the chat carried over (Phase 7).
- **Google scopes:** today's sign-in asks for `calendar.events`, `gmail.readonly` and `gmail.compose` at once. Move to per-connector scopes with `include_granted_scopes`. Drive uses `drive.readonly`.
- Verify each remote URL and its auth at build time, and record the result in this doc.

| Connector | Kind | URL or scope (verify at build) | Auth |
|---|---|---|---|
| Gmail | native | `gmail.readonly`, `gmail.compose` | Google |
| Google Calendar | native | `calendar.events` | Google |
| Google Drive | native | `drive.readonly` | Google |
| Linear | remote | `https://mcp.linear.app/mcp` | OAuth (tested in Phase 7) |
| Notion | remote | `https://mcp.notion.com/mcp` | OAuth |
| Slack | remote | to confirm | to confirm |
| GitHub | remote | `https://api.githubcopilot.com/mcp/` | OAuth or token |

**Status (17 Nov 2026):** built on branch `release-0.5.0-usage-reminders`. Existing setups keep Gmail and Calendar on. The Google tab still holds the account and the client; Connect there asks for the connectors that are on (Gmail and Calendar when none are).

**Before Drive works for real:** in bleep's Google Cloud project, enable the Google Drive API and add the `drive.readonly` scope to the OAuth consent screen. Until then Google refuses the Drive sign-in.

**Done when:** a fresh install shows the catalog with Google connectors off; I turn on Calendar and Drive, sign in once, and Google asks only for those scopes; the chat finds a Drive doc by name and reads it; turning Drive off removes its tools; Linear turns on and lists my issues.

**Tests:** catalog shape and default modes in Rust, scope sets per connector, Drive tools with recorded API fixtures (`tests/fixtures/google/`), the catalog and switches in Playwright.

**Open:**
- `drive.readonly` is a restricted Google scope, like the Gmail ones: a public launch needs Google's verification and the paid security assessment. `drive.file` needs no assessment but only sees files bleep created or the person picked. Proposed: `drive.readonly` for now, decide before a public launch.
- Which connectors are on by default for a new install.

---

## 5. F4. Usage page

**Why:** the thin context bar under the chat header is hard to read and says little. I want one place for usage.

**What changes**
- **Remove** the context meter bar from the chat panel header (`app/src/chat/ChatPanel.tsx`, the `.meter` element).
- **Add** a "Usage" page. It first shipped as a chat tab. It now lives in Settings > Usage, and a gear in the chat header opens Settings.
- **Header room:** four tabs do not fit next to the "bleep" word at 400 px, so the droid head is the brand mark and the word is for screen readers only.
- **Warning kept:** when the chat context passes 80% of the compaction threshold, the gear shows a small dot and opens Settings > Usage. Nothing else in the header.

**What the tab shows**
- **Range:** Today, 7 days, 30 days.
- **Top tiles:** model calls, tokens in and out, cost (API key only), chat context used vs the compaction point.
- **Activity heatmap:** model calls by hour of day and day of week. Shows when bleep works and if it is too busy at night.
- **Tokens per day:** stacked bars by purpose (chat, triage, check-in, compaction, screenshot check, travel).
- **By model:** a table of runtime, model, calls, tokens, cost.
- **Tools:** the most used tools, with counts and fail rate.
- **Agents:** asks auto-approved, approved by me, denied, and timed out.

**How**
- **Data:** `llm_calls`, `api_usage`, `tool_calls`, `agent_asks`, `messages`, `sessions`. All local. No new data leaves the Mac.
- **Models:** `llm_calls` has no model column. Claude Code rows are named from the current tier mapping (main and small); API key rows use the exact model from `api_usage`. No migration.
- **Tokens:** only chat turns and API key calls report tokens. Other Claude Code one-shots count as calls only.
- **Query:** one Tauri command `usage_summary(range)` that returns ready-made series. The view only draws.
- **Charts:** small SVG components, no chart library, to keep the bundle light. Colors from `theme.css`, light and dark.
- **The Activity tab in Settings** stays as the detailed log. Usage is the overview.

**Done when:** the header has no bar, Settings > Usage shows 7 days of real data, and the heatmap matches the `llm_calls` rows.

**Status (17 Nov 2026):** built on branch `release-0.5.0-usage-reminders`. Later: a plan-limit history (needs `RateLimited` events saved).

**Tests:** the summary queries in Rust with a seeded test DB, the chart math in Vitest, the tab in Playwright with mocked IPC.

---

## 6. F5. Native reminders

**Why:** today a reminder is a Google Calendar event, which needs Google, an approval card, and clutters my calendar. "Remind me to call mom at 7" should just work.

**What it does**
- **Set:** in the chat, "remind me at 7 to call mom" or "remind me in 20 min to check the oven". Also a quick-add field in a Reminders list.
- **Fire:** at the time, a bubble shows over the droid, a macOS notification shows too (so I see it when the droid is hidden), and the line goes into the chat.
- **Actions:** "Done", "Snooze 10 min", "Snooze 1 h", "Tomorrow".
- **Repeat:** daily, weekdays, weekly, monthly.
- **List:** a Reminders tab (bell) in the chat panel: due ones with Done and Snooze, upcoming ones, and the last 10 done. The bell shows how many are due.
- **Bills:** the check-in's "add a reminder for this bill" offer creates a native reminder instead of a calendar event (calendar stays an option).
- **Quiet hours:** a reminder I set myself fires even in quiet hours. It is not a proactive ping and does not use the ping budget.

**How**
- **Tools:** `reminder_create`, `reminder_update`, `reminder_delete` as `safe` (local, easy to undo), `reminder_list` as `read`. Served by bleep's MCP server, so they work under every runtime.
- **Store:** new table `reminders` (id, text, due_at, tz, repeat, status, snoozed_until, source, created_at).
- **Timer:** its own task sleeps until the next due reminder (at most 20 s, so sleep and clock changes are caught). After sleep or a restart, a reminder missed by less than 2 hours fires marked late; an older one only posts a "Missed reminder" line in the chat and waits under Due.
- **macOS notification:** `tauri-plugin-notification`. bleep asks for permission the first time I set a reminder.

**Done when:** "remind me in 2 minutes to stretch" fires a bubble and a macOS notification on time, snooze works, a daily repeat fires the next day, and a reminder set before a restart still fires.

**Tests:** time parsing and repeat math in Rust, the missed-on-wake path, the list and actions in Playwright.

**Status (17 Nov 2026):** built on branch `release-0.5.0-usage-reminders`.

**Open:** sync with Apple Reminders (EventKit) so reminders show on my phone? It needs a Reminders permission and Swift or `objc2` code. Proposed: not in 0.5.0, keep the `reminders` table so a sync can come later.

---

## 7. Dev build: "bleep (dev)"

To test 0.5.0 before sign-off, a dev build installs next to the production app.

- **Build:** `npm run build:dev` in `app/` (with the secrets file sourced). It installs to `/Applications/bleep (dev).app`.
- **Apart from production:** its own bundle id (`site.avirooppaul.bleep.dev`), data folder (`~/.bleep-dev`), Keychain item and single-instance lock.
- **First run:** copies the production database, config, agent rules and memory once, so Usage and the chat have real history. Google, remote MCP and API key sign-ins are not copied (they live in the Keychain); sign in again in the dev build if needed.
- **Shared:** the Claude Code sign-in (`~/.bleep/claude-home`), so no second login. Chat sessions stay separate.
- **Never touches production:** no auto-update, no launch at login, no changes to the Claude Code hooks or the Codex notify setting, no Codex approvals.
- **Starts paused,** so it does not ping or check in twice. Reminders still fire. Resume from the tray.
- **Marked "(dev)" everywhere:** app and window names, tray tooltip and "dev" next to the tray icon, a "dev" badge on the droid and the chat header, notifications, Settings > Updates.

---

## 8. More features

Add new items here with the same shape: why, what it does, how, done when, tests.

### F6. Files on this Mac

**Why:** bills, tickets and notes often sit in Downloads or Documents, not in Drive.

**What it does**
- A local connector in the catalog ("On this Mac"), off by default, no sign-in.
- Terminal-style tools. These run at once, read only:
  - `files_search`: Spotlight, by name or text, PDFs included.
  - `files_recent`: changed in the last 30 days, newest first.
  - `files_list` (like `ls`, up to 4 levels deep), `files_find` (like `find -name`), `files_grep` (like `grep -rn`, text files up to 2 MB).
  - `files_read`: PDFs, Word documents and RTF through macOS `textutil`, text files. `from_line` and `lines` work like `head` and `tail`.
  - No `cd`: every tool takes a folder path, so there is no hidden state.
- **`files_run`:** any zsh command in an allowed folder, like a terminal. It always shows an approval card with the exact command and folder, and a warning that it runs as me. 60 second limit, output cut and marked untrusted. Why a card: a command can do anything, and text in a mail or file could try to trick the model into asking for one.
- Only the folders in `config.toml` `[files] folders` (default `~/Documents`, `~/Desktop`, `~/Downloads`).
- **Never read:** anything outside those folders (symlinks and `..` are resolved first), hidden files and folders, and key or password files (`.pem`, `.key`, `.p12`, `.kdbx`, `id_rsa` and similar). Files over 20 MB are refused.
- File names and text are marked untrusted, like mail.

**Done when:** I turn it on, ask "what is the amount on the electricity bill I downloaded", and bleep finds the PDF and reads the amount.

**Tests:** a fake Spotlight over a temp folder: allowed files only; hidden, key and symlinked files refused; ls, find, grep and line ranges; PDF and RTF read; `files_run` only through the card, only in an allowed folder, with exit code and stderr (`connectors::files`). The command card in `approvalView.test.ts`.

**Open:** a folder picker in Settings. Today the list is changed in `config.toml`.

### F7. PDF reading

**What it does:** `drive_read` and `files_read` pull the text out of PDFs on this Mac (`pdf-extract`, in a blocking thread so a bad PDF cannot crash bleep). A scanned PDF with no text layer says so. Shared code: `connectors/doc_text.rs`.

**Done when:** a PDF bill in Drive and one in Downloads both read in the chat. F2 travel cards use the same code for PDF tickets.

**Open:** text from scanned PDFs (macOS Vision OCR).

### F8. Offer a connector in chat

**Why:** people do not open Settings to find connectors.

**What it does:** an always-on tool, `connector_offer`, lists every catalog connector in its description. When I ask for something a connector that is off would do ("what is on my Linear board"), the model calls it, and the answer shows that connector's card with its switch (and the token field for GitHub). Turning it on there works like Settings. The next message starts a fresh session with the new tools, as for any tool change.

**Done when:** with Linear off, "what is on my Linear board" shows the Linear card in the answer; I turn it on, ask again, and get my issues.

**Tests:** the tool result and description (`marketplace`), the card in chat and its switch (`e2e/offer.spec.ts`), dark mode contrast.

---

## 9. Sign-off

Each feature is checked by hand on my real accounts against its "Done when" line before sign-off.

| Feature | Built | CI green | Checked by hand |
|---|---|---|---|
| F1 After-meeting follow-up | no | no | no |
| F2 Travel cards | no | no | no |
| F3 Connector marketplace and Google Drive | yes | no | no |
| F4 Usage page | yes | no | no |
| F5 Native reminders | yes | no | no |
| F6 Files on this Mac | yes | no | no |
| F7 PDF reading | yes | no | no |
| F8 Offer a connector in chat | yes | no | no |

**Release steps after sign-off only:** follow the bleep release checklist (bleep repo, `bleep-releases`, Homebrew tap). Update the landing page with the new features at the same time.

Signed off by: Aviroop on 18 Nov 2026. F1 and F2 are not built; they move to a later release.
