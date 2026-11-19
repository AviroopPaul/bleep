#!/bin/zsh
# Builds "bleep (local)" and installs it next to the production and nightly apps.
# It keeps its own data (~/.bleep-local), Keychain item and bundle id. See src-tauri/src/flavor.rs.
set -euo pipefail
cd "$(dirname "$0")/.."
source ~/.config/zsh/secrets.zsh
# Old local builds pile up in target/: drop this crate's stale objects, keep the dependencies.
(cd src-tauri && cargo clean --release -p bleep >/dev/null 2>&1 || true)
npx tauri build --config src-tauri/tauri.local.conf.json --features local
app="src-tauri/target/release/bundle/macos/bleep (local).app"
dest="/Applications/bleep (local).app"
osascript -e 'tell application id "site.avirooppaul.bleep.local" to quit' 2>/dev/null || true
# "bleep (dev)" was renamed "bleep (local)". Its data moves over on first launch.
osascript -e 'tell application id "site.avirooppaul.bleep.dev" to quit' 2>/dev/null || true
sleep 1
rm -rf "$dest" "/Applications/bleep (dev).app"
cp -R "$app" "$dest"
echo "Installed $dest"
