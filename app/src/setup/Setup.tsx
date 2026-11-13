import { useCallback, useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api, healthOk, onGoogleStatus, type AgentSettings, type GoogleState, type RuntimeHealth } from "../ipc";
import { DROID_FULL } from "../droid/droidSvg";
import { GoogleSettings } from "../settings/Settings";
import { AgentSettingsTab } from "../settings/AgentSettings";

export const STEPS = ["Welcome", "Claude", "Google", "Agents", "Done"] as const;
const INSTALL_URL = "https://code.claude.com/docs/en/setup";

function Droid() {
  return <div className="su-droid droid" aria-hidden="true" dangerouslySetInnerHTML={{ __html: DROID_FULL }} />;
}

function Check({ ok, label }: { ok: boolean; label: string }) {
  return (
    <li className={`su-check ${ok ? "ok" : ""}`}>
      <span aria-hidden="true">{ok ? "✓" : "–"}</span>
      {label}
      <span className="sr-only">{ok ? " (done)" : " (not yet)"}</span>
    </li>
  );
}

/** Claude Code is bleep's brain: it must be installed and signed in once for bleep. */
function ClaudeStep({ health, recheck, checking }: { health: RuntimeHealth | null; recheck: () => void; checking: boolean }) {
  const h = health;
  return (
    <>
      <h2>Claude Code</h2>
      <p className="set-lead">bleep thinks with Claude, through Claude Code on your Mac. No API key and no extra bill.</p>
      <ul className="su-checks" aria-label="Claude Code status">
        <Check ok={!!h?.installed} label={h?.installed ? `Installed${h.version ? `, version ${h.version}` : ""}` : "Installed"} />
        <Check ok={!!h?.installed && !!h?.signed_in} label={h?.signed_in ? `Signed in${h.plan ? ` (${h.plan})` : ""}` : "Signed in for bleep"} />
      </ul>
      {h && !h.installed && (
        <div className="su-note">
          <p>Install Claude Code first. Then come back and press Check again.</p>
          <button className="linkbtn" onClick={() => void api.openUrl(INSTALL_URL)}>How to install Claude Code</button>
        </div>
      )}
      {h && h.installed && !h.signed_in && (
        <div className="su-note">
          <p>bleep keeps its own Claude sign-in, apart from yours. A Terminal window opens. Follow the steps there, then come back.</p>
          <button className="btn pri" onClick={() => void api.openLogin()}>Sign in to Claude</button>
        </div>
      )}
      {h?.hint && <p className="set-lead">{h.hint}</p>}
      <div className="acts">
        <button className="btn" disabled={checking} onClick={recheck}>{checking ? "Checking…" : "Check again"}</button>
      </div>
    </>
  );
}

function DoneStep({ health, google, agents }: { health: RuntimeHealth | null; google: GoogleState | null; agents: AgentSettings | null }) {
  return (
    <>
      <Droid />
      <h2 className="su-center">All set.</h2>
      <ul className="su-checks" aria-label="What is set up">
        <Check ok={healthOk(health)} label="Claude Code ready" />
        <Check ok={google?.status.state === "connected"} label={google?.status.state === "connected" ? `Google connected as ${google.status.account}` : "Google connected"} />
        <Check ok={!!agents?.claude_hooks} label="Claude Code hooks installed" />
        <Check ok={!!agents?.codex_notify} label="Codex turn notices installed" />
      </ul>
      <p className="set-lead su-center">
        Double-tap <kbd>⌥</kbd> Option anywhere to call me. Anything you skipped is in Settings, from the bleep icon in the menu bar.
      </p>
    </>
  );
}

export function Setup() {
  const [i, setI] = useState(0);
  const [health, setHealth] = useState<RuntimeHealth | null>(null);
  const [checking, setChecking] = useState(false);
  const [google, setGoogle] = useState<GoogleState | null>(null);
  const [agents, setAgents] = useState<AgentSettings | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [finishing, setFinishing] = useState(false);

  const recheck = useCallback(() => {
    setChecking(true);
    api.health().then(setHealth).catch(() => {}).finally(() => setChecking(false));
  }, []);
  const refreshAll = useCallback(() => {
    recheck();
    api.googleState().then(setGoogle).catch(() => {});
    api.agentsSettings().then(setAgents).catch(() => {});
  }, [recheck]);

  useEffect(() => {
    refreshAll();
    // Coming back from Terminal or the browser: look again.
    const un = getCurrentWindow().onFocusChanged(({ payload }) => payload && refreshAll());
    const unG = onGoogleStatus(() => void api.googleState().then(setGoogle).catch(() => {}));
    return () => {
      void un.then((f) => f());
      void unG.then((f) => f());
    };
  }, [refreshAll]);
  useEffect(() => {
    if (STEPS[i] === "Done") refreshAll();
  }, [i, refreshAll]);

  const step = STEPS[i];
  const last = i === STEPS.length - 1;
  const ready =
    step === "Claude" ? healthOk(health) :
    step === "Google" ? google?.status.state === "connected" :
    step === "Agents" ? !!agents?.claude_hooks || !!agents?.codex_notify : true;

  const finish = async () => {
    setError(null);
    setFinishing(true);
    try {
      await api.setupFinish();
    } catch (e) {
      setError(String(e));
      setFinishing(false);
    }
  };

  return (
    <main className="setup">
      <ol className="su-steps" aria-label="Setup steps">
        {STEPS.map((s, k) => (
          <li key={s} className={k === i ? "on" : k < i ? "past" : ""} aria-current={k === i ? "step" : undefined}>{s}</li>
        ))}
      </ol>
      <section className="su-body" aria-labelledby="su-h">
        <span id="su-h" className="sr-only">{step}</span>
        {step === "Welcome" && (
          <>
            <Droid />
            <h1 className="su-center">Hi, I'm bleep.</h1>
            <p className="set-lead su-center">
              I float on your Mac. I watch your Claude Code and Codex sessions, your mail and your calendar. I handle the small asks and tell you what needs you.
            </p>
            <p className="set-lead su-center su-small">Setup takes about two minutes. You can change all of it later in Settings.</p>
          </>
        )}
        {step === "Claude" && <ClaudeStep health={health} recheck={recheck} checking={checking} />}
        {step === "Google" && (
          <>
            <h2>Gmail and Calendar</h2>
            <p className="set-lead">bleep reads your mail and calendar to brief you, catch bills and prep meetings. It never sends, moves or deletes anything without your tap.</p>
            <GoogleSettings />
          </>
        )}
        {step === "Agents" && (
          <>
            <h2>Your coding agents</h2>
            <p className="set-lead">Let bleep see when Claude Code or Codex waits on you. Auto-approve stays off until you turn it on.</p>
            <AgentSettingsTab onChange={setAgents} />
          </>
        )}
        {step === "Done" && <DoneStep health={health} google={google} agents={agents} />}
        {error && <p className="set-err" role="alert">{error}</p>}
      </section>
      <footer className="su-foot">
        {i > 0 && !last ? <button className="btn" onClick={() => setI(i - 1)}>Back</button> : <span />}
        {last ? (
          <button className="btn pri" disabled={finishing} onClick={() => void finish()}>{finishing ? "Starting…" : "Start using bleep"}</button>
        ) : (
          <button className="btn pri" onClick={() => setI(i + 1)}>
            {step === "Welcome" ? "Let's set up" : ready ? "Next" : "Skip for now"}
          </button>
        )}
      </footer>
    </main>
  );
}
