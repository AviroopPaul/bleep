// Fake backend for headless UI checks (Playwright). Loaded only with ?mock=1.
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import type { ChatEmit, Message } from "./ipc";

const row = (id: number, role: Message["role"], content: string): Message => ({
  id, role, kind: "chat", content, tool_steps: [], tokens: null, status: "done", session_id: 1, created_at: "",
});

export function installMocks(label: string) {
  mockWindows(label);
  const history: Message[] = [row(1, "user", "hello"), row(2, "assistant", "Hi! I am bleep.")];
  let next = 3;
  mockIPC(
    (cmd, args) => {
      const a = (args ?? {}) as Record<string, unknown>;
      switch (cmd) {
        case "chat_history":
          return a.beforeId ? [] : history;
        case "runtime_health":
          return { runtime: "claude", installed: true, version: "2.1.319", version_ok: true, signed_in: true, plan: "pro", hint: null };
        case "chat_send": {
          if (a.text === "fail please") throw "bleep could not reach Claude Code: spawn failed";
          next += 2;
          const id = next;
          const steps: ChatEmit["event"][] = [
            { type: "tool_start", id: "t1", name: "bleep_now", args: {} },
            { type: "tool_result", id: "t1", ok: true, preview: "{}" },
            { type: "text_delta", text: "It is 8:05 PM in Tokyo." },
            { type: "usage", input: 1200, output: 14 },
            { type: "done", runtime_session_id: "s1" },
          ];
          steps.forEach((event, i) => setTimeout(() => emit("chat://event", { message_id: id, event }), 150 * (i + 1)));
          return id;
        }
        default:
          return null;
      }
    },
    { shouldMockEvents: true },
  );
}
