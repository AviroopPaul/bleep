import { useCallback, useEffect, useLayoutEffect, useReducer, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  AlarmClock, ArrowDown, ArrowUp, Bell, BellRing, Bot, Calendar, Check, Copy, Mail, Maximize2, MessageCircle, Mic, Minimize2,
  RefreshCw, Settings, Sparkles, Square, Volume2, X, type LucideIcon,
} from "lucide-react";
import {
  api, healthOk, onApproval, onChatEvent, onChatExpanded, onCompacting, onConnectors, onNotice, onPing, onVoice,
  type ChatContext, type ConnectorsView, type Message, type RuntimeHealth,
} from "../ipc";
import { meterLevel } from "../settings/memoryText";
import { chatReducer, emptyChat, formatBackAt } from "./chatReducer";
import { greeting, reachLine, suggestions, type SuggestionIcon } from "./chatText";
import { ToolSteps } from "./ToolSteps";
import { ApprovalCard } from "./ApprovalCard";
import { OfferCard } from "../settings/Catalog";
import { Markdown } from "./Markdown";
import { Avatar, useAvatar } from "../avatar/Avatar";
import { AgentsView, useAgents } from "../agents/AgentsView";
import { RemindersView, useReminders } from "../reminders/RemindersView";
import { BADGE, NAME } from "../flavor";
import "./chat.css";

const PAGE = 50;
const SUG_ICONS: Record<SuggestionIcon, LucideIcon> = { calendar: Calendar, mail: Mail, reminder: AlarmClock, agents: Bot, help: Sparkles };

function clock(at: string): string | null {
  const d = new Date(at);
  if (!at || Number.isNaN(d.getTime())) return null;
  return new Intl.DateTimeFormat("en-US", { hour: "numeric", minute: "2-digit" }).format(d);
}

const pingTime = (at: string) => {
  const t = clock(at);
  return t ? `bleep, ${t}` : "bleep";
};

/** Connectors the model offered in this turn, once each. */
function offers(m: Message): string[] {
  const ids = m.tool_steps.filter((s) => s.name === "connector_offer" && s.ok).map((s) => (s.args as { id?: unknown })?.id);
  return [...new Set(ids.filter((x): x is string => typeof x === "string"))];
}

/** Copy, read aloud and retry under a finished reply. */
function ReplyActions({ m, onRetry }: { m: Message; onRetry?: () => void }) {
  const [copied, setCopied] = useState(false);
  const copy = () => {
    void navigator.clipboard.writeText(m.content).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1200);
    }).catch(() => {});
  };
  const t = clock(m.created_at);
  return (
    <div className="msg-acts">
      {m.content && <button type="button" aria-label={copied ? "Copied" : "Copy reply"} title="Copy" onClick={copy}>{copied ? <Check aria-hidden="true" /> : <Copy aria-hidden="true" />}</button>}
      {m.content && <button type="button" aria-label="Read aloud" title="Read aloud" onClick={() => void api.voiceSpeak(m.content, true)}><Volume2 aria-hidden="true" /></button>}
      {onRetry && <button type="button" aria-label="Ask again" title="Ask again" onClick={onRetry}><RefreshCw aria-hidden="true" /></button>}
      {t && <time dateTime={m.created_at}>{t}</time>}
    </div>
  );
}

