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
    case "answered":
      return "Answered by you";
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

/** One question of a Claude Code AskUserQuestion call. */
export interface Choice {
  question: string;
  header: string;
  options: { label: string; description: string }[];
  multiSelect: boolean;
}

/** The questions of an AskUserQuestion ask, or null for any other ask. */
export function choicesOf(a: Pick<AgentAsk, "payload">): Choice[] | null {
  if (a.payload.tool_name !== "AskUserQuestion" || !Array.isArray(a.payload.questions)) return null;
  return (a.payload.questions as Record<string, unknown>[])
    .filter((q) => typeof q.question === "string")
    .map((q) => ({
      question: q.question as string,
      header: typeof q.header === "string" ? q.header : "",
      options: (Array.isArray(q.options) ? q.options : [])
        .filter((o): o is Record<string, unknown> => !!o && typeof (o as Record<string, unknown>).label === "string")
        .map((o) => ({ label: o.label as string, description: typeof o.description === "string" ? o.description : "" })),
      multiSelect: q.multiSelect === true,
    }));
}

/** My picks as Claude Code wants them: several labels joined with ", ", or my own words. */
export function answersFor(choices: Choice[], picked: Record<string, string[]>, other: Record<string, string>): Record<string, string> {
  const out: Record<string, string> = {};
  for (const c of choices) {
    const own = other[c.question]?.trim();
    const labels = picked[c.question] ?? [];
    const a = own || labels.join(", ");
    if (a) out[c.question] = a;
  }
  return out;
}

/** What a permission ask wants to run or touch, for the answer card. */
export function askDetail(a: Pick<AgentAsk, "payload">): string | null {
  const p = a.payload;
  if (typeof p.command === "string" && p.command) return p.command;
  if (Array.isArray(p.paths) && p.paths.length) return (p.paths as string[]).join("\n");
  return null;
}
