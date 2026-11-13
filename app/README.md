# bleep app

The macOS app. Tauri 2 (Rust core) with React web views. Plan: `../docs/build-plan.md`.

## Run

```bash
npm install
npm run tauri dev
```

The droid appears bottom right. The menu bar icon has Show or hide droid, Open chat, Pause, Launch at login (release builds only), Settings and Quit.

## Sign bleep's Claude in (once)

bleep uses its own Claude Code login, kept in `~/.bleep/claude-home`, separate from your terminal's. Click **Sign in** in the chat, or run:

```bash
CLAUDE_CONFIG_DIR=~/.bleep/claude-home claude auth login
```

## Connect Google

Open **Settings…** from the menu bar icon and press **Connect Google**. Your browser opens Google's sign-in once. That is all a user does.

bleep's own Google app is built in. Its client ID and secret come from two variables at build time (see Build). The sign-in token goes in the macOS Keychain (service `site.avirooppaul.bleep`). bleep asks for calendar events, Gmail read and Gmail drafts, so Phase 3 needs no second sign-in.

Developers can use their own Google Cloud app instead: Settings, **Use your own Google Cloud app**. Its ID and secret go in `~/.bleep/config.toml` (mode 600). **Use bleep's app** switches back.

### Set up bleep's Google app (developer, once)

1. In Google Cloud, enable the Google Calendar API and the Gmail API.
2. Set up the consent screen (Google Auth Platform): app name, support email, and on **Audience** set it to **In production**. In "Testing", Google expires sign-ins every 7 days.
3. Create an OAuth client of type **Desktop app**.
4. Add to `~/.config/zsh/secrets.zsh`:

   ```bash
   export BLEEP_GOOGLE_CLIENT_ID="…"
   export BLEEP_GOOGLE_CLIENT_SECRET="…"
   ```

Until Google verifies the app, users see an "unverified app" warning and Google allows at most 100 of them.

## Calendar and the meeting bar

- **Ask in the chat:** "what is on today", "find me 30 free minutes tomorrow", "add a reminder to pay rent at 6 PM".
- **Changes need your tap:** adding, changing, deleting an event or replying to an invite shows a card with the exact change. Nothing happens until you press Approve. A card left for 10 minutes expires.
- **Meeting bar:** 60 and 10 minutes before a meeting, a slim bar shows under the menu bar, and the droid lights up. Prep me asks bleep for a short note. Snooze hides it until 10 minutes before. Join opens the meeting link. The bar skips all-day events and meetings you declined. Pause in the menu stops it.

## Mail

- **Ask in the chat:** "anything unread from today", "what did Priya say about the deck", "draft a reply saying Friday works".
- **Drafts are free:** bleep saves a reply in your Gmail drafts without asking. Nothing is sent.
- **Sending needs your tap:** a card shows the recipients, subject and full body. Someone you have never emailed gets a red warning. At most 10 sends a day.

## Watching mail and check-ins

- **Every 2 minutes** bleep checks Gmail for new mail. The first check only reads the last 2 days and never pings.
- **Sorting:** rules first (promotions, social and forums are ignored; bill words plus an amount mean a bill; VIPs are important; mailing lists are worth knowing). Only unclear mail goes to the small model.
- **Pings:** the droid's bubble says "Bill due today: City Power ₹2,340" or "Boss needs a reply". Never between 23:00 and 08:00, at most 4 an hour, once per thread every 6 hours. Pause in the menu stops them. Tap "Not useful" three times for one sender and Settings suggests muting them.
- **Check-ins** at 9:00, 13:00 and 18:00 (times in `config.toml` under `[checkin]`): meetings, bills due within 3 days, and threads waiting over a day for your reply, with offers to add a reminder or draft a reply. Nothing to say means no check-in.
- **Settings, Mail tab:** VIPs, muted senders, mute suggestions. **Activity tab:** every tool call, approval card and model call.

## Watching coding agents

- **Turn it on:** Settings, Agents tab. "Install hooks" adds bleep's entries to `~/.claude/settings.json` after a second tap ("Yes, install"); a backup is saved next to it first, and "Remove hooks" takes only bleep's entries out. For Codex, "Install turn notices" puts `~/.bleep/bin/bleep-hook` in front of your `notify` program (which still runs first), and "Watch Codex sessions" connects to the Codex background service when it runs. bleep never starts that service.
- **Watch only by default:** sessions that wait on you show in the chat panel's Agents view and in the droid's bubble ("shop: wants to run git push --force", or "2 agents need you"), and check-ins mention them.
- **Approvals (off until you turn on "Approve safe asks by my rules"):** rules in `~/.bleep/agent-rules.toml` decide. Tests, lint, typecheck, `git status` and diff, and reads inside the repo are allowed; anything with `git push`, `rm`, `sudo`, `.env`, network installs, paths outside the repo, flags that run or write things (`--pre`, `-exec`, `--config`, `--output`, `--fix`), secret files, or shell tricks (`$(...)`, globs, redirects, `&`) asks you. The working folder must be a real repo folder, not `/` or your home folder. Anything bleep does not approve keeps its normal prompt in the terminal. Codex asks also get Approve and Deny buttons in bleep.
- **Screenshots:** when a Claude Code session ends a turn with a screenshot and "Does this look right?", a small model compares it with your ask. Only a confident match says "go on", at most twice an hour per session.
- **Fail closed:** if bleep is closed or takes over 20 seconds, the hook prints nothing and the agent shows its normal prompt.

## Build

```bash
source ~/.config/zsh/secrets.zsh && npm run tauri build
```

The two `BLEEP_GOOGLE_CLIENT_*` variables are compiled in. Without them the build still works, and Settings asks for your own Google app. Output: `src-tauri/target/release/bundle/macos/bleep.app`, signed with the local "ScreenRecz Local Signing" identity so Input Monitoring survives rebuilds. Copy it to `/Applications`.

## Summon

Double tap Option anywhere. This needs System Settings, Privacy and Security, Input Monitoring, with bleep switched on, then a restart of bleep. Until then, Option Space opens the chat.

## Tests

```bash
cd src-tauri && cargo test     # Rust: store, runtime, parser, MCP, approvals, Google, calendar, scheduler, chat, windows, hotkey
cd bleep-hook && cargo test    # the hook binary: never blocks an agent
npm test                       # vitest: chat reducer, approval cards, bar text, agents text
npm run e2e                    # Playwright, headless Chromium, mocked backend
```

## Data

Everything lives in `~/.bleep/` (`BLEEP_HOME` overrides it): `bleep.db`, `config.toml`, `claude-home/`, `claude-work/`, `run/` (MCP config with a per-launch token and `bleep.sock` for the hooks, mode 700), `bin/bleep-hook`, `agent-rules.toml`, `logs/`.
