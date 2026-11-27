import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { api, onAgents, type AgentAsk, type AgentChatLine, type AgentSession, type AgentsView as View } from "../ipc";
import { ArrowUp } from "lucide-react";
import { Markdown } from "../chat/Markdown";
import { ago, answerNote, answersFor, askDetail, choicesOf, folder, groupSessions, handledLine, type Choice } from "./agentsText";

const EMPTY: View = { sessions: [], asks: [], held: [], replyable: [] };

/** The agent list, kept fresh from "agents://changed". */
export function useAgents() {
  const [view, setView] = useState<View>(EMPTY);
  const refresh = useCallback(() => api.agentsList().then((v) => setView({ ...EMPTY, ...v })).catch(() => {}), []);
  useEffect(() => {
    void refresh();
    const un = onAgents(() => void refresh());
    return () => void un.then((f) => f());
  }, [refresh]);
  return { view, refresh };
}

const TOOL: Record<AgentSession["tool"], string> = { claude: "Claude Code", codex: "Codex" };
const STATE: Record<AgentSession["state"], string> = { waiting: "needs you", running: "running", done: "done" };

/** True when bleep can pass my answer to this session right now. */
function canAnswer(v: View, s: AgentSession, ask?: AgentAsk) {
  if (s.tool === "codex") return !!ask?.rpc_id;
  return (!!ask && v.held.includes(ask.id)) || v.replyable.includes(s.id);
}

function Tool({ s }: { s: AgentSession }) {
  return <span className={`ag-tool ${s.tool}`} title={TOOL[s.tool]} aria-label={TOOL[s.tool]}>{s.tool === "claude" ? "CC" : "Cx"}</span>;
}

function Row({ s, ask, live, onOpen }: { s: AgentSession; ask?: AgentAsk; live: boolean; onOpen: () => void }) {
  const what = ask?.summary ?? s.last_ask ?? s.title ?? "";
  return (
    <li className={`ag-row ${s.state}`}>
      <button className="ag-hit" aria-label={`Open ${folder(s.project)}`} onClick={onOpen} />
      <Tool s={s} />
      <div className="ag-main">
        <span className="ag-proj">{folder(s.project)}</span>
        {what && <span className="ag-sum">{what}</span>}
        {s.state === "waiting" && <span className="ag-note">{live ? "Tap to answer" : s.tool === "claude" ? "Answer in the terminal" : "Answer in Codex"}</span>}
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

/** Option chips for each AskUserQuestion question, plus my own words. */
function ChoiceForm({ choices, busy, onSend }: { choices: Choice[]; busy: boolean; onSend: (answers: Record<string, string>) => void }) {
  const [picked, setPicked] = useState<Record<string, string[]>>({});
  const [other, setOther] = useState<Record<string, string>>({});
  const answers = answersFor(choices, picked, other);
  const toggle = (c: Choice, label: string) =>
    setPicked((p) => {
      const now = p[c.question] ?? [];
      const on = now.includes(label);
      return { ...p, [c.question]: c.multiSelect ? (on ? now.filter((x) => x !== label) : [...now, label]) : on ? [] : [label] };
    });
  return (
    <form className="ag-choices" onSubmit={(e) => { e.preventDefault(); onSend(answers); }}>
      {choices.map((c, i) => (
        <fieldset key={c.question} className="ag-q">
          <legend>
            {c.header && <span className="ag-qhead">{c.header}</span>}
            <span className="ag-qtext">{c.question}</span>
            {c.multiSelect && <span className="ag-qhint">Pick any</span>}
          </legend>
          <div className="ag-opts">
            {c.options.map((o) => (
              <button
                type="button"
                key={o.label}
                className="ag-opt"
                aria-pressed={(picked[c.question] ?? []).includes(o.label)}
                onClick={() => toggle(c, o.label)}
              >
                <b>{o.label}</b>
                {o.description && <small>{o.description}</small>}
              </button>
            ))}
          </div>
          <label className="sr-only" htmlFor={`ag-other-${i}`}>Other answer to: {c.question}</label>
          <input id={`ag-other-${i}`} className="ag-other" placeholder="Or type your own answer" value={other[c.question] ?? ""} onChange={(e) => setOther((o) => ({ ...o, [c.question]: e.target.value }))} />
        </fieldset>
      ))}
      <button className="btn pri" type="submit" disabled={busy || Object.keys(answers).length === 0}>Send answer</button>
    </form>
  );
}

function ReplyForm({ busy, onSend }: { busy: boolean; onSend: (text: string) => void }) {
  const [text, setText] = useState("");
  const send = () => text.trim() && onSend(text);
  return (
    <form className="composer ag-reply" onSubmit={(e) => { e.preventDefault(); send(); }}>
      <label htmlFor="ag-reply" className="sr-only">Reply to Claude</label>
      <textarea
        id="ag-reply"
        rows={2}
        value={text}
        placeholder="Reply to Claude"
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
            e.preventDefault();
            send();
          }
        }}
      />
      <button type="submit" className="send" aria-label="Send" title="Send" disabled={busy || !text.trim()}><ArrowUp aria-hidden="true" /></button>
    </form>
  );
}

