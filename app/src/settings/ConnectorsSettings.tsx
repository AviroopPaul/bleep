import { useEffect, useState } from "react";
import { api, onConnectors, type ConnectorsView, type RemoteServer, type ToolMode } from "../ipc";
import { ADDED, authText, modeLabel, NEEDS_CLIENT, SIGNING_IN, statusText, toolCount } from "./connectorsText";
import { Catalog } from "./Catalog";

const MODES: ToolMode[] = ["ask", "auto", "off"];

function Server({ s, onError }: { s: RemoteServer; onError: (e: string | null) => void }) {
  const [busy, setBusy] = useState<string | null>(null);
  const [confirm, setConfirm] = useState(false);
  const [token, setToken] = useState("");
  const run = async (label: string, f: () => Promise<unknown>) => {
    onError(null);
    setBusy(label);
    try {
      await f();
    } catch (e) {
      onError(String(e));
    } finally {
      setBusy(null);
    }
  };
  const needsSignIn = s.status.state === "needs_sign_in";
  return (
    <div className="set-card conn-card" role="group" aria-label={s.name}>
      <div className="set-head">
        <h4 className="conn-name">{s.name}</h4>
        <span className={`pill ${s.status.state}`} role="status">{statusText(s.status)}</span>
      </div>
      <p className="conn-url">{s.url}</p>
      <p className="set-lead conn-meta">{authText(s.auth)}. {toolCount(s.tools)}.</p>
      {needsSignIn && s.auth === "token" && (
        <div className="fields">
          <label htmlFor={`tok-${s.name}`}>New token</label>
          <input id={`tok-${s.name}`} type="password" autoComplete="off" value={token} onChange={(e) => setToken(e.target.value)} />
        </div>
      )}
      <div className="acts conn-acts">
        {(needsSignIn || s.auth === "oauth") && (
          <button className={`btn sm ${needsSignIn ? "pri" : ""}`} disabled={busy !== null} onClick={() => void run("sign", () => api.remoteSignIn(s.name, s.auth === "token" ? token : null))}>
            {busy === "sign" ? "Waiting for the browser..." : "Sign in again"}
          </button>
        )}
        {busy === "sign" && s.auth === "oauth" && <button className="btn sm" onClick={() => void api.remoteCancelSignIn()}>Cancel</button>}
        <button className="btn sm" disabled={busy !== null} onClick={() => void run("refresh", () => api.remoteRefresh(s.name))}>
          {busy === "refresh" ? "Refreshing..." : "Refresh tools"}
        </button>
        {confirm ? (
          <span className="conn-confirm" role="alert">
            Remove {s.name}? Its tools and sign-in are deleted.
            <button className="btn sm danger" onClick={() => void run("remove", () => api.remoteRemove(s.name))}>Remove</button>
            <button className="btn sm" onClick={() => setConfirm(false)}>Keep</button>
          </span>
        ) : (
          <button className="btn sm" disabled={busy !== null} onClick={() => setConfirm(true)}>Remove</button>
        )}
      </div>
      {s.tools.length > 0 && (
        <ul className="conn-tools" aria-label={`Tools of ${s.name}`}>
          {s.tools.map((t) => (
            <li key={t.name}>
              <span className="conn-tool">
                <span className="conn-tool-name">{t.name}</span>
                {t.description && <span className="conn-tool-desc">{t.description}</span>}
                {t.skipped && <span className="conn-tool-skip">Not used: {t.skipped}</span>}
              </span>
              <select
                aria-label={`Mode for ${t.name}`}
                value={t.mode}
                disabled={busy !== null}
                onChange={(e) => {
                  const m = e.target.value as ToolMode;
                  void run("mode", () => api.remoteSetTool(s.name, t.name, m === t.default_mode ? null : m));
                }}
              >
                {MODES.map((m) => (
                  <option key={m} value={m}>{modeLabel(m, t.default_mode)}</option>
                ))}
              </select>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

export function ConnectorsSettingsTab() {
  const [view, setView] = useState<ConnectorsView | null>(null);
  const [url, setUrl] = useState("");
  const [name, setName] = useState("");
  const [advanced, setAdvanced] = useState(false);
  const [token, setToken] = useState("");
  const [clientId, setClientId] = useState("");
  const [clientSecret, setClientSecret] = useState("");
  const [adding, setAdding] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = () => api.connectorsList().then(setView).catch(() => {});
  useEffect(() => {
    void load();
    const un = onConnectors(() => void load());
    return () => void un.then((f) => f());
  }, []);

  const add = async () => {
    setError(null);
    setNote(SIGNING_IN);
    setAdding(true);
    try {
      const out = await api.remoteAdd({ url: url.trim(), name: name.trim() || null, token: token.trim() || null, client_id: clientId.trim() || null, client_secret: clientSecret.trim() || null });
      if (out.kind === "added") {
        setNote(ADDED(out.name));
        setUrl("");
        setName("");
        setToken("");
        setClientId("");
        setClientSecret("");
        setAdvanced(false);
      } else if (out.kind === "needs_client_id") {
        setAdvanced(true);
        setNote(NEEDS_CLIENT);
      } else {
        setAdvanced(true);
        setNote(out.message);
      }
      void load();
    } catch (e) {
      setNote(null);
      setError(String(e));
    } finally {
      setAdding(false);
    }
  };

  return (
    <div className="mail-set">
      <Catalog onError={setError} />
      {view && view.servers.length > 0 && (
        <section aria-labelledby="srv-h">
          <h3 id="srv-h" className="mail-h">Your servers</h3>
          <div className="conn-list">
            {view.servers.map((s) => (
              <Server key={s.name} s={s} onError={setError} />
            ))}
          </div>
        </section>
      )}

      <section aria-labelledby="add-h">
        <h3 id="add-h" className="mail-h">Add a server by URL</h3>
        <p className="set-lead">For an MCP server that is not listed above. Paste its URL and bleep finds out how it signs in.</p>
        <form
          className="fields"
          onSubmit={(e) => {
            e.preventDefault();
            if (!adding && url.trim()) void add();
          }}
        >
          <label htmlFor="mcp-url">Server URL</label>
          <input id="mcp-url" type="url" placeholder="https://example.com/mcp" value={url} onChange={(e) => setUrl(e.target.value)} disabled={adding} />
          <label htmlFor="mcp-name">Name (optional)</label>
          <input id="mcp-name" placeholder="Taken from the URL" value={name} onChange={(e) => setName(e.target.value)} disabled={adding} />
          <button type="button" className="linkbtn conn-adv" aria-expanded={advanced} onClick={() => setAdvanced((v) => !v)}>
            {advanced ? "Hide sign-in options" : "Sign-in options"}
          </button>
          {advanced && (
            <>
              <label htmlFor="mcp-token">Token</label>
              <input id="mcp-token" type="password" autoComplete="off" placeholder="Only if the server gave you one" value={token} onChange={(e) => setToken(e.target.value)} disabled={adding} />
              <label htmlFor="mcp-client">Client ID</label>
              <input id="mcp-client" autoComplete="off" value={clientId} onChange={(e) => setClientId(e.target.value)} disabled={adding} />
              <label htmlFor="mcp-secret">Client secret (optional)</label>
              <input id="mcp-secret" type="password" autoComplete="off" value={clientSecret} onChange={(e) => setClientSecret(e.target.value)} disabled={adding} />
            </>
          )}
          <div className="acts">
            <button type="submit" className="btn pri" disabled={adding || !url.trim()}>{adding ? "Adding..." : "Add"}</button>
            {adding && <button type="button" className="btn" onClick={() => void api.remoteCancelSignIn()}>Cancel</button>}
          </div>
        </form>
        {note && <p className="set-lead" role="status">{note}</p>}
      </section>

      {error && <p className="set-err" role="alert">{error}</p>}
    </div>
  );
}
