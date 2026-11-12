#!/bin/sh
# Counts starts, then answers each input with half a turn (up to the first text) and never finishes.
D="$(dirname "$0")"
echo start >> "$CLAUDE_CONFIG_DIR/starts"
while read -r l; do head -n 5 "$D/text_turn.jsonl"; done
