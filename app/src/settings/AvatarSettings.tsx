import { useState } from "react";
import { api } from "../ipc";
import { Avatar, useAvatar } from "../avatar/Avatar";
import { BUDDIES, FINISHES, PALETTE, buddy, finishOf, switchKind, type AvatarConfig, type Mood } from "../avatar/avatars";

const MOODS: { id: Mood; name: string }[] = [
  { id: "idle", name: "Idle" },
  { id: "listening", name: "Listening" },
  { id: "thinking", name: "Thinking" },
  { id: "alert", name: "Alert" },
  { id: "happy", name: "Happy" },
];

export function AvatarSettingsTab() {
  const saved = useAvatar();
  const [mood, setMood] = useState<Mood>("idle");
  const [error, setError] = useState<string | null>(null);
  if (!saved) return null;
  const c = saved;
  const b = buddy(c.kind);
  const finish = finishOf(c);

  const save = async (next: AvatarConfig) => {
    setError(null);
    try {
      await api.avatarSet(next);
    } catch (e) {
      setError(String(e));
    }
  };

  return (
    <section className="set-card" aria-labelledby="av-h">
      <div className="set-head">
        <h2 id="av-h">Your buddy</h2>
      </div>
      <p className="set-lead">Pick who floats on your screen. The menu bar icon shows the same buddy as an outline.</p>

      <div className="av-stage" data-testid="av-preview">
        {/* Remount on mood change so one-shot moves (hop, shake) play again. */}
        <Avatar key={mood} cfg={c} mood={mood} />
      </div>
      <div className="av-moods" role="group" aria-label="Try a mood">
        {MOODS.map((m) => (
          <button key={m.id} type="button" aria-pressed={mood === m.id} onClick={() => setMood(m.id)}>{m.name}</button>
        ))}
      </div>

      <span className="av-label" id="av-kind">Buddy</span>
      <div className="av-pick" role="radiogroup" aria-labelledby="av-kind">
        {BUDDIES.map((x) => {
          const cfg = x.id === c.kind ? c : switchKind(c, x.id);
          return (
            <button key={x.id} type="button" role="radio" className="av-tile" aria-checked={x.id === c.kind} onClick={() => void save(cfg)}>
              <span><Avatar cfg={cfg} still /></span>
              {x.name}
            </button>
          );
        })}
      </div>

      {b.fixed ? (
        <p className="av-note">The droid keeps its own colors.</p>
      ) : (
        <>
          <span className="av-label" id="av-color">Color</span>
          <div className="av-swatches" role="radiogroup" aria-labelledby="av-color">
            {PALETTE.map((p) => (
              <button key={p.hex} type="button" role="radio" aria-label={p.name} title={p.name} aria-checked={c.color === p.hex} style={{ background: p.hex }} onClick={() => void save({ ...c, color: p.hex })} />
            ))}
          </div>

          <span className="av-label" id="av-finish">Finish</span>
          <div className="av-finish" role="radiogroup" aria-labelledby="av-finish">
            {FINISHES.map((f) => (
              <button key={f.id} type="button" role="radio" aria-checked={finish === f.id} disabled={b.pixel} onClick={() => void save({ ...c, finish: f.id })}>{f.name}</button>
            ))}
          </div>
          {b.pixel && <p className="av-note">Boop is pixel art, so it is always flat.</p>}
        </>
      )}
      {error && <p className="set-err" role="alert">{error}</p>}
    </section>
  );
}
