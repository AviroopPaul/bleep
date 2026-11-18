// Copy for Settings > Connectors. No em dashes in any of it.
import type { CatalogItem, ConnectorStatus, McpAuth, RemoteTool, ToolMode } from "../ipc";

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

/** The pill on a catalog card. Nothing while it is off and idle. */
export function catalogStatus(c: CatalogItem, busy: boolean): string {
  if (busy) return c.on ? "Turning off..." : "Waiting for sign-in...";
  if (!c.on) return "";
  switch (c.status.state) {
    case "connected": return c.kind === "remote" && c.tools ? `On, ${c.tools} tools` : "On";
    case "needs_sign_in": return "Needs sign-in";
    case "needs_setup": return "Set up Google first";
    case "error": return c.status.message;
  }
}

export const GOOGLE_NOTE = "One Google sign-in for all three. Turning one on asks Google only for what it needs.";

export const SIGNING_IN = "Checking how it signs in. If a browser tab opens, sign in there.";
export const NEEDS_CLIENT = "This server does not let bleep register itself. Paste a client ID from the server's settings, then press Add again.";
export const ADDED = (name: string) => `Added ${name}. Its tools work in the chat from your next message.`;
