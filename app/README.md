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
npm test                       # vitest: chat reducer, approval cards, bar text
npm run e2e                    # Playwright, headless Chromium, mocked backend
```

## Data

Everything lives in `~/.bleep/` (`BLEEP_HOME` overrides it): `bleep.db`, `config.toml`, `claude-home/`, `claude-work/`, `run/` (MCP config with a per-launch token, mode 700), `logs/`.
