import { describe, expect, it } from "vitest";
import { ago, folder, groupSessions, handledLine } from "./agentsText";
import type { AgentAsk, AgentSession } from "../ipc";

const s = (id: string, state: AgentSession["state"]): AgentSession => ({
  id, tool: "claude", project: "/Users/me/code/shop", title: null, transcript_path: null, state, last_ask: null, updated: "2026-11-13T10:00:00Z",
});
const ask = (decision: AgentAsk["decision"], rule: string | null = null): AgentAsk => ({
  id: 1, session_id: "claude:a", kind: "permission", summary: "wants to run npm test", payload: {}, checks: [], decision, rule, rpc_id: null, created_at: "", decided_at: null,
});

describe("agents text", () => {
  it("groups sessions by what they need", () => {
    const g = groupSessions([s("a", "running"), s("b", "waiting"), s("c", "done"), s("d", "waiting")]);
    expect(g.needsYou.map((x) => x.id)).toEqual(["b", "d"]);
    expect(g.running.map((x) => x.id)).toEqual(["a"]);
    expect(g.done.map((x) => x.id)).toEqual(["c"]);
  });

  it("says how each ask was handled", () => {
    expect(handledLine(ask("auto", "npm test"))).toBe('Approved by rule "npm test"');
    expect(handledLine(ask("approved"))).toBe("Approved by you");
    expect(handledLine(ask("denied"))).toBe("Denied by you");
    expect(handledLine(ask("expired"))).toBe("Answered elsewhere");
    expect(handledLine(ask("queued"))).toBe("Sent to you");
  });

  it("shows ages and folder names", () => {
    const now = new Date("2026-11-13T10:00:00Z").getTime();
    expect(ago("2026-11-13T09:59:40Z", now)).toBe("just now");
    expect(ago("2026-11-13T09:55:00Z", now)).toBe("5 min ago");
    expect(ago("2026-11-13T07:00:00Z", now)).toBe("3 h ago");
    expect(ago("2026-11-11T10:00:00Z", now)).toBe("2 d ago");
    expect(folder("/Users/me/code/shop/")).toBe("shop");
    expect(folder("")).toBe("unknown folder");
  });

  it("uses no em dashes", () => {
    for (const d of ["auto", "approved", "denied", "expired", "queued"] as const) expect(handledLine(ask(d, "x"))).not.toContain("\u2014");
  });
});
