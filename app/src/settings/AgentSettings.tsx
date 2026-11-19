import { useEffect, useState } from "react";
import { api, type AgentSettings } from "../ipc";

/** Install or remove bleep's entries in my agent config, only after a second tap. */
function Hookup(props: { title: string; installed: boolean; lead: string; consent: string; installLabel: string; removeLabel: string; onInstall: () => Promise<void>; onRemove: () => Promise<void>; extra?: React.ReactNode }) {
  const [asking, setAsking] = useState(false);
  const id = `ag-${props.title.replace(/\s/g, "").toLowerCase()}`;
  return (
    <section className="ag-set" role="region" aria-labelledby={id}>
      <div className="ag-set-head">
        <h3 id={id} className="mail-h">{props.title}</h3>
        <span className={`pill ${props.installed ? "connected" : ""}`} role="status">{props.installed ? "Installed" : "Not installed"}</span>
      </div>
      <p className="set-lead">{props.lead}</p>
      {props.extra}
      {props.installed ? (
        <button className="btn" onClick={() => void props.onRemove()}>{props.removeLabel}</button>
      ) : asking ? (
        <div className="consent">
          <p>{props.consent}</p>
          <div className="acts">
            <button className="btn pri" onClick={() => void props.onInstall().then(() => setAsking(false))}>Yes, install</button>
            <button className="btn" onClick={() => setAsking(false)}>Cancel</button>
          </div>
        </div>
      ) : (
        <button className="btn pri" onClick={() => setAsking(true)}>{props.installLabel}</button>
      )}
    </section>
  );
}

function Toggle({ label, checked, onChange }: { label: string; checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <label className="toggle">
      <input type="checkbox" role="switch" checked={checked} onChange={(e) => onChange(e.target.checked)} />
      <span>{label}</span>
    </label>
  );
}

const ANSWER_MODES: [AgentSettings["auto_answer"], string][] = [["off", "Off"], ["shadow", "Log only"], ["on", "On"]];
const ANSWER_HELP: Record<AgentSettings["auto_answer"], string> = {
  off: "bleep never answers for you.",
  shadow: "bleep decides and logs what it would send, next to what you typed. It sends nothing.",
  on: "bleep sends the first button right away and tells you what it sent.",
};

export function AgentSettingsTab({ onChange }: { onChange?: (s: AgentSettings) => void } = {}) {
  const [s, setS] = useState<AgentSettings | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    api.agentsSettings().then(setS).catch(() => {});
  }, []);
  const run = async (f: () => Promise<AgentSettings>) => {
    setError(null);
    try {
      const n = await f();
      setS(n);
      onChange?.(n);
    } catch (e) {
      setError(String(e));
    }
  };
  if (!s) return null;
  const set = (patch: Partial<AgentSettings>) => {
    const n = { ...s, ...patch };
    return run(() => api.agentsSet(n.auto_approve, n.screenshot_check, n.codex_watch, n.auto_answer));
  };
  return (
    <div className="mail-set">
      <Hookup
        title="Claude Code"
        installed={s.claude_hooks}
        lead="bleep sees when a Claude Code session waits on you. Anything bleep does not approve still shows its normal prompt in the terminal."
        consent="bleep adds hooks to ~/.claude/settings.json so it can see when a session waits on you. A backup is saved first. Remove takes them out again."
        installLabel="Install hooks"
        removeLabel="Remove hooks"
        onInstall={() => run(api.agentsInstallClaude)}
        onRemove={() => run(api.agentsRemoveClaude)}
      />
      <Hookup
        title="Codex"
        installed={s.codex_notify}
        lead={s.codex_running ? "The Codex background service is running." : "The Codex background service is not running. bleep never starts it."}
        consent="bleep adds itself in front of the notify program in ~/.codex/config.toml, so it hears when a Codex turn ends. Your program still runs first. A backup is saved first."
        installLabel="Install turn notices"
        removeLabel="Remove turn notices"
        onInstall={() => run(api.agentsInstallCodexNotify)}
        onRemove={() => run(api.agentsRemoveCodexNotify)}
        extra={<Toggle label="Watch Codex sessions" checked={s.codex_watch} onChange={(v) => void set({ codex_watch: v })} />}
      />
      <section aria-labelledby="ag-rules-h">
        <h3 id="ag-rules-h" className="mail-h">Approvals</h3>
        <p className="set-lead">With this on, bleep approves only what your rules allow, such as tests and reads inside the repo. Everything else waits for you.</p>
        <Toggle label="Approve safe asks by my rules" checked={s.auto_approve} onChange={(v) => void set({ auto_approve: v })} />
        <Toggle label="Check screenshots with a small model" checked={s.screenshot_check} onChange={(v) => void set({ screenshot_check: v })} />
        <button className="btn" onClick={() => void api.agentsRevealRules()}>Show rules file</button>
      </section>
      <section aria-labelledby="ag-answer-h">
        <h3 id="ag-answer-h" className="mail-h">Answers</h3>
        <p className="set-lead">When Claude ends a turn with a question and buttons, bleep can send its first button for you. Only go-aheads on your rules' answer list qualify, never ship, release, delete or taste calls. A small model can still say no.</p>
        <div className="choice" role="radiogroup" aria-label="Auto-answer">
          {ANSWER_MODES.map(([mode, label]) => (
            <label key={mode} className={`choice-opt ${s.auto_answer === mode ? "on" : ""}`}>
              <input type="radio" name="auto-answer" aria-label={label} checked={s.auto_answer === mode} onChange={() => void set({ auto_answer: mode })} />
              <span>
                <b>{label}</b>
                <small>{ANSWER_HELP[mode]}</small>
              </span>
            </label>
          ))}
        </div>
      </section>
      {error && <p className="set-err" role="alert">{error}</p>}
    </div>
  );
}
