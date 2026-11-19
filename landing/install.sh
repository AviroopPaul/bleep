#!/bin/sh
# Installs bleep into Applications and opens it.
#   curl -fsSL https://bleep-omega.vercel.app/install.sh | sh
# Downloads the latest release, checks its SHA-256, copies bleep.app, and clears the download
# quarantine flag so macOS opens it like any other app. Source of this script: the bleep repo.
set -eu

BASE="https://github.com/AviroopPaul/bleep/releases/latest/download"
fail() { printf 'bleep: %s\n' "$*" >&2; exit 1; }

[ "$(uname -s)" = "Darwin" ] || fail "bleep runs on macOS only."
[ "$(uname -m)" = "arm64" ] || fail "bleep needs a Mac with Apple silicon."
major="$(sw_vers -productVersion | cut -d. -f1)"
[ "$major" -ge 15 ] || fail "bleep needs macOS 15 or later."

tmp="$(mktemp -d)"
mnt="$tmp/mnt"
cleanup() { hdiutil detach "$mnt" -quiet >/dev/null 2>&1 || true; rm -rf "$tmp"; }
trap cleanup EXIT INT TERM

echo "Downloading bleep..."
curl -fsSL "$BASE/bleep-mac.dmg" -o "$tmp/bleep-mac.dmg" || fail "download failed."
curl -fsSL "$BASE/bleep-mac.dmg.sha256" -o "$tmp/bleep-mac.dmg.sha256" || fail "checksum download failed."
want="$(awk '{print $1}' "$tmp/bleep-mac.dmg.sha256")"
got="$(shasum -a 256 "$tmp/bleep-mac.dmg" | awk '{print $1}')"
[ -n "$want" ] && [ "$want" = "$got" ] || fail "checksum mismatch. Nothing was installed."

mkdir -p "$mnt"
hdiutil attach "$tmp/bleep-mac.dmg" -nobrowse -readonly -quiet -mountpoint "$mnt" || fail "could not open the disk image."

dest="/Applications"
if [ ! -w "$dest" ]; then dest="$HOME/Applications"; mkdir -p "$dest"; fi
if pgrep -xq bleep; then osascript -e 'quit app id "site.avirooppaul.bleep"' >/dev/null 2>&1 || true; sleep 1; fi
rm -rf "$dest/bleep.app"
ditto "$mnt/bleep.app" "$dest/bleep.app"
xattr -dr com.apple.quarantine "$dest/bleep.app" 2>/dev/null || true

echo "Installed bleep in $dest. Opening it..."
open "$dest/bleep.app"
