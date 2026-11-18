# bleep

Floating macOS assistant. Plan and decisions: `docs/build-plan.md`. App: `app/` (Tauri 2, Rust core in `app/src-tauri`, React views in `app/src`). Landing page: `landing/` (Vercel). Its text pages (vs, alternatives, Claude Code, changelog, privacy) and `sitemap.xml` come from `scripts/seo/pages.py`: edit copy there and rerun it after each release. SEO/GEO plan: `docs/seo-audit.md`.

## Tests

- **Locally, run only the tests for what you changed.** Rust: `cd app/src-tauri && cargo test --lib <module>`. Web: `cd app && npx vitest run <file>`, `npx playwright test <spec>`.
- **Do not run the full suites locally.** CI runs them on every pull request (`.github/workflows/ci.yml`). If a PR check fails, fix it on the branch.
- Tests never call Google or open a browser. Google APIs are mocked (`src-tauri/src/google/mock.rs`, fixtures in `src-tauri/tests/fixtures/google/`). Playwright runs headless with mocked IPC (`?mock=1`).
- Voice tests that need the speech model on disk are `#[ignore]`: `cargo test --lib voice -- --ignored --nocapture`. They speak with macOS `say`, so run them on a Mac. `cargo run --example voice_live` checks the real mic (it plays the phrase through the speakers).

## Build

`source ~/.config/zsh/secrets.zsh && npm run tauri build` in `app/`. The secrets file gives `BLEEP_GOOGLE_CLIENT_ID` and `BLEEP_GOOGLE_CLIENT_SECRET`, compiled in as bleep's Google app. Never print their values.

**Dev build:** `npm run build:dev` in `app/` builds "bleep (dev)" (cargo feature `dev`, `src-tauri/tauri.dev.conf.json`) and installs it to `/Applications/bleep (dev).app`, next to production. It has its own data (`~/.bleep-dev`), Keychain item and bundle id, never updates itself, and never touches the Claude Code hooks. See `src-tauri/src/flavor.rs`. Use it to test unreleased work.

## Copy

No em dashes in any user-facing text, prompts included.
