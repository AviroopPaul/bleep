import { useEffect, useState } from "react";
import { api, onUpdate, type UpdateView } from "../ipc";
import { checkedText, phaseText } from "./updateText";

const PILL: Record<string, string> = { ready: "connected", available: "needs_sign_in", error: "error" };

export function UpdatesSettingsTab() {
  const [v, setV] = useState<UpdateView | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.updateState().then(setV).catch(() => {});
    const un = onUpdate(setV);
    return () => void un.then((f) => f());
  }, []);

  const run = async (f: () => Promise<UpdateView>) => {
    setError(null);
    try {
      setV(await f());
    } catch (e) {
      setError(String(e));
    }
  };

  if (!v) return null;
  const p = v.phase;
  const busy = p.state === "checking" || p.state === "downloading";
  const avail = p.state === "available" ? p : null;

  return (
    <section className="set-card" aria-labelledby="upd-h">
      <div className="set-head">
        <h2 id="upd-h">bleep {v.current}</h2>
        {p.state !== "idle" && <span className={`pill ${PILL[p.state] ?? ""}`} role="status">{phaseText(p)}</span>}
      </div>
      <p className="set-lead">
        bleep checks for a new version every few hours. Updates are signed, and bleep checks the signature before it installs one.
      </p>
      <label className="toggle">
        <input type="checkbox" role="switch" checked={v.auto_install} onChange={(e) => void run(() => api.updateSetAuto(e.target.checked))} />
        <span>Install updates automatically</span>
      </label>
      {avail?.notes && (
        <details className="own" open>
          <summary>What is new in {avail.version}</summary>
          <p className="set-lead upd-notes">{avail.notes}</p>
        </details>
      )}
      <div className="acts main-acts">
        {p.state === "ready" ? (
          <button type="button" className="btn pri" onClick={() => void api.updateRestart()}>Restart now</button>
        ) : p.state === "available" ? (
          <button type="button" className="btn pri" onClick={() => void run(api.updateInstall)}>Install {p.version}</button>
        ) : (
          <button type="button" className="btn" disabled={busy} onClick={() => void run(api.updateCheck)}>
            {p.state === "checking" ? "Checking..." : "Check now"}
          </button>
        )}
      </div>
      <p className="set-lead" aria-live="polite">{checkedText(v.last_checked)}</p>
      {error && <p className="set-err" role="alert">{error}</p>}
    </section>
  );
}
