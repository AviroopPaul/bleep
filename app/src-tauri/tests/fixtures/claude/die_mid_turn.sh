#!/bin/sh
# Prints the first 4 lines of a real turn, then dies before the result line.
head -n 4 "$(dirname "$0")/text_turn.jsonl"
exit 1
