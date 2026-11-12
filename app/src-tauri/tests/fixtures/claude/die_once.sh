#!/bin/sh
# Dies at once the first time (a child that died while idle), then answers normally.
D="$(dirname "$0")"
if [ ! -f "$CLAUDE_CONFIG_DIR/died" ]; then touch "$CLAUDE_CONFIG_DIR/died"; exit 0; fi
while read -r l; do cat "$D/text_turn.jsonl"; done
