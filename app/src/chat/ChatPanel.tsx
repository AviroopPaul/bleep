import { useCallback, useEffect, useLayoutEffect, useReducer, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api, healthOk, onApproval, onChatEvent, onChatExpanded, onCompacting, onNotice, onPing, onVoice, type ChatContext, type Message, type RuntimeHealth } from "../ipc";
import { meterLevel } from "../settings/memoryText";
import { chatReducer, emptyChat, formatBackAt } from "./chatReducer";
import { ToolSteps } from "./ToolSteps";
import { ApprovalCard } from "./ApprovalCard";
import { OfferCard } from "../settings/Catalog";
import { Markdown } from "./Markdown";
import { Avatar, useAvatar } from "../avatar/Avatar";
import { AgentsView, useAgents } from "../agents/AgentsView";
import { RemindersView, useReminders } from "../reminders/RemindersView";
import { DEV, NAME } from "../flavor";

const PAGE = 50;
const SUGGESTIONS = ["What time is it in Tokyo?", "What day is it today?"];

function pingTime(at: string): string {
  const d = new Date(at);
  if (!at || Number.isNaN(d.getTime())) return "bleep";
  return `bleep, ${new Intl.DateTimeFormat("en-US", { hour: "numeric", minute: "2-digit" }).format(d)}`;
}

/** Connectors the model offered in this turn, once each. */
function offers(m: Message): string[] {
  const ids = m.tool_steps.filter((s) => s.name === "connector_offer" && s.ok).map((s) => (s.args as { id?: unknown })?.id);
  return [...new Set(ids.filter((x): x is string => typeof x === "string"))];
}

