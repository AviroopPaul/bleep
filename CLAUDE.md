# bleep

Floating macOS assistant. Plan and decisions: `docs/build-plan.md`. App: `app/` (Tauri 2, Rust core in `app/src-tauri`, React views in `app/src`). Landing page: `landing/` (Vercel). Its text pages (vs, alternatives, Claude Code, changelog, privacy) and `sitemap.xml` come from `scripts/seo/pages.py`: edit copy there and rerun it after each release. SEO/GEO plan: `docs/seo-audit.md`.

## Tests

- **Locally, run only the tests for what you changed.** Rust: `cd app/src-tauri && cargo test --lib <module>`. Web: `cd app && npx vitest run <file>`, `npx playwright test <spec>`.
- **Do not run the full suites locally.** CI runs them on every pull request (`.github/workflows/ci.yml`). If a PR check fails, fix it on the branch.
- Tests never call Google or open a browser. Google APIs are mocked (`src-tauri/src/google/mock.rs`, fixtures in `src-tauri/tests/fixtures/google/`). Playwright runs headless with mocked IPC (`?mock=1`).
- Voice tests that need the speech model on disk are `#[ignore]`: `cargo test --lib voice -- --ignored --nocapture`. They speak with macOS `say`, so run them on a Mac. `cargo run --example voice_live` checks the real mic (it plays the phrase through the speakers).

## Build

`source ~/.config/zsh/secrets.zsh && npm run tauri build` in `app/`. The secrets file gives `BLEEP_GOOGLE_CLIENT_ID` and `BLEEP_GOOGLE_CLIENT_SECRET`, compiled in as bleep's Google app. Never print their values.

**Three apps side by side** (`src-tauri/src/flavor.rs`). Each has its own name, bundle id, data folder, Keychain item and icon. They share one Claude Code hook: the first running app (prod, then nightly, then local) answers, the others only see session activity.
- **Prod** ("bleep", `~/.bleep`): the release.
- **Nightly** ("bleep (nightly)", `~/.bleep-nightly`, feature `nightly`): `.github/workflows/nightly.yml` builds it from `main` every night and publishes it to the rolling `nightly` prerelease in `AviroopPaul/bleep-releases`. It updates from `latest-nightly.json` only.
- **Local** ("bleep (local)", `~/.bleep-local`, feature `local`): `npm run build:local` in `app/` builds and installs `/Applications/bleep (local).app`. `npm run dev:local` runs it without bundling (hot reload). It never updates itself and never sets a login item. Use it to test unreleased work.

## Copy

No em dashes in any user-facing text, prompts included.
