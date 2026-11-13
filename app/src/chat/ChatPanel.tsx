import { useCallback, useEffect, useLayoutEffect, useReducer, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api, healthOk, onApproval, onChatEvent, onNotice, type Message, type RuntimeHealth } from "../ipc";
import { chatReducer, emptyChat, formatBackAt } from "./chatReducer";
import { ToolSteps } from "./ToolSteps";
import { ApprovalCard } from "./ApprovalCard";
import { Markdown } from "./Markdown";
import { DROID_HEAD } from "../droid/droidSvg";
import { AgentsView, useAgents } from "../agents/AgentsView";

const PAGE = 50;
const SUGGESTIONS = ["What time is it in Tokyo?", "What day is it today?"];

function Bubble({ m }: { m: Message }) {
  if (m.role === "system") return <div className="msg sys">{m.content}</div>;
  if (m.role === "user" && m.kind === "scheduled") return <div className="msg sched"><span className="tag">scheduled</span>{m.content}</div>;
  if (m.role === "user") return <div className="msg me">{m.content}</div>;
  const waiting = m.status === "streaming" && !m.content && !m.tool_steps.length;
  return (
    <div className={`msg bot ${m.status === "error" ? "error" : ""}`}>
      <ToolSteps steps={m.tool_steps} />
      {m.approvals?.map((a) => <ApprovalCard key={a.id} a={a} />)}
      {waiting ? (
        <span className="typing" aria-label="bleep is thinking"><i /><i /><i /></span>
      ) : (
        <span className="text"><Markdown text={m.content} /></span>
      )}
      {m.status === "interrupted" && <span className="note">Interrupted</span>}
    </div>
  );
}

