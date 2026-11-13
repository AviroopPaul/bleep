#!/usr/bin/env bash
# Builds the release dmg signed with bleep's own certificate (scripts/make-signing-cert.sh).
#   app/scripts/build-release.sh   ->  app/release/bleep-mac.dmg + bleep-mac.dmg.sha256
# Needs BLEEP_GOOGLE_CLIENT_ID / _SECRET in the environment (source ~/.config/zsh/secrets.zsh).
set -euo pipefail
cd "$(dirname "$0")/.."
SIGN_DIR="${SIGN_DIR:-$HOME/.config/bleep-signing}"
P12="$SIGN_DIR/bleep-signing.p12"; PW="$(cat "$SIGN_DIR/p12-password")"
CN="bleep Self-Signed"
: "${BLEEP_GOOGLE_CLIENT_ID:?source your secrets first}"

# codesign only searches keychains on the user search list, so add a throwaway one and restore after.
KC_DIR="$(mktemp -d)"; KC="$KC_DIR/bleep-build.keychain-db"; KC_PASS="$(openssl rand -hex 16)"
ORIG=(); while IFS= read -r l; do l="${l//\"/}"; l="${l#"${l%%[![:space:]]*}"}"; [ -n "$l" ] && ORIG+=("$l"); done < <(security list-keychains -d user)
cleanup() { security list-keychains -d user -s "${ORIG[@]}" 2>/dev/null || true; security delete-keychain "$KC" 2>/dev/null || true; rm -rf "$KC_DIR"; }
trap cleanup EXIT
security create-keychain -p "$KC_PASS" "$KC"
security unlock-keychain -p "$KC_PASS" "$KC"
security set-keychain-settings -lut 21600 "$KC"
security import "$P12" -k "$KC" -P "$PW" -T /usr/bin/codesign -f pkcs12 >/dev/null
security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$KC_PASS" "$KC" >/dev/null
security list-keychains -d user -s "${ORIG[@]}" "$KC"

npm run build:hook
npm run tauri build -- --bundles app,dmg --config "{\"bundle\":{\"macOS\":{\"signingIdentity\":\"$CN\"}}}"

APP=src-tauri/target/release/bundle/macos/bleep.app
codesign --verify --deep --strict "$APP"
codesign -d -r- "$APP" 2>&1 | grep designated
VER="$(node -p "require('./src-tauri/tauri.conf.json').version")"
mkdir -p release
cp "src-tauri/target/release/bundle/dmg/bleep_${VER}_aarch64.dmg" release/bleep-mac.dmg
(cd release && shasum -a 256 bleep-mac.dmg > bleep-mac.dmg.sha256 && cat bleep-mac.dmg.sha256)
