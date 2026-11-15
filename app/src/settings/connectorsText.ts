// Copy for Settings > Connectors. No em dashes in any of it.
import type { ConnectorStatus, McpAuth, RemoteTool, ToolMode } from "../ipc";

export function statusText(s: ConnectorStatus): string {
  switch (s.state) {
    case "connected": return "Connected";
    case "needs_sign_in": return "Needs sign-in";
    case "needs_setup": return "Not set up";
    case "error": return s.message;
  }
}

const AUTH: Record<McpAuth, string> = { oauth: "Signs in through your browser", token: "Uses a token you pasted", none: "No sign-in" };
export const authText = (a: McpAuth) => AUTH[a];

const MODE: Record<ToolMode, string> = { ask: "Ask first", auto: "Run at once", off: "Off" };
export const modeLabel = (m: ToolMode, def: ToolMode) => (m === def ? `${MODE[m]} (default)` : MODE[m]);

/** Tools bleep can use: not off and not left out. */
export function toolCount(tools: RemoteTool[]): string {
  const n = tools.filter((t) => t.mode !== "off" && !t.skipped).length;
  return n === 0 ? "No tools" : n === 1 ? "1 tool" : `${n} tools`;
}

export const SIGNING_IN = "Checking how it signs in. If a browser tab opens, sign in there.";
export const NEEDS_CLIENT = "This server does not let bleep register itself. Paste a client ID from the server's settings, then press Add again.";
export const ADDED = (name: string) => `Added ${name}. Its tools work in the chat from your next message.`;