function Bubble({ m }: { m: Message }) {
  if (m.kind === "divider") return <div className="divider" role="separator" aria-label={m.content}><span>{m.content}</span></div>;
  if (m.kind === "ping") return <div className="msg ping"><span className="tag">{pingTime(m.created_at)}</span>{m.content}</div>;
  if (m.role === "system") return <div className="msg sys">{m.content}</div>;
  if (m.role === "user" && m.kind === "scheduled") return <div className="msg sched"><span className="tag">scheduled</span>{m.content}</div>;
  if (m.role === "user") return <div className="msg me">{m.content}</div>;
  const waiting = m.status === "streaming" && !m.content && !m.tool_steps.length;
  return (
    <div className={`msg bot ${m.status === "error" ? "error" : ""}`}>
      <ToolSteps steps={m.tool_steps} />
      {m.approvals?.map((a) => <ApprovalCard key={a.id} a={a} />)}
      {offers(m).map((id) => <OfferCard key={id} id={id} />)}
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
  const avatar = useAvatar();
  const [health, setHealth] = useState<RuntimeHealth | null>(null);
  const [draft, setDraft] = useState("");
  const [sending, setSending] = useState(false);
  const [noMore, setNoMore] = useState(false);
  const [sendError, setSendError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [compacting, setCompacting] = useState(false);
  const [ctx, setCtx] = useState<ChatContext | null>(null);
  const [big, setBig] = useState(false);
  const log = useRef<HTMLDivElement>(null);
  const input = useRef<HTMLTextAreaElement>(null);
  const stick = useRef(true);
  const keepFrom = useRef<number | null>(null);
  const [view, setView] = useState<"chat" | "agents" | "reminders">("chat");
  // After the wake phrase: listening for the request.
  const [listening, setListening] = useState(false);
  // Replies to spoken requests are read aloud when they finish.
  const spoken = useRef(new Set<number>());
  const sendRef = useRef<(text: string, byVoice?: boolean) => Promise<void>>(async () => {});
  const agents = useAgents();
  const reminders = useReminders();
  const due = reminders.list.filter((r) => r.status === "fired").length;
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
    const unPing = onPing((message) => dispatch({ type: "ping", message }));
    const unBig = onChatExpanded(setBig);
    api.chatContext().then(setCtx).catch(() => {});
    const unCompact = onCompacting((on) => {
      setCompacting(on);
      if (!on) {
        // The divider and the new session are in the store now.
        api.history(null, PAGE).then((messages) => dispatch({ type: "history", messages, mode: "replace" })).catch(() => {});
        api.chatContext().then(setCtx).catch(() => {});
      }
    });
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
      void unPing.then((f) => f());
      void unBig.then((f) => f());
      void unCompact.then((f) => f());
      void unFocus.then((f) => f());
      window.removeEventListener("keydown", onKey);
    };
  }, [checkHealth]);

  useEffect(() => {
    const un = onVoice((e) => {
      if (e.kind === "wake") {
        setView("chat");
        setListening(true);
        void api.voiceStopSpeaking();
      } else {
        setListening(false);
        if (e.kind === "heard") void sendRef.current(e.text, true);
      }
    });
    return () => void un.then((f) => f());
  }, []);

  // A spoken request's reply is read aloud once it is done.
  useEffect(() => {
    for (const id of spoken.current) {
      const m = state.messages.find((x) => x.id === id);
      if (!m || m.status === "streaming") continue;
      spoken.current.delete(id);
      if (m.status === "done" && m.content.trim()) void api.voiceSpeak(m.content);
    }
  }, [state.messages]);

  // After each reply the window may be known for the first time, which can move the threshold.
  useEffect(() => {
    if (state.streamingId === null) api.chatContext().then(setCtx).catch(() => {});
  }, [state.streamingId]);

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
  const busy = sending || compacting || state.streamingId !== null;
  const used = state.context?.used ?? ctx?.used ?? null;
  // Near compaction, the gear shows a dot and opens Settings > Usage, where the numbers live.
  const ctxHigh = !!ctx && used != null && meterLevel(used, ctx.threshold) === "high";
  const canSend = ok && !busy && draft.trim().length > 0;

  const send = async (text: string, byVoice = false) => {
    const t = text.trim();
    if (!t) return;
    if (!ok || busy) {
      // A spoken request that cannot go now waits in the box, not lost.
      if (byVoice) setDraft(t);
      return;
    }
    setSending(true);
    stick.current = true;
    void api.voiceStopSpeaking();
    try {
      const assistantId = await api.send(t);
      if (byVoice) spoken.current.add(assistantId);
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

  sendRef.current = send;

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
        <span className="head-mark">
          <span className="head-droid">{avatar && <Avatar cfg={avatar} still />}</span>
          {DEV && <span className="head-dev" aria-hidden="true">dev</span>}
        </span>
        {/* The tabs and buttons need the room, so the buddy's head is the brand mark here. */}
        <h1 className="sr-only">{NAME}</h1>
        <div className="views" role="tablist" aria-label="View">
          <button role="tab" className="view-tab" aria-selected={view === "chat"} onClick={() => setView("chat")}>Chat</button>
          <button role="tab" className="view-tab" aria-selected={view === "agents"} aria-label={waiting ? `Agents, ${waiting} need you` : "Agents"} onClick={() => setView("agents")}>
            Agents{waiting > 0 && <span className="view-count" aria-hidden="true">{waiting}</span>}
          </button>
          <button role="tab" className="view-tab icon" aria-selected={view === "reminders"} aria-label={due ? `Reminders, ${due} due` : "Reminders"} title="Reminders" onClick={() => setView("reminders")}>
            <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M4 11.5V7a4 4 0 0 1 8 0v4.5l1 1H3l1-1ZM6.5 14h3" /></svg>
            {due > 0 && <span className="view-count" aria-hidden="true">{due}</span>}
          </button>
        </div>
        <span className="spacer" data-tauri-drag-region />
        <button
          className="iconbtn gear"
          aria-label={ctxHigh ? "Settings, chat context is near compaction" : "Settings"}
          title="Settings"
          onClick={() => void api.openSettings(ctxHigh ? "usage" : undefined)}
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
            <path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z" />
            <circle cx="12" cy="12" r="3" />
          </svg>
          {ctxHigh && <span className="view-dot" aria-hidden="true" />}
        </button>
        <button
          className="iconbtn"
          aria-label={big ? "Shrink chat" : "Enlarge chat"}
          aria-pressed={big}
          title={big ? "Shrink" : "Enlarge"}
          onClick={() => {
            setBig(!big);
            void api.setChatExpanded(!big);
          }}
        >
          <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
            {big ? <path d="M2.5 9.5h4v4M13.5 6.5h-4v-4M6.5 9.5 2 14M9.5 6.5 14 2" /> : <path d="M9.5 2.5h4v4M6.5 13.5h-4v-4M13.5 2.5 9 7M2.5 13.5 7 9" />}
          </svg>
        </button>
        <button className="iconbtn" aria-label="Close chat" title="Close (Esc)" onClick={() => void api.hideChat()}>
          <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true"><path d="M4 4l8 8M12 4l-8 8" /></svg>
        </button>
      </header>

      {view === "agents" && <AgentsView view={agents.view} refresh={() => void agents.refresh()} />}
      {view === "reminders" && <RemindersView list={reminders.list} refresh={() => void reminders.refresh()} />}

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
        {compacting && <div className="msg sys" role="status">Compacting... Facts go to memory first. Your history stays here.</div>}
      </div>

      {view === "chat" && ok && state.messages.length === 0 && (
        <div className="sugs">
          {SUGGESTIONS.map((s) => <button key={s} onClick={() => void send(s)} disabled={busy}>{s}</button>)}
        </div>
      )}

      <form className={`composer ${listening ? "listening" : ""}`} hidden={view !== "chat"} onSubmit={(e) => { e.preventDefault(); void send(draft); }}>
        <label htmlFor="msg" className="sr-only">Message bleep</label>
        <textarea
          id="msg"
          ref={input}
          rows={1}
          value={draft}
          placeholder={listening ? "Listening..." : compacting ? "Compacting..." : ok ? "Ask bleep anything about your day" : "Sign in to chat"}
          disabled={!ok || busy}
          onChange={(e) => {
            setDraft(e.target.value);
            setListening(false);
          }}
          onKeyDown={onKeyDown}
          autoFocus
        />
        <button type="submit" className="send" aria-label="Send" title="Send (Enter)" disabled={!canSend}>
          <svg viewBox="0 0 20 20" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
            <path d="M10 16V4M4.5 9.5 10 4l5.5 5.5" />
          </svg>
        </button>
      </form>
    </div>
  );
}
