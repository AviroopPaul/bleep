import { DEV } from "../flavor";
import { useCallback, useEffect, useRef, useState } from "react";
import { MailSettings } from "./MailSettings";
import { Activity } from "./Activity";
import { AgentSettingsTab } from "./AgentSettings";
import { MemorySettingsTab } from "./MemorySettings";
import { ConnectorsSettingsTab } from "./ConnectorsSettings";
import { ModelSettingsTab } from "./ModelSettings";
import { UpdatesSettingsTab } from "./UpdatesSettings";
import { VoiceSettingsTab } from "./VoiceSettings";
import { AvatarSettingsTab } from "./AvatarSettings";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api, onGoogleStatus, onSettingsGoto, type ChatContext, type GoogleState, type LinkName } from "../ipc";
import { Avatar, useAvatar } from "../avatar/Avatar";
import { UsageView } from "../usage/UsageView";

function statusText(g: GoogleState | null): string {
  if (!g) return "";
  switch (g.status.state) {
    case "needs_setup": return "Not set up";
    case "needs_sign_in": return g.expired ? "Sign-in expired. Connect again." : "Ready to connect";
    case "connected": return `Connected as ${g.status.account}`;
    case "error": return `Error: ${g.status.message}`;
  }
}

const STEPS: { text: string; links: [LinkName, string][] }[] = [
  { text: "Create a project in Google Cloud. Any name works.", links: [["console", "Create a Google Cloud project"]] },
  { text: "Turn on the two APIs bleep uses.", links: [["calendar_api", "Turn on the Calendar API"], ["gmail_api", "Turn on the Gmail API"]] },
  { text: "Set up the consent screen with an app name and your email. Then, on the Audience page, set it to In production so sign-in does not expire every 7 days.", links: [["consent", "Open the consent screen"]] },
  { text: "Create an OAuth client. Pick the type Desktop app.", links: [["credentials", "Create an OAuth client"]] },
  { text: "Paste the client ID and secret below, save, then press Connect Google above.", links: [] },
];

