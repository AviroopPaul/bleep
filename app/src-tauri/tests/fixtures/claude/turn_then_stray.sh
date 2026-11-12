#!/bin/sh
# Answers every input with a full turn, then one stray line after the result.
D="$(dirname "$0")"
while read -r l; do cat "$D/text_turn.jsonl"; echo '{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"STRAY"}}}'; done
