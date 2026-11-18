#!/bin/zsh
# Builds "bleep (dev)" and installs it next to the production app.
# It keeps its own data (~/.bleep-dev), Keychain item and bundle id. See src-tauri/src/flavor.rs.
set -euo pipefail
cd "$(dirname "$0")/.."
source ~/.config/zsh/secrets.zsh
npx tauri build --config src-tauri/tauri.dev.conf.json --features dev
app="src-tauri/target/release/bundle/macos/bleep (dev).app"
dest="/Applications/bleep (dev).app"
osascript -e 'tell application id "site.avirooppaul.bleep.dev" to quit' 2>/dev/null || true
sleep 1
rm -rf "$dest"
cp -R "$app" "$dest"
echo "Installed $dest"
