# bleep app

The macOS app. Tauri 2 (Rust core) with React web views. Plan: `../docs/build-plan.md`.

## Run

```bash
npm install
npm run tauri dev
```

The droid appears bottom right. The menu bar icon has Show or hide droid, Open chat, Pause, Launch at login (release builds only) and Quit.

## Sign bleep's Claude in (once)

bleep uses its own Claude Code login, kept in `~/.bleep/claude-home`, separate from your terminal's. Click **Sign in** in the chat, or run:

```bash
CLAUDE_CONFIG_DIR=~/.bleep/claude-home claude auth login
```

## Build

```bash
npm run tauri build
```

Output: `src-tauri/target/release/bundle/macos/bleep.app`, signed with the local "ScreenRecz Local Signing" identity so Input Monitoring survives rebuilds. Copy it to `/Applications`.

## Summon

Double tap Option anywhere. This needs System Settings, Privacy and Security, Input Monitoring, with bleep switched on, then a restart of bleep. Until then, Option Space opens the chat.

## Tests

```bash
cd src-tauri && cargo test     # Rust: store, runtime, parser, MCP, chat, windows, hotkey
npm test                       # vitest: chat reducer
npm run e2e                    # Playwright, headless Chromium, mocked backend
```

## Data

Everything lives in `~/.bleep/` (`BLEEP_HOME` overrides it): `bleep.db`, `config.toml`, `claude-home/`, `claude-work/`, `run/` (MCP config with a per-launch token, mode 700), `logs/`.