/** What the session waits for, and the controls to answer it from here. */
function AnswerCard({ v, s, ask, onDone }: { v: View; s: AgentSession; ask?: AgentAsk; onDone: () => void }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const run = async (f: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    try {
      await f();
      onDone();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };
  const held = !!ask && v.held.includes(ask.id);
  const choices = ask && held ? choicesOf(ask) : null;
  const replyable = s.tool === "claude" && v.replyable.includes(s.id);
  const err = error && <p className="set-err" role="alert">{error}</p>;
  const note = ask ? answerNote(ask) : null;
  const auto = note && ask && (
    <div className="ag-auto" role="status">
      {note.kind === "pending" && (
        note.delaySecs > 0 ? (
          <>
            <p className="ag-why">bleep will answer "{note.label}" in {note.delaySecs} s.</p>
            <button className="btn" disabled={busy} onClick={() => void run(() => api.agentsKeep(ask.id))}>Keep for me</button>
          </>
        ) : (
          <p className="ag-why">bleep is answering "{note.label}" for you.</p>
        )
      )}
      {note.kind === "shadow" && (
        <p className="ag-why">Log only: bleep would answer "{note.label}"{note.wouldSend === false ? ", but the model check said no" : ""}.</p>
      )}
      {note.kind === "held" && <p className="ag-why">{note.why}</p>}
    </div>
  );

  if (ask && choices) {
    return (
      <section className="ag-card" aria-label="Claude asks">
        <h3 className="ag-card-h">Claude asks</h3>
        <ChoiceForm choices={choices} busy={busy} onSend={(a) => void run(() => api.agentsChoose(ask.id, a))} />
        {err}
      </section>
    );
  }
  if (ask && ask.kind === "permission" && (held || (s.tool === "codex" && ask.rpc_id))) {
    const detail = askDetail(ask);
    return (
      <section className="ag-card" aria-label="Permission ask">
        <h3 className="ag-card-h">{TOOL[s.tool]} {ask.summary}</h3>
        {detail && <pre className="ag-cmd">{detail}</pre>}
        {ask.checks.length > 0 && <p className="ag-why">{ask.checks.join(". ")}</p>}
        <div className="ag-acts">
          <button className="btn pri" disabled={busy} onClick={() => void run(() => api.agentsDecide(ask.id, true))}>Approve</button>
          <button className="btn" disabled={busy} onClick={() => void run(() => api.agentsDecide(ask.id, false))}>Deny</button>
        </div>
        {err}
      </section>
    );
  }
  if (replyable) {
    return (
      <section className="ag-card reply" aria-label="Reply">
        {ask?.kind === "question" && <h3 className="ag-card-h">{ask.summary}</h3>}
        {auto}
        <ReplyForm busy={busy} onSend={(t) => void run(() => api.agentsReply(s.id, t))} />
        {err}
      </section>
    );
  }
  if (s.state === "waiting") {
    return (
      <section className="ag-card" aria-label="Answer elsewhere">
        {ask && <h3 className="ag-card-h">{ask.summary}</h3>}
        {auto}
        <p className="ag-why">{s.tool === "claude" ? "Answer this one in the terminal." : "Answer this one in Codex."}</p>
      </section>
    );
  }
  return null;
}

/** One session: its chat so far, and the ask at the bottom. */
function SessionView({ v, s, ask, onBack, refresh }: { v: View; s: AgentSession; ask?: AgentAsk; onBack: () => void; refresh: () => void }) {
  const [lines, setLines] = useState<AgentChatLine[] | null>(null);
  const log = useRef<HTMLDivElement>(null);
  const [sent, setSent] = useState(false);
  useEffect(() => {
    let live = true;
    api.agentsChat(s.id).then((l) => live && setLines(l)).catch(() => live && setLines([]));
    return () => {
      live = false;
    };
  }, [s.id, s.updated]);
  useLayoutEffect(() => {
    if (log.current) log.current.scrollTop = log.current.scrollHeight;
  }, [lines, sent]);
  useEffect(() => {
    if (s.state === "waiting") setSent(false);
  }, [s.state, ask?.id]);
  return (
    <div className="ag-session" role="region" aria-label={`${folder(s.project)} chat`}>
      <header className="ag-shead">
        <button className="iconbtn" aria-label="Back to agents" onClick={onBack}>
          <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M10 3L5 8l5 5" /></svg>
        </button>
        <Tool s={s} />
        <div className="ag-main">
          <span className="ag-proj">{folder(s.project)}</span>
          <span className={`ag-state ${s.state}`}>{STATE[s.state]} · {ago(s.updated)}</span>
        </div>
      </header>
      <div className="log ag-log" ref={log}>
        {lines === null && <div className="msg sys">Loading the chat...</div>}
        {lines?.length === 0 && <div className="msg sys">{s.tool === "codex" ? "bleep shows the chat for Claude Code sessions only." : "No chat to show yet."}</div>}
        {lines?.map((l, i) =>
          l.role === "tool" ? (
            <div key={i} className="ag-step">{l.text}</div>
          ) : l.role === "user" ? (
            <div key={i} className="msg me">{l.text}</div>
          ) : (
            <div key={i} className="msg bot"><span className="text"><Markdown text={l.text} /></span></div>
          ),
        )}
        {sent && <div className="msg sys" role="status">Sent. Claude goes on.</div>}
      </div>
      <AnswerCard v={v} s={s} ask={ask} onDone={() => { setSent(true); refresh(); }} />
    </div>
  );
}

/** Coding sessions that wait on me, run, or finished, and what bleep did with their asks. */
export function AgentsView({ view, refresh }: { view: View; refresh: () => void }) {
  const [open, setOpen] = useState<string | null>(null);
  const g = groupSessions(view.sessions);
  const queued = (s: AgentSession) => view.asks.find((a) => a.session_id === s.id && a.decision === "queued");
  const handled = view.asks.filter((a) => a.decision !== "queued").slice(0, 20);
  const current = open ? view.sessions.find((s) => s.id === open) : undefined;
  if (current) return <SessionView v={view} s={current} ask={queued(current)} onBack={() => setOpen(null)} refresh={refresh} />;
  const row = (s: AgentSession) => <Row key={s.id} s={s} ask={queued(s)} live={canAnswer(view, s, queued(s))} onOpen={() => setOpen(s.id)} />;
  return (
    <div className="agents" role="region" aria-label="Coding agents">
      {!view.sessions.length && !handled.length && <p className="ag-empty">No coding agents running.</p>}
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
