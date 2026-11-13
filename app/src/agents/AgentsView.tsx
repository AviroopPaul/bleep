import { useCallback, useEffect, useState } from "react";
import { api, onAgents, type AgentAsk, type AgentSession, type AgentsView as View } from "../ipc";
import { ago, folder, groupSessions, handledLine } from "./agentsText";

/** The agent list, kept fresh from "agents://changed". */
export function useAgents() {
  const [view, setView] = useState<View>({ sessions: [], asks: [] });
  const refresh = useCallback(() => api.agentsList().then(setView).catch(() => {}), []);
  useEffect(() => {
    void refresh();
    const un = onAgents(() => void refresh());
    return () => void un.then((f) => f());
  }, [refresh]);
  return { view, refresh };
}

const TOOL: Record<AgentSession["tool"], string> = { claude: "Claude Code", codex: "Codex" };

function Row({ s, ask, onDecide }: { s: AgentSession; ask?: AgentAsk; onDecide: (id: number, yes: boolean) => void }) {
  const what = ask?.summary ?? s.last_ask ?? s.title ?? "";
  return (
    <li className={`ag-row ${s.state}`}>
      <span className={`ag-tool ${s.tool}`} title={TOOL[s.tool]} aria-label={TOOL[s.tool]}>{s.tool === "claude" ? "CC" : "Cx"}</span>
      <div className="ag-main">
        <span className="ag-proj">{folder(s.project)}</span>
        {what && <span className="ag-sum">{what}</span>}
        {s.state === "waiting" && s.tool === "claude" && <span className="ag-note">Answer in the terminal</span>}
        {s.state === "waiting" && s.tool === "codex" && ask?.rpc_id && (
          <span className="ag-acts">
            <button className="btn pri" onClick={() => onDecide(ask.id, true)}>Approve</button>
            <button className="btn" onClick={() => onDecide(ask.id, false)}>Deny</button>
          </span>
        )}
        {s.state === "waiting" && s.tool === "codex" && !ask?.rpc_id && <span className="ag-note">Answer in Codex</span>}
      </div>
      <span className="ag-age">{ago(s.updated)}</span>
    </li>
  );
}

function Group({ title, items, children }: { title: string; items: unknown[]; children: React.ReactNode }) {
  if (!items.length) return null;
  const id = `ag-${title.replace(/\s/g, "").toLowerCase()}`;
  return (
    <section aria-labelledby={id}>
      <h2 id={id} className="ag-h">{title}</h2>
      <ul className="ag-list">{children}</ul>
    </section>
  );
}

/** Coding sessions that wait on me, run, or finished, and what bleep did with their asks. */
export function AgentsView({ view, refresh }: { view: View; refresh: () => void }) {
  const [error, setError] = useState<string | null>(null);
  const g = groupSessions(view.sessions);
  const queued = (s: AgentSession) => view.asks.find((a) => a.session_id === s.id && a.decision === "queued");
  const handled = view.asks.filter((a) => a.decision !== "queued").slice(0, 20);
  const decide = async (id: number, yes: boolean) => {
    setError(null);
    try {
      await api.agentsDecide(id, yes);
    } catch (e) {
      setError(String(e));
    }
    refresh();
  };
  const row = (s: AgentSession) => <Row key={s.id} s={s} ask={queued(s)} onDecide={(id, yes) => void decide(id, yes)} />;
  return (
    <div className="agents" role="region" aria-label="Coding agents">
      {!view.sessions.length && !handled.length && <p className="ag-empty">No coding agents running.</p>}
      {error && <p className="set-err" role="alert">{error}</p>}
      <Group title="Needs you" items={g.needsYou}>{g.needsYou.map(row)}</Group>
      <Group title="Running" items={g.running}>{g.running.map(row)}</Group>
      <Group title="Done" items={g.done}>{g.done.map(row)}</Group>
      <Group title="Handled" items={handled}>
        {handled.map((a) => (
          <li key={a.id} className="ag-row handled">
            <span className="ag-main">
              <span className="ag-sum">{a.summary}</span>
              <span className="ag-note">{handledLine(a)}</span>
            </span>
            <span className="ag-age">{ago(a.decided_at ?? a.created_at)}</span>
          </li>
        ))}
      </Group>
    </div>
  );
}
