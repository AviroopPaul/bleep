import type { AgentAsk, AgentSession } from "../ipc";

export function groupSessions(sessions: AgentSession[]) {
  return {
    needsYou: sessions.filter((s) => s.state === "waiting"),
    running: sessions.filter((s) => s.state === "running"),
    done: sessions.filter((s) => s.state === "done"),
  };
}

/** How bleep handled one ask, in plain words. */
export function handledLine(a: Pick<AgentAsk, "decision" | "rule">): string {
  switch (a.decision) {
    case "auto":
      return `Approved by rule "${a.rule ?? "unknown"}"`;
    case "approved":
      return "Approved by you";
    case "denied":
      return "Denied by you";
    case "expired":
      return "Answered elsewhere";
    default:
      return "Sent to you";
  }
}

export function ago(at: string, now = Date.now()): string {
  const mins = Math.floor((now - new Date(at).getTime()) / 60_000);
  if (mins < 1) return "just now";
  if (mins < 60) return `${mins} min ago`;
  const h = Math.floor(mins / 60);
  if (h < 24) return `${h} h ago`;
  return `${Math.floor(h / 24)} d ago`;
}

export function folder(project: string): string {
  return project.replace(/\/+$/, "").split("/").pop() || "unknown folder";
}