export function ChatPanel() {
  const [state, dispatch] = useReducer(chatReducer, emptyChat);
  const [health, setHealth] = useState<RuntimeHealth | null>(null);
  const [draft, setDraft] = useState("");
  const [sending, setSending] = useState(false);
  const [noMore, setNoMore] = useState(false);
  const [sendError, setSendError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const log = useRef<HTMLDivElement>(null);
  const input = useRef<HTMLTextAreaElement>(null);
  const stick = useRef(true);
  const keepFrom = useRef<number | null>(null);
  const [view, setView] = useState<"chat" | "agents">("chat");
  const agents = useAgents();
  const waiting = agents.view.sessions.filter((x) => x.state === "waiting").length;

  const checkHealth = useCallback(() => api.health().then(setHealth).catch(() => setHealth(null)), []);

  useEffect(() => {
    api.history(null, PAGE).then((messages) => {
      dispatch({ type: "history", messages, mode: "replace" });
      setNoMore(messages.length < PAGE);
    });
    void checkHealth();
    const un = onChatEvent(({ message_id, event }) => dispatch({ type: "event", messageId: message_id, event }));
    const unApproval = onApproval((approval) => dispatch({ type: "approval", approval }));
    const unNotice = onNotice(setNotice);
    const unFocus = getCurrentWindow().onFocusChanged(({ payload: focused }) => {
      if (focused) {
        input.current?.focus();
        void checkHealth();
      }
    });
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && void api.hideChat();
    window.addEventListener("keydown", onKey);
    return () => {
      void un.then((f) => f());
      void unApproval.then((f) => f());
      void unNotice.then((f) => f());
      void unFocus.then((f) => f());
      window.removeEventListener("keydown", onKey);
    };
  }, [checkHealth]);

  // Keep the view pinned to the bottom while new text streams in, and steady when older pages load.
  useLayoutEffect(() => {
    const el = log.current;
    if (!el) return;
    if (keepFrom.current !== null) {
      el.scrollTop = el.scrollHeight - keepFrom.current;
      keepFrom.current = null;
    } else if (stick.current) {
      el.scrollTop = el.scrollHeight;
    }
  }, [state.messages]);

  const onScroll = () => {
    const el = log.current;
    if (!el) return;
    stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
    if (el.scrollTop < 30 && !noMore && state.messages.length) {
      setNoMore(true);
      api.history(state.messages[0].id, PAGE).then((older) => {
        keepFrom.current = el.scrollHeight - el.scrollTop;
        dispatch({ type: "history", messages: older, mode: "prepend" });
        setNoMore(older.length < PAGE);
      });
    }
  };

  const ok = healthOk(health);
  const busy = sending || state.streamingId !== null;
  const canSend = ok && !busy && draft.trim().length > 0;

  const send = async (text: string) => {
    const t = text.trim();
    if (!t || !ok || busy) return;
    setSending(true);
    stick.current = true;
    try {
      const assistantId = await api.send(t);
      const user: Message = { id: assistantId - 1, role: "user", kind: "chat", content: t, tool_steps: [], tokens: null, status: "done", session_id: null, created_at: "" };
      dispatch({ type: "sent", user, assistantId });
      setDraft("");
      setSendError(null);
      setNotice(null);
    } catch (e) {
      const msg = String(e);
      if (msg !== "busy") {
        setSendError(msg);
        // The core may have saved the turn with an error status; show what it stored.
        api.history(null, PAGE).then((messages) => dispatch({ type: "history", messages, mode: "replace" })).catch(() => {});
      }
    } finally {
      setSending(false);
      input.current?.focus();
    }
  };

  const onKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
      e.preventDefault();
      void send(draft);
    }
  };

  const limited = state.rateLimitedUntil && state.rateLimitedUntil * 1000 > Date.now();

  return (
    <div className="panel">
      <header className="panel-head" data-tauri-drag-region>
        <span className="head-droid" dangerouslySetInnerHTML={{ __html: DROID_HEAD }} />
        <h1>bleep</h1>
        <div className="views" role="tablist" aria-label="View">
          <button role="tab" className="view-tab" aria-selected={view === "chat"} onClick={() => setView("chat")}>Chat</button>
          <button role="tab" className="view-tab" aria-selected={view === "agents"} aria-label={waiting ? `Agents, ${waiting} need you` : "Agents"} onClick={() => setView("agents")}>
            Agents{waiting > 0 && <span className="view-count" aria-hidden="true">{waiting}</span>}
          </button>
        </div>
        <span className="spacer" data-tauri-drag-region />
        <span className="badge">Claude Code</span>
        <button className="iconbtn" aria-label="Close chat" title="Close (Esc)" onClick={() => void api.hideChat()}>
          <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true"><path d="M4 4l8 8M12 4l-8 8" /></svg>
        </button>
      </header>

      {view === "agents" && <AgentsView view={agents.view} refresh={() => void agents.refresh()} />}

      {view === "chat" && health && !ok && (
        <div className="banner" role="status">
          <p>{health.hint}</p>
          <div className="acts">
            {health.installed && health.version_ok && (
              <button className="btn pri" onClick={() => void api.openLogin()}>Sign in</button>
            )}
            <button className="btn" onClick={() => void checkHealth()}>Check again</button>
          </div>
        </div>
      )}

      <div className="log" ref={log} onScroll={onScroll} aria-live="polite" hidden={view !== "chat"}>
        {state.messages.map((m) => <Bubble key={m.id} m={m} />)}
        {state.loose.map((a) => <ApprovalCard key={a.id} a={a} />)}
        {limited && <div className="msg sys">{formatBackAt(state.rateLimitedUntil!)}</div>}
        {sendError && <div className="msg sys err" role="alert">{sendError}</div>}
        {notice && <div className="msg sys" role="status">{notice}</div>}
      </div>

      {view === "chat" && ok && state.messages.length === 0 && (
        <div className="sugs">
          {SUGGESTIONS.map((s) => <button key={s} onClick={() => void send(s)} disabled={busy}>{s}</button>)}
        </div>
      )}

      <form className="composer" hidden={view !== "chat"} onSubmit={(e) => { e.preventDefault(); void send(draft); }}>
        <label htmlFor="msg" className="sr-only">Message bleep</label>
        <textarea
          id="msg"
          ref={input}
          rows={1}
          value={draft}
          placeholder={ok ? "Ask bleep" : "Sign in to chat"}
          disabled={!ok || busy}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={onKeyDown}
          autoFocus
        />
        <button type="submit" disabled={!canSend}>Send</button>
      </form>
    </div>
  );
}
