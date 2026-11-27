#!/bin/sh
# Fake `claude`: records its args and stdin, then plays $FIXTURE (a file next to this script).
for a in "$@"; do printf '%s\n' "$a"; done > "$CLAUDE_CONFIG_DIR/args"
cat > "$CLAUDE_CONFIG_DIR/stdin"
cat "$(dirname "$0")/$(cat "$CLAUDE_CONFIG_DIR/fixture")"
