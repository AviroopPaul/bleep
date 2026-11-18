# Contributing to bleep

Thanks for helping. bleep is a small project run by one person, so short, focused pull requests get merged fastest.

## Before you start

- **Bugs:** open an issue with the bug template. Include your macOS version, bleep version (Settings, About) and the steps.
- **Features:** open an issue first and say what you want to do. A short talk saves you work on something that does not fit.
- **Security issues:** do not open a public issue. See [SECURITY.md](SECURITY.md).

## Set up

```sh
git clone https://github.com/AviroopPaul/bleep.git
cd bleep/app
npm install
npm run tauri dev
```

You need a Mac with Apple silicon on macOS 15+, Node 22+, stable Rust and Xcode Command Line Tools. See [`app/README.md`](app/README.md) for Google sign-in and model setup.

To test next to the released app, run `npm run build:dev`. It installs "bleep (dev)" with its own data in `~/.bleep-dev`.

## Tests

Run only the tests for what you changed. CI runs the full suites on every pull request.

```sh
cd app/src-tauri && cargo test --lib <module>
cd app && npx vitest run <file>
cd app && npx playwright test <spec>
```

- Tests never call Google. Use the mocks in `app/src-tauri/src/google/mock.rs` and fixtures in `app/src-tauri/tests/fixtures/google/`.
- Playwright runs headless with mocked IPC (`?mock=1`).

## Rules for changes

- **Safety first.** bleep never sends mail, moves events, deletes, pays, or runs a command without the user's yes. A change to approval rules needs tests for the risky cases (push, deploy, delete, secrets, money).
- **Data stays local.** No new servers, no telemetry, no secrets in config files or logs.
- **Copy:** short, plain sentences. No em dashes in any user-facing text, prompts included.
- **Keep it light.** Watch idle CPU, memory and battery.

## Pull requests

1. Fork, then branch from `main`.
2. Keep one change per pull request.
3. Add or update tests.
4. Fill in the pull request template.

By contributing, you agree that your work is under the [MIT license](LICENSE).
