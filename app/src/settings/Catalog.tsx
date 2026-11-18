import { useEffect, useState } from "react";
import { api, onConnectors, type CatalogItem, type CatalogView } from "../ipc";
import { catalogStatus, GOOGLE_NOTE } from "./connectorsText";
import { LOGOS } from "./logos";

function Logo({ id, name }: { id: string; name: string }) {
  const l = LOGOS[id];
  return (
    <span className="cat-ico" aria-hidden="true">
      {l ? (
        <svg viewBox="0 0 24 24"><path d={l.d} fill={l.hex} /></svg>
      ) : (
        <b>{name.charAt(0)}</b>
      )}
    </span>
  );
}

export function Card({ c, onView, onError }: { c: CatalogItem; onView: (v: CatalogView) => void; onError: (e: string | null) => void }) {
  const [busy, setBusy] = useState(false);
  const [token, setToken] = useState("");
  const needsToken = c.token && !c.on && c.status.state !== "connected";
  const set = async (on: boolean) => {
    onError(null);
    setBusy(true);
    try {
      onView(await api.catalogSet(c.id, on, needsToken ? token.trim() || null : null));
      setToken("");
    } catch (e) {
      onError(String(e));
    } finally {
      setBusy(false);
    }
  };
  const status = catalogStatus(c, busy);
  return (
    <li className={`cat-card ${c.on ? "on" : ""}`} role="group" aria-label={c.name}>
      <Logo id={c.id} name={c.name} />
      <div className="cat-main">
        <div className="cat-top">
          <h4 className="cat-name">{c.name}</h4>
          {status && <span className={`pill ${busy ? "" : c.status.state}`} role="status">{status}</span>}
        </div>
        <p className="cat-blurb">{c.blurb}</p>
        {needsToken && (
          <div className="cat-token">
            <label htmlFor={`cat-tok-${c.id}`} className="sr-only">{c.name} token</label>
            <input id={`cat-tok-${c.id}`} type="password" autoComplete="off" placeholder="Paste a token" value={token} onChange={(e) => setToken(e.target.value)} disabled={busy} />
          </div>
        )}
        {c.on && c.status.state === "needs_sign_in" && (
          <button className="btn sm pri cat-sign" disabled={busy} onClick={() => void set(true)}>Sign in</button>
        )}
      </div>
      <label className="cat-switch">
        <input
          type="checkbox"
          role="switch"
          aria-label={`${c.name} on`}
          checked={c.on}
          disabled={busy || (needsToken && !token.trim())}
          onChange={(e) => void set(e.target.checked)}
        />
        <span aria-hidden="true" />
      </label>
    </li>
  );
}

/** A connector the model offered in the chat (`connector_offer`): its card, with the switch. */
export function OfferCard({ id }: { id: string }) {
  const [item, setItem] = useState<CatalogItem | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    const load = () => api.catalogList().then((v) => setItem(v.items.find((c) => c.id === id) ?? null)).catch(() => {});
    void load();
    const un = onConnectors(() => void load());
    return () => void un.then((f) => f());
  }, [id]);
  if (!item) return null;
  return (
    <div className="offer">
      <ul className="cat-list">
        <Card c={item} onView={(v) => setItem(v.items.find((c) => c.id === id) ?? null)} onError={setError} />
      </ul>
      {error && <p className="offer-err" role="alert">{error}</p>}
      {item.on && item.status.state === "connected" && <p className="offer-ok">On. Ask again and bleep can use it.</p>}
    </div>
  );
}

/** Settings > Connectors: every connector we set up, each with a switch and its sign-in. */
export function Catalog({ onError }: { onError: (e: string | null) => void }) {
  const [view, setView] = useState<CatalogView | null>(null);
  useEffect(() => {
    const load = () => api.catalogList().then(setView).catch(() => {});
    void load();
    const un = onConnectors(() => void load());
    return () => void un.then((f) => f());
  }, []);
  if (!view) return null;
  const google = view.items.filter((c) => c.kind === "google");
  const apps = view.items.filter((c) => c.kind === "remote");
  const local = view.items.filter((c) => c.kind === "local");
  return (
    <>
      <section aria-labelledby="cat-g-h">
        <h3 id="cat-g-h" className="mail-h">Google</h3>
        <p className="set-lead">{GOOGLE_NOTE}</p>
        <ul className="cat-list">{google.map((c) => <Card key={c.id} c={c} onView={setView} onError={onError} />)}</ul>
      </section>
      {local.length > 0 && (
        <section aria-labelledby="cat-l-h">
          <h3 id="cat-l-h" className="mail-h">On this Mac</h3>
          <p className="set-lead">Nothing to sign in to. bleep looks at files only when you ask. A command always shows you a card first.</p>
          <ul className="cat-list">{local.map((c) => <Card key={c.id} c={c} onView={setView} onError={onError} />)}</ul>
        </section>
      )}
      <section aria-labelledby="cat-a-h">
        <h3 id="cat-a-h" className="mail-h">Apps</h3>
        <p className="set-lead">Turn one on and sign in. Tools that change things always show you a card first.</p>
        <ul className="cat-list">{apps.map((c) => <Card key={c.id} c={c} onView={setView} onError={onError} />)}</ul>
      </section>
      <section aria-labelledby="cat-on-h">
        <h3 id="cat-on-h" className="mail-h">Always on</h3>
        <ul className="cat-always">{view.always_on.map((n) => <li key={n}>{n}</li>)}</ul>
      </section>
    </>
  );
}
