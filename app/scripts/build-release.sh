#!/usr/bin/env bash
# Builds the release dmg signed with bleep's own certificate (scripts/make-signing-cert.sh).
#   app/scripts/build-release.sh   ->  app/release/bleep-mac.dmg + .sha256, plus the update feed
#                                       (bleep-mac.app.tar.gz + latest.json, signed with updater.key)
# Needs BLEEP_GOOGLE_CLIENT_ID / _SECRET in the environment (source ~/.config/zsh/secrets.zsh).
#
# FLAVOR=nightly VERSION=0.5.1-nightly.20261119.7 app/scripts/build-release.sh builds "bleep (nightly)"
# instead (.github/workflows/nightly.yml): release/bleep-nightly-mac.dmg, bleep-nightly-mac.app.tar.gz
# and latest-nightly.json, for the rolling "nightly" release in AviroopPaul/bleep-releases.
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

# The updater key (minisign) signs bleep.app.tar.gz. Its public half is in tauri.conf.json.
export TAURI_SIGNING_PRIVATE_KEY="$SIGN_DIR/updater.key"
TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$(cat "$SIGN_DIR/updater-key-password")"; export TAURI_SIGNING_PRIVATE_KEY_PASSWORD

FLAVOR="${FLAVOR:-prod}"
SIGNING="{\"bundle\":{\"createUpdaterArtifacts\":true,\"macOS\":{\"signingIdentity\":\"$CN\"}}}"
case "$FLAVOR" in
  prod)
    NAME=bleep; OUT=bleep-mac; FEED=latest.json
    EXTRA=() ;;
  nightly)
    : "${VERSION:?FLAVOR=nightly needs VERSION, for example 0.5.1-nightly.20261119.7}"
    NAME="bleep (nightly)"; OUT=bleep-nightly-mac; FEED=latest-nightly.json
    EXTRA=(--features nightly --config src-tauri/tauri.nightly.conf.json --config "{\"version\":\"$VERSION\"}") ;;
  *) echo "FLAVOR must be prod or nightly" >&2; exit 1 ;;
esac

npm run build:hook
# The odd expansion keeps an empty array working under macOS's bash 3.2 with set -u.
npm run tauri build -- --bundles app,dmg ${EXTRA[@]+"${EXTRA[@]}"} --config "$SIGNING"

APP="src-tauri/target/release/bundle/macos/$NAME.app"
codesign --verify --deep --strict "$APP"
codesign -d -r- "$APP" 2>&1 | grep designated
VER="${VERSION:-$(node -p "require('./src-tauri/tauri.conf.json').version")}"
mkdir -p release
cp "src-tauri/target/release/bundle/dmg/${NAME}_${VER}_aarch64.dmg" "release/$OUT.dmg" 2>/dev/null \
  || cp "$(ls -t "src-tauri/target/release/bundle/dmg/${NAME}_"*_aarch64.dmg | head -1)" "release/$OUT.dmg"
(cd release && shasum -a 256 "$OUT.dmg" > "$OUT.dmg.sha256" && cat "$OUT.dmg.sha256")

# Update feed for the in-app updater: release/latest.json points at this version's tarball.
# Optional release notes (shown in Settings > Updates): app/release/notes.md.
cp "$APP.tar.gz" "release/$OUT.app.tar.gz"
if [ "$FLAVOR" = nightly ]; then
  URL="https://github.com/AviroopPaul/bleep-releases/releases/download/nightly/$OUT.app.tar.gz"
else
  URL="https://github.com/AviroopPaul/bleep-releases/releases/download/v$VER/$OUT.app.tar.gz"
fi
NOTES_FILE=release/notes.md VER="$VER" URL="$URL" FEED="release/$FEED" SIG="$(cat "$APP.tar.gz.sig")" node -e '
const fs = require("fs");
const notes = fs.existsSync(process.env.NOTES_FILE) ? fs.readFileSync(process.env.NOTES_FILE, "utf8").trim() : "";
const url = process.env.URL;
const feed = { version: process.env.VER, notes, pub_date: new Date().toISOString(),
  platforms: { "darwin-aarch64": { signature: process.env.SIG, url } } };
fs.writeFileSync(process.env.FEED, JSON.stringify(feed, null, 2) + "\n");'
echo "Upload to AviroopPaul/bleep-releases:"
echo "  release/$OUT.dmg release/$OUT.dmg.sha256 release/$OUT.app.tar.gz release/$FEED"
