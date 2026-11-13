#!/bin/sh
# Fake claude for an image one-shot: needs stream-json in and an image on stdin.
case "$*" in *"--input-format stream-json"*) ;; *) echo '{"type":"result","is_error":true,"result":"no stream-json"}'; exit 1;; esac
read -r line
case "$line" in *'"type":"image"'*) ;; *) echo '{"type":"result","is_error":true,"result":"no image"}'; exit 1;; esac
echo '{"type":"system","subtype":"init"}'
echo '{"type":"assistant","message":{"content":[]}}'
echo '{"type":"result","subtype":"success","is_error":false,"structured_output":{"matches":true,"confidence":"high","reason":"ok"}}'
