# Agent watch fixtures

Recorded on 13 Nov 2026. Paths and ids replaced with neutral ones.

## claude/ (Claude Code 2.1.319)
- `session_start`, `user_prompt_submit`, `permission_request_bash`, `permission_request_read`, `post_tool_use`, `stop`, `session_end`: captured from a headless run with logging hooks in a temporary `--settings` file. `permission_request_bash` has its command changed to `npm test`; `stop` has its `last_assistant_message` changed to a look question.
- `stop_active`: `stop` with `stop_hook_active: true`.
- `notification_permission`, `notification_idle`: hand-built from the 2.1.319 hook schema (`message`, `notification_type`), because headless runs do not send Notification.
- Checked live: `PermissionRequest` fires in headless and interactive runs. The output `{"hookSpecificOutput":{"hookEventName":"PermissionRequest","decision":{"behavior":"allow"}}}` skips the prompt; no output leaves the normal prompt. `Stop` input carries `last_assistant_message`.

## transcript_*.jsonl
Hand-built in the real line shape. A real user prompt is `type: "user"` with string content (or text blocks). Tool results are `type: "user"` with `tool_result` blocks; screenshots sit inside them as `{"type":"image","source":{"type":"base64","media_type":...,"data":...}}`. Many other line types (`attachment`, `last-prompt`, `api-request*`) must be skipped. The image is a 1x1 PNG.

## codex/ (Codex CLI 0.164.0, daemon 0.167.0)
Shapes from a live probe with two clients on the shared daemon and a throwaway thread (archived after). Values trimmed.
- The daemon's control socket is `~/.codex/app-server-control/app-server-control.sock` (a symlink). It speaks JSON-RPC over **WebSocket** (HTTP upgrade on the Unix socket), not newline JSON. `codex app-server proxy` does not start the daemon; with no daemon it fails.
- `thread/read` status: `{"type":"active","activeFlags":["waitingOnApproval"]}` or `{"type":"idle"}`.
- A client that subscribes (`thread/resume`) after an approval request gets it replayed. The first answer wins; the other client gets `serverRequest/resolved`. `thread/unsubscribe` does not unload a thread another client still holds.
- Commands arrive wrapped: `/bin/zsh -lc '<command>'`. Decisions: `accept`, `acceptForSession`, `decline`, `cancel`.
- `notify_payload`: the JSON Codex appends as the last argument to the `notify` program (hand-built from the documented format).

## One-shot with an image (Claude Code 2.1.319)
`claude -p --input-format stream-json --output-format stream-json --verbose --json-schema <schema> --model <small> --tools "" --setting-sources "" --strict-mcp-config --no-session-persistence`, with one stdin line `{"type":"user","message":{"role":"user","content":[{"type":"text",...},{"type":"image","source":{"type":"base64","media_type":"image/png","data":...}}]}}` then EOF. `--input-format stream-json` refuses `--output-format json`. The answer is the last line with `type: "result"`; `structured_output` holds the object. Checked live: a blue square gave `matches: true, confidence: high`.