function Bubble({ m, onRetry }: { m: Message; onRetry?: () => void }) {
  if (m.kind === "divider") return <div className="divider" role="separator" aria-label={m.content}><span>{m.content}</span></div>;
  if (m.kind === "ping") {
    return (
      <div className="msg ping">
        <span className="ping-ico" aria-hidden="true"><BellRing /></span>
        <span className="ping-body"><span className="ping-text">{m.content}</span><span className="tag">{pingTime(m.created_at)}</span></span>
      </div>
    );
  }
  if (m.role === "system") return <div className="msg sys">{m.content}</div>;
  if (m.role === "user" && m.kind === "scheduled") return <div className="msg sched"><span className="tag">scheduled</span>{m.content}</div>;
  if (m.role === "user") return <div className="msg me">{m.content}</div>;
  const waiting = m.status === "streaming" && !m.content && !m.tool_steps.length;
  const finished = m.status !== "streaming" && (m.content.trim().length > 0 || !!m.error);
  return (
    <>
      <div className={`msg bot ${m.status === "error" ? "error" : ""}`}>
        <ToolSteps steps={m.tool_steps} />
        {m.approvals?.map((a) => <ApprovalCard key={a.id} a={a} />)}
        {offers(m).map((id) => <OfferCard key={id} id={id} />)}
        {waiting ? (
          <span className="typing shimmer" aria-label="bleep is thinking">Thinking...</span>
        ) : (
          m.content && <span className="text"><Markdown text={m.content} /></span>
        )}
        {m.error && <span className="msg-err" role="alert">{m.error}</span>}
        {m.status === "interrupted" && <span className="note">Stopped</span>}
      </div>
      {finished && <ReplyActions m={m} onRetry={onRetry} />}
    </>
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
  const [conns, setConns] = useState<ConnectorsView | null>(null);
  // Scrolled up, away from the newest message: show the jump button.
  const [away, setAway] = useState(false);
  const log = useRef<HTMLDivElement>(null);
  const input = useRef<HTMLTextAreaElement>(null);
  const stick = useRef(true);
  const keepFrom = useRef<number | null>(null);
  const [view, setView] = useState<"chat" | "agents" | "reminders">("chat");
  // After the wake phrase or the mic button: listening for the request.
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
    const loadConns = () => api.connectorsList().then(setConns).catch(() => {});
    void loadConns();
    const unConns = onConnectors(() => void loadConns());
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
      void unConns.then((f) => f());
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

  // The box grows with the text, up to its max height.
  useLayoutEffect(() => {
    const el = input.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${el.scrollHeight}px`;
  }, [draft]);

  const onScroll = () => {
    const el = log.current;
    if (!el) return;
    stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
    setAway(!stick.current);
    if (el.scrollTop < 30 && !noMore && state.messages.length) {
      setNoMore(true);
      api.history(state.messages[0].id, PAGE).then((older) => {
        keepFrom.current = el.scrollHeight - el.scrollTop;
        dispatch({ type: "history", messages: older, mode: "prepend" });
        setNoMore(older.length < PAGE);
      });
    }
  };

  const toLatest = () => {
    stick.current = true;
    setAway(false);
    log.current?.scrollTo({ top: log.current.scrollHeight, behavior: "smooth" });
  };

  const ok = healthOk(health);
  const streaming = state.streamingId !== null;
  const busy = sending || compacting || streaming;
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
    setAway(false);
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
        // The core may have saved the turn with its error; then that row shows it, once.
        api.history(null, PAGE).then((messages) => {
          dispatch({ type: "history", messages, mode: "replace" });
          const last = messages[messages.length - 1];
          if (!(last?.role === "assistant" && last.status === "error")) setSendError(msg);
        }).catch(() => setSendError(msg));
      }
    } finally {
      setSending(false);
      input.current?.focus();
    }
  };

  sendRef.current = send;

  const stop = async () => {
    const id = state.streamingId;
    if (id === null) return;
    void api.voiceStopSpeaking();
    await api.chatStop().catch(() => false);
    dispatch({ type: "stopped", messageId: id });
    input.current?.focus();
  };

  // Listens for a request without the wake phrase. With voice off, Settings explains how to turn it on.
  const mic = async () => {
    if (listening) {
      setListening(false);
      await api.voiceListen(false).catch(() => false);
      return;
    }
    const on = await api.voiceListen(true).catch(() => false);
    if (!on) void api.openSettings("voice");
  };

  // Ask again: the user message right before this reply.
  const retryFor = (i: number): (() => void) | undefined => {
    for (let j = i - 1; j >= 0; j--) {
      const u = state.messages[j];
      if (u.role === "user" && u.kind === "chat") return () => void send(u.content);
      if (u.role === "assistant") return undefined;
    }
    return undefined;
  };

  const onKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
      e.preventDefault();
      void send(draft);
    }
  };

  const limited = state.rateLimitedUntil && state.rateLimitedUntil * 1000 > Date.now();
  const connected = (conns?.builtin ?? []).filter((c) => c.status.state === "connected");
  const ids = new Set(connected.map((c) => c.id));
  const reach = [ids.has("calendar") && "Calendar", ids.has("gmail") && "Gmail"].filter((x): x is string => !!x);
  const sugs = suggestions(ids, agents.view.sessions.length, waiting);
  const empty = ok && state.messages.length === 0;

  return (
    <div className="panel">
      <header className="panel-head" data-tauri-drag-region>
        <span className="head-mark">
          <span className="head-droid">{avatar && <Avatar cfg={avatar} still />}</span>
          {BADGE && <span className="head-dev" aria-hidden="true">{BADGE}</span>}
        </span>
        {/* The tabs and buttons need the room, so the buddy's head is the brand mark here. */}
        <h1 className="sr-only">{NAME}</h1>
        <div className="views" role="tablist" aria-label="View">
          <button role="tab" className="view-tab" aria-selected={view === "chat"} onClick={() => setView("chat")}>
            <MessageCircle aria-hidden="true" />Chat
          </button>
          <button role="tab" className="view-tab" aria-selected={view === "agents"} aria-label={waiting ? `Agents, ${waiting} need you` : "Agents"} onClick={() => setView("agents")}>
            <Bot aria-hidden="true" />Agents{waiting > 0 && <span className="view-count" aria-hidden="true">{waiting}</span>}
          </button>
          <button role="tab" className="view-tab icon" aria-selected={view === "reminders"} aria-label={due ? `Reminders, ${due} due` : "Reminders"} title="Reminders" onClick={() => setView("reminders")}>
            <Bell aria-hidden="true" />
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
          <Settings aria-hidden="true" />
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
          {big ? <Minimize2 aria-hidden="true" /> : <Maximize2 aria-hidden="true" />}
        </button>
        <button className="iconbtn" aria-label="Close chat" title="Close (Esc)" onClick={() => void api.hideChat()}>
          <X aria-hidden="true" />
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

      <div className="log-wrap" hidden={view !== "chat"}>
        <div className="log" ref={log} onScroll={onScroll} aria-live="polite">
          {empty && (
            <div className="hello">
              <span className="hello-droid" aria-hidden="true">{avatar && <Avatar cfg={avatar} still />}</span>
              <h2>{greeting(new Date())}</h2>
              <p>{reachLine(reach)}</p>
              <div className="sugs">
                {sugs.map((s) => {
                  const Icon = SUG_ICONS[s.icon];
                  return (
                    <button key={s.text} type="button" onClick={() => void send(s.text)} disabled={busy}>
                      <Icon aria-hidden="true" /><span>{s.text}</span><small>{s.source}</small>
                    </button>
                  );
                })}
              </div>
            </div>
          )}
          {state.messages.map((m, i) => <Bubble key={m.id} m={m} onRetry={busy ? undefined : retryFor(i)} />)}
          {state.loose.map((a) => <ApprovalCard key={a.id} a={a} />)}
          {limited && <div className="msg sys">{formatBackAt(state.rateLimitedUntil!)}</div>}
          {sendError && <div className="msg sys err" role="alert">{sendError}</div>}
          {notice && <div className="msg sys" role="status">{notice}</div>}
          {compacting && <div className="msg sys" role="status">Compacting... Facts go to memory first. Your history stays here.</div>}
        </div>
        {away && (
          <button type="button" className="to-latest" onClick={toLatest}>
            <ArrowDown aria-hidden="true" />{streaming ? "New reply" : "Jump to latest"}
          </button>
        )}
      </div>

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
        <button
          type="button"
          className={`mic ${listening ? "on" : ""}`}
          aria-label={listening ? "Stop listening" : "Talk to bleep"}
          aria-pressed={listening}
          title={listening ? "Stop listening" : "Talk"}
          disabled={!ok || busy}
          onClick={() => void mic()}
        >
          <Mic aria-hidden="true" />
        </button>
        {streaming ? (
          <button type="button" className="send stop" aria-label="Stop" title="Stop" onClick={() => void stop()}>
            <Square aria-hidden="true" />
          </button>
        ) : (
          <button type="submit" className="send" aria-label="Send" title="Send (Enter)" disabled={!canSend}>
            <ArrowUp aria-hidden="true" />
          </button>
        )}
      </form>
    </div>
  );
}
