#!/bin/sh
# Acts like claude --resume with a stale id: one error result, then stays alive.
sed -n 2p "$(dirname "$0")/init_error.jsonl"
exec sleep 10
