import { useEffect, useState } from "react";
import { api, type MemorySettings, type MemoryView, type Proposal } from "../ipc";
import { fileLabel, sizeLabel, sourceLabel, thresholdNote, tokens } from "./memoryText";

const MIN = 200_000;
const MAX = 300_000;

export function MemorySettingsTab() {
  const [mem, setMem] = useState<MemoryView | null>(null);
  const [sugs, setSugs] = useState<Proposal[]>([]);
  const [cfg, setCfg] = useState<MemorySettings | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.memoryList().then(setMem).catch(() => {});
    api.memorySuggestions().then(setSugs).catch(() => {});
    api.memorySettings().then(setCfg).catch(() => {});
  }, []);

  const run = async <T,>(f: () => Promise<T>, then: (v: T) => void) => {
    setError(null);
    try {
      then(await f());
    } catch (e) {
      setError(String(e));
    }
  };

  const decide = (id: number, add: boolean) =>
    run(() => api.memoryDecide(id, add), (list) => {
      setSugs(list);
      if (add) void api.memoryList().then(setMem);
    });

  const compactNow = async () => {
    setStatus("Compacting...");
    try {
      await api.memoryCompactNow();
      setStatus("Compacted. The chat goes on from the summary.");
    } catch (e) {
      setStatus(String(e) === "busy" ? "bleep is busy. Try again after this reply." : `Could not compact: ${String(e)}`);
    }
  };

  return (
    <div className="mail-set">
      <section aria-labelledby="mem-h">
        <h3 id="mem-h" className="mail-h">What bleep knows</h3>
        <p className="set-lead">bleep always sees these facts. They are plain files you can also edit in any editor.</p>
        {mem?.files.map((f) => (
          <div key={f.file} className="mem-file" role="group" aria-label={fileLabel(f.file)}>
            <h4 className="mem-file-h">{fileLabel(f.file)} <span className="mem-file-name">{f.file}</span></h4>
            {f.facts.length === 0 ? (
              <p className="mem-empty">Nothing yet.</p>
            ) : (
              <ul className="mem-list">
                {f.facts.map((fact, i) => (
                  <li key={`${i}-${fact}`}>
                    <span className="mem-fact">{fact}</span>
                    <button className="btn sm" aria-label={`Forget: ${fact}`} onClick={() => void run(() => api.memoryForget(f.file, fact), setMem)}>Forget</button>
                  </li>
                ))}
              </ul>
            )}
          </div>
        ))}
        {mem && <p className="set-lead" role="status">{sizeLabel(mem.size, mem.budget)}</p>}
        <button className="btn" onClick={() => void api.memoryReveal()}>Show memory folder</button>
      </section>

      <section aria-labelledby="sug-h">
        <h3 id="sug-h" className="mail-h">Suggestions</h3>
        <p className="set-lead">Facts bleep noticed. Nothing is saved until you add it.</p>
        {sugs.length === 0 ? (
          <p className="mem-empty">No suggestions right now.</p>
        ) : (
          <ul className="mem-list">
            {sugs.map((p) => (
              <li key={p.id}>
                <span className="mem-fact">{p.fact}<span className="mem-src">{sourceLabel(p.source)}</span></span>
                <span className="acts">
                  <button className="btn sm pri" aria-label={`Add: ${p.fact}`} onClick={() => void decide(p.id, true)}>Add</button>
                  <button className="btn sm" aria-label={`Dismiss: ${p.fact}`} onClick={() => void decide(p.id, false)}>Dismiss</button>
                </span>
              </li>
            ))}
          </ul>
        )}
      </section>

      {cfg && (
        <section aria-labelledby="cmp-h">
          <h3 id="cmp-h" className="mail-h">Compaction</h3>
          <p className="set-lead">When the chat gets full, bleep saves facts, writes a summary and carries on in a fresh session. Your history stays in view.</p>
          <label className="range">
            <span>Compact at {tokens(cfg.compact_at)}</span>
            <input
              type="range"
              min={MIN}
              max={MAX}
              step={10_000}
              value={Math.min(MAX, Math.max(MIN, cfg.compact_at))}
              onChange={(e) => void run(() => api.memorySet(Number(e.target.value)), setCfg)}
            />
          </label>
          <p className="set-lead" role="status">{thresholdNote(cfg.compact_at, cfg.threshold, cfg.window)}</p>
          <button className="btn" onClick={() => void compactNow()}>Compact now</button>
          {status && <p className="set-lead" role="status">{status}</p>}
        </section>
      )}
      {error && <p className="set-err" role="alert">{error}</p>}
    </div>
  );
}
