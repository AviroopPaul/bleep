#!/usr/bin/env bash
# Creates bleep's own self-signed code-signing certificate. Run ONCE, then keep it forever.
#
# Why: an ad-hoc signature changes with every build, so macOS treats each update as a new app.
# Keychain access (the Google token) and other grants then reset. A certificate pins the app's
# designated requirement, so updates keep them. It does not get past Gatekeeper on its own: the
# Homebrew cask and install.sh clear the quarantine flag after checking the SHA-256.
#
# Back up bleep-signing.p12 AND p12-password (password manager). Losing them means the next
# release is a "different app" to every user's Mac.
set -euo pipefail
OUT_DIR="${OUT_DIR:-$HOME/.config/bleep-signing}"
CN="${CN:-bleep Self-Signed}"
P12="$OUT_DIR/bleep-signing.p12"
if [ -e "$P12" ]; then echo "refusing to overwrite $P12"; exit 1; fi
mkdir -p "$OUT_DIR"; chmod 700 "$OUT_DIR"
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
PW="$(openssl rand -hex 24)"
openssl req -x509 -newkey rsa:2048 -nodes -keyout "$TMP/key.pem" -out "$TMP/cert.pem" -days 3650 \
  -subj "/CN=$CN/O=bleep" \
  -addext "basicConstraints=critical,CA:false" -addext "keyUsage=critical,digitalSignature" \
  -addext "extendedKeyUsage=critical,codeSigning" 2>/dev/null
# -legacy keeps the .p12 readable by macOS `security import`.
openssl pkcs12 -export -legacy -out "$P12" -inkey "$TMP/key.pem" -in "$TMP/cert.pem" -passout "pass:$PW" -name "$CN" 2>/dev/null \
  || openssl pkcs12 -export -out "$P12" -inkey "$TMP/key.pem" -in "$TMP/cert.pem" -passout "pass:$PW" -name "$CN" 2>/dev/null
printf '%s' "$PW" > "$OUT_DIR/p12-password"; chmod 600 "$P12" "$OUT_DIR/p12-password"
FP="$(openssl x509 -in "$TMP/cert.pem" -noout -fingerprint -sha1 | sed 's/.*=//; s/://g')"
printf 'bleep code-signing certificate\nCreated: %s\nSubject: CN=%s, O=bleep\nSHA-1:   %s\nBack up bleep-signing.p12 and p12-password together.\n' "$(date -u +%FT%TZ)" "$CN" "$FP" > "$OUT_DIR/README.txt"
echo "Created $P12 (SHA-1 $FP). Password saved in $OUT_DIR/p12-password."
