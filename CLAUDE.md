# bleep

Floating macOS assistant. Plan and decisions: `docs/build-plan.md`. App: `app/` (Tauri 2, Rust core in `app/src-tauri`, React views in `app/src`). Landing page: `landing/` (Vercel).

## Tests

- **Locally, run only the tests for what you changed.** Rust: `cd app/src-tauri && cargo test --lib <module>`. Web: `cd app && npx vitest run <file>`, `npx playwright test <spec>`.
- **Do not run the full suites locally.** CI runs them on every pull request (`.github/workflows/ci.yml`). If a PR check fails, fix it on the branch.
- Tests never call Google or open a browser. Google APIs are mocked (`src-tauri/src/google/mock.rs`, fixtures in `src-tauri/tests/fixtures/google/`). Playwright runs headless with mocked IPC (`?mock=1`).

## Build

`source ~/.config/zsh/secrets.zsh && npm run tauri build` in `app/`. The secrets file gives `BLEEP_GOOGLE_CLIENT_ID` and `BLEEP_GOOGLE_CLIENT_SECRET`, compiled in as bleep's Google app. Never print their values.

## Copy

No em dashes in any user-facing text, prompts included.