export function GoogleSettings() {
  const [g, setG] = useState<GoogleState | null>(null);
  const [clientId, setClientId] = useState("");
  const [secret, setSecret] = useState("");
  const [connecting, setConnecting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(() => api.googleState().then((s) => {
    setG(s);
    setClientId((c) => c || s.client_id);
  }).catch(() => {}), []);

  useEffect(() => {
    void load();
    const un = onGoogleStatus(() => void load());
    const unFocus = getCurrentWindow().onFocusChanged(({ payload }) => payload && void load());
    return () => {
      void un.then((f) => f());
      void unFocus.then((f) => f());
    };
  }, [load]);

  const canSave = clientId.trim().length > 0 && (secret.trim().length > 0 || !!g?.has_secret);
  const save = async () => {
    setError(null);
    try {
      setG(await api.googleSaveClient(clientId.trim(), secret.trim()));
      setSecret("");
    } catch (e) {
      setError(String(e));
    }
  };
  const connect = async () => {
    setError(null);
    setConnecting(true);
    try {
      await api.googleConnect();
      await load();
    } catch (e) {
      setError(String(e));
    } finally {
      setConnecting(false);
    }
  };
  const useBuiltin = async () => {
    setError(null);
    try {
      setG(await api.googleUseBuiltin());
      setClientId("");
      setSecret("");
    } catch (e) {
      setError(String(e));
    }
  };
  const disconnect = async () => {
    setError(null);
    try {
      await api.googleDisconnect();
      await load();
    } catch (e) {
      setError(String(e));
    }
  };

  const state = g?.status.state;
  return (
    <section className="set-card" aria-labelledby="google-h">
        <div className="set-head">
          <h2 id="google-h">Account</h2>
          <span className={`pill ${state ?? ""}`} role="status">{statusText(g)}</span>
        </div>
        <p className="set-lead">
          {g && !g.builtin && !g.using_own
            ? "This build has no Google app built in. Set up your own below, once."
            : "Connect your Google account so bleep can read your calendar. Changes always wait for your OK."}
        </p>
        <div className="acts main-acts">
          {state === "connected" ? (
            <button type="button" className="btn" onClick={() => void disconnect()}>Disconnect</button>
          ) : (
            <button type="button" className="btn pri" disabled={connecting || state === "needs_setup" || !g} onClick={() => void connect()}>
              {connecting ? "Finish sign-in in your browser…" : g?.expired ? "Connect again" : "Connect Google"}
            </button>
          )}
        </div>
        {error && <p className="set-err" role="alert">{error}</p>}
        {g && (
          <details className="own" open={!g.builtin && !g.using_own}>
            <summary>Use your own Google Cloud app</summary>
            <p className="set-lead">For developers. Most people never need this. Setup takes about 15 minutes.</p>
            <ol className="steps">
              {STEPS.map((st, i) => (
                <li key={i}>
                  <p>{st.text}</p>
                  {st.links.length > 0 && (
                    <div className="links">
                      {st.links.map(([name, label]) => (
                        <button key={name} className="linkbtn" onClick={() => void api.openLink(name)}>{label}</button>
                      ))}
                    </div>
                  )}
                </li>
              ))}
            </ol>
            <form className="fields" onSubmit={(e) => { e.preventDefault(); if (canSave) void save(); }}>
              <label htmlFor="cid">Client ID</label>
              <input id="cid" value={clientId} onChange={(e) => setClientId(e.target.value)} spellCheck={false} autoComplete="off" />
              <label htmlFor="csec">Client secret</label>
              <input id="csec" type="password" value={secret} onChange={(e) => setSecret(e.target.value)} placeholder={g.has_secret ? "Saved. Leave empty to keep it." : ""} autoComplete="off" />
              <div className="acts">
                <button type="submit" className="btn" disabled={!canSave}>Save</button>
                {g.using_own && g.builtin && (
                  <button type="button" className="btn" onClick={() => void useBuiltin()}>Use bleep's app</button>
                )}
              </div>
            </form>
          </details>
        )}
      </section>
  );
}

type Tab = "Model" | "Avatar" | "Voice" | "Connectors" | "Google" | "Mail" | "Agents" | "Memory" | "Usage" | "Activity" | "Updates";

/** The sidebar, in sections. */
const GROUPS: { name: string; tabs: Tab[] }[] = [
  { name: "General", tabs: ["Model", "Avatar", "Voice"] },
  { name: "Connections", tabs: ["Connectors", "Google", "Mail", "Agents"] },
  { name: "Your data", tabs: ["Memory", "Usage", "Activity"] },
  { name: "App", tabs: ["Updates"] },
];
const TABS: Tab[] = GROUPS.flatMap((g) => g.tabs);
const GROUP_OF = Object.fromEntries(GROUPS.flatMap((g) => g.tabs.map((t) => [t, g.name]))) as Record<Tab, string>;

const LEDE: Record<Tab, string> = {
  Model: "Choose what runs bleep and which models it uses.",
  Avatar: "Pick your buddy. Your menu bar icon follows along.",
  Voice: "Say the wake word to talk. bleep can speak back.",
  Connectors: "Turn on the apps bleep can use, or add any remote MCP server.",
  Google: "Your Google account for Gmail, Calendar and Drive.",
  Mail: "Tell bleep whose mail matters and what to skip.",
  Agents: "How bleep watches and helps your coding sessions.",
  Memory: "What bleep remembers about you, and when it compacts the chat.",
  Usage: "What bleep used and when. It all stays on this Mac.",
  Activity: "A log of what bleep did for you.",
  Updates: "Get new versions of bleep.",
};

const ICON: Record<Tab, string> = {
  Model: '<rect x="4" y="4" width="8" height="8" rx="2"/><path d="M6.5 1.5v2M9.5 1.5v2M6.5 12.5v2M9.5 12.5v2M1.5 6.5h2M1.5 9.5h2M12.5 6.5h2M12.5 9.5h2"/>',
  Avatar: '<circle cx="8" cy="8" r="6.2"/><path d="M6 6.4v1.2M10 6.4v1.2M5.7 10a3.2 3.2 0 0 0 4.6 0"/>',
  Voice: '<rect x="5.5" y="1.5" width="5" height="8.5" rx="2.5"/><path d="M3 8a5 5 0 0 0 10 0M8 13v1.5"/>',
  Connectors: '<path d="M6 1.5v3M10 1.5v3M4 4.5h8V7a4 4 0 0 1-8 0zM8 11v3.5"/>',
  Google: '<circle cx="8" cy="5.4" r="2.9"/><path d="M2.6 14.2a5.4 5.4 0 0 1 10.8 0"/>',
  Mail: '<rect x="1.5" y="3" width="13" height="10" rx="2"/><path d="m2.2 4.2 5.8 4.6 5.8-4.6"/>',
  Agents: '<rect x="1.5" y="2.5" width="13" height="11" rx="2"/><path d="m4.5 6 2 2-2 2M8.5 10.5h3"/>',
  Memory: '<path d="M3.5 2.5h9v11L8 10.5l-4.5 3z"/>',
  Usage: '<path d="M3 13.5V9M8 13.5V3M13 13.5V6.5"/>',
  Activity: '<circle cx="8" cy="8" r="6.2"/><path d="M8 4.6V8l2.4 1.5"/>',
  Updates: '<path d="M8 2v8M4.5 6.5 8 10l3.5-3.5M2.5 13.5h11"/>',
};

function Icon({ t }: { t: Tab }) {
  return (
    <span className="tab-ico" aria-hidden="true">
      <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" dangerouslySetInnerHTML={{ __html: ICON[t] }} />
    </span>
  );
}

/** `#settings/usage` opens a section; so does `settings://goto` when the window is already open. */
function tabFrom(name: string | null | undefined): Tab | null {
  const n = (name ?? "").toLowerCase();
  return TABS.find((t) => t.toLowerCase() === n) ?? null;
}

export function Settings() {
  const [tab, setTab] = useState<Tab>(() => tabFrom(window.location.hash.split("/")[1]) ?? "Model");
  const avatar = useAvatar();
  const tabs = useRef<(HTMLButtonElement | null)[]>([]);
  useEffect(() => {
    const un = onSettingsGoto((name) => {
      const t = tabFrom(name);
      if (t) setTab(t);
    });
    return () => void un.then((f) => f());
  }, []);
  const go = (i: number) => {
    const next = (i + TABS.length) % TABS.length;
    setTab(TABS[next]);
    tabs.current[next]?.focus();
  };
  const onKey = (e: React.KeyboardEvent, i: number) => {
    const to: Record<string, number> = { ArrowDown: i + 1, ArrowRight: i + 1, ArrowUp: i - 1, ArrowLeft: i - 1, Home: 0, End: TABS.length - 1 };
    if (!(e.key in to)) return;
    e.preventDefault();
    go(to[e.key]);
  };
  return (
    <main className="settings">
      <div className="set-side">
        <div className="set-brand">
          <span className="set-bud">{avatar && <Avatar cfg={avatar} still />}</span>
          <h1>Settings{DEV && <span className="dev-tag">(dev)</span>}</h1>
        </div>
        <div className="tabs" role="tablist" aria-label="Settings" aria-orientation="vertical">
          {GROUPS.map((g) => (
            <div className="tab-group" role="presentation" key={g.name}>
              <p className="tab-group-h" aria-hidden="true">{g.name}</p>
              {g.tabs.map((t) => {
                const i = TABS.indexOf(t);
                return (
                  <button
                    key={t}
                    ref={(el) => { tabs.current[i] = el; }}
                    role="tab"
                    id={`tab-${t}`}
                    className="tab"
                    aria-label={t}
                    aria-selected={tab === t}
                    aria-controls={`panel-${t}`}
                    tabIndex={tab === t ? 0 : -1}
                    onClick={() => setTab(t)}
                    onKeyDown={(e) => onKey(e, i)}
                  >
                    <Icon t={t} />
                    <span>{t}</span>
                  </button>
                );
              })}
            </div>
          ))}
        </div>
      </div>
      <div role="tabpanel" id={`panel-${tab}`} aria-labelledby={`tab-${tab}`} className="tab-panel">
        <div className="set-page">
          <header className="set-top">
            <p className="set-kicker">{GROUP_OF[tab]}</p>
            <h2 className="set-title">{tab}</h2>
            <p className="set-sub">{LEDE[tab]}</p>
          </header>
          {tab === "Model" && <ModelSettingsTab />}
          {tab === "Avatar" && <AvatarSettingsTab />}
          {tab === "Voice" && <VoiceSettingsTab />}
          {tab === "Google" && <GoogleSettings />}
          {tab === "Mail" && <MailSettings />}
          {tab === "Memory" && <MemorySettingsTab />}
          {tab === "Connectors" && <ConnectorsSettingsTab />}
          {tab === "Agents" && <AgentSettingsTab />}
          {tab === "Usage" && <UsageSettingsTab />}
          {tab === "Activity" && <Activity />}
          {tab === "Updates" && <UpdatesSettingsTab />}
        </div>
      </div>
    </main>
  );
}

/** Usage in Settings: the chat context comes from the core, the rest from the local database. */
function UsageSettingsTab() {
  const [ctx, setCtx] = useState<ChatContext | null>(null);
  useEffect(() => {
    api.chatContext().then(setCtx).catch(() => {});
  }, []);
  return <UsageView ctx={ctx} />;
}
