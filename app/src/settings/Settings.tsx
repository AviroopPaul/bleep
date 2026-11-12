import { useCallback, useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api, onGoogleStatus, type GoogleState, type LinkName } from "../ipc";

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

export function Settings() {
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
    <main className="settings">
      <h1>Settings</h1>
      <section className="set-card" aria-labelledby="google-h">
        <div className="set-head">
          <h2 id="google-h">Google</h2>
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
    </main>
  );
}
