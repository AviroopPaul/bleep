import { useEffect, useState } from "react";
import { api, type MailConfig } from "../ipc";

function AddressList({ title, items, onRemove }: { title: string; items: string[]; onRemove: (a: string) => void }) {
  return (
    <>
    {!items.length && <p className="set-lead">None yet.</p>}
    <ul className="chips" aria-label={title}>
      {items.map((a) => (
        <li key={a} className="chip">
          <span>{a}</span>
          <button className="chip-x" aria-label={`Remove ${a}`} onClick={() => onRemove(a)}>×</button>
        </li>
      ))}
    </ul>
    </>
  );
}

/** VIPs ping and count as important; muted senders are ignored. Nothing is muted without a tap. */
export function MailSettings() {
  const [m, setM] = useState<MailConfig | null>(null);
  const [vip, setVip] = useState("");
  const [mute, setMute] = useState("");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.mailSettings().then(setM).catch(() => {});
  }, []);

  const save = async (vips: string[], muted: string[]) => {
    setError(null);
    try {
      setM(await api.mailSetLists(vips, muted));
    } catch (e) {
      setError(String(e));
    }
  };
  const resolve = async (addr: string, yes: boolean) => {
    try {
      setM(await api.mailResolveSuggestion(addr, yes));
    } catch (e) {
      setError(String(e));
    }
  };
  if (!m) return null;
  const add = (list: string[], v: string) => [...list, v.trim().toLowerCase()];

  return (
    <div className="mail-set">
      {m.suggested_mutes.length > 0 && (
        <section aria-labelledby="sug-h">
          <h3 id="sug-h" className="mail-h">Mute these senders?</h3>
          <p className="set-lead">You marked their pings as not useful three times.</p>
          {m.suggested_mutes.map((a) => (
            <div key={a} className="sug">
              <span>{a}</span>
              <button className="btn" aria-label={`Mute ${a}`} onClick={() => void resolve(a, true)}>Mute</button>
              <button className="btn" aria-label={`Keep ${a}`} onClick={() => void resolve(a, false)}>Keep</button>
            </div>
          ))}
        </section>
      )}
      <section aria-labelledby="vip-h">
        <h3 id="vip-h" className="mail-h">VIPs</h3>
        <p className="set-lead">Mail from them counts as important, and replies they wait for can ping you.</p>
        <AddressList title="VIPs" items={m.vips} onRemove={(a) => void save(m.vips.filter((x) => x !== a), m.muted)} />
        <form className="add" onSubmit={(e) => { e.preventDefault(); if (vip.trim()) void save(add(m.vips, vip), m.muted).then(() => setVip("")); }}>
          <label htmlFor="vip" className="sr-only">Add a VIP</label>
          <input id="vip" type="email" placeholder="name@example.com" value={vip} onChange={(e) => setVip(e.target.value)} />
          <button type="submit" className="btn" disabled={!vip.trim()}>Add VIP</button>
        </form>
      </section>
      <section aria-labelledby="mute-h">
        <h3 id="mute-h" className="mail-h">Muted senders</h3>
        <p className="set-lead">bleep ignores their mail.</p>
        <AddressList title="Muted senders" items={m.muted} onRemove={(a) => void save(m.vips, m.muted.filter((x) => x !== a))} />
        <form className="add" onSubmit={(e) => { e.preventDefault(); if (mute.trim()) void save(m.vips, add(m.muted, mute)).then(() => setMute("")); }}>
          <label htmlFor="mute" className="sr-only">Mute a sender</label>
          <input id="mute" type="email" placeholder="name@example.com" value={mute} onChange={(e) => setMute(e.target.value)} />
          <button type="submit" className="btn" disabled={!mute.trim()}>Mute</button>
        </form>
      </section>
      {error && <p className="set-err" role="alert">{error}</p>}
    </div>
  );
}
