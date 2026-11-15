import { useEffect, useMemo, useState } from "react";
import { api, onRuntime, type ApiConfig, type ModelInfo, type ModelSettings, type SpendView, type TestResult, type Tiers } from "../ipc";
import { CLAUDE_MODELS, modelLine, spendText } from "./modelText";

type Engine = "claude" | "api";

/** Which engine runs bleep: Claude Code, or a provider API with my own key. */
export function ModelSettingsTab() {
  const [s, setS] = useState<ModelSettings | null>(null);
  const [engine, setEngine] = useState<Engine>("claude");
  const [tiers, setTiers] = useState<Tiers>({ main: "sonnet", small: "haiku" });
  const [cfg, setCfg] = useState<ApiConfig | null>(null);
  const [key, setKey] = useState("");
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [listNote, setListNote] = useState<string | null>(null);
  const [test, setTest] = useState<TestResult | null>(null);
  const [busy, setBusy] = useState<"test" | "save" | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [spend, setSpend] = useState<SpendView | null>(null);

  const load = () =>
    api.modelSettings().then((v) => {
      setS(v);
      setEngine(v.runtime);
      setTiers(v.tiers);
      setCfg(v.api);
    }).catch(() => {});
  useEffect(() => {
    void load();
    api.spendToday().then(setSpend).catch(() => {});
    const un = onRuntime(() => void api.spendToday().then(setSpend).catch(() => {}));
    return () => void un.then((f) => f());
  }, []);

  const preset = useMemo(() => s?.presets.find((p) => p.id === cfg?.provider), [s, cfg?.provider]);
  const saved = !!(preset && s?.keys.includes(preset.id));
  const canList = !!preset && !!cfg && (!preset.needs_key || saved || key.trim() !== "") && (cfg.base_url || preset.base_url) !== "";

  // The model list follows the provider, the base URL and a newly typed key.
  useEffect(() => {
    if (engine !== "api" || !canList || !cfg) {
      setModels([]);
      return;
    }
    let live = true;
    setListNote("Loading models...");
    const t = setTimeout(() => {
      api.modelList(cfg.provider, cfg.base_url, key.trim() || null)
        .then((m) => live && (setModels(m), setListNote(m.length ? null : "The provider listed no models. Type a model id.")))
        .catch((e) => live && (setModels([]), setListNote(`Could not load models: ${String(e)} You can still type a model id.`)));
    }, 400);
    return () => {
      live = false;
      clearTimeout(t);
    };
  }, [engine, canList, cfg?.provider, cfg?.base_url, key]);

  if (!s || !cfg) return null;
  const set = (patch: Partial<ApiConfig>) => {
    setCfg({ ...cfg, ...patch });
    setTest(null);
    setNote(null);
  };
  const pickProvider = (id: string) => {
    const p = s.presets.find((x) => x.id === id);
    if (!p) return;
    setKey("");
    set({ provider: id, base_url: p.base_url, main: p.main, small: p.small });
  };
  const info = (id: string) => models.find((m) => m.id === id);

  const save = async () => {
    setError(null);
    setNote(null);
    setBusy("save");
    try {
      await api.modelSave({ runtime: engine, tiers, api: cfg, key: engine === "api" && key.trim() ? key.trim() : null });
      setKey("");
      await load();
      setNote(engine === "api" ? `Saved. bleep now runs on ${preset?.label ?? "the API"}.` : "Saved. bleep now runs on Claude Code.");
      api.spendToday().then(setSpend).catch(() => {});
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
    }
  };
  const runTest = async () => {
    setError(null);
    setTest(null);
    setBusy("test");
    try {
      setTest(await api.modelTest(cfg.provider, cfg.base_url, key.trim() || null, cfg.main));
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
    }
  };
  const forget = async () => {
    if (!preset) return;
    await api.modelForgetKey(preset.id).catch((e) => setError(String(e)));
    await load();
  };

  return (
    <div className="mail-set model-set">
      <section aria-labelledby="eng-h">
        <h3 id="eng-h" className="mail-h">What runs bleep</h3>
        <div className="choice" role="radiogroup" aria-labelledby="eng-h">
          <label className={`choice-opt ${engine === "claude" ? "on" : ""}`}>
            <input type="radio" name="engine" checked={engine === "claude"} onChange={() => setEngine("claude")} />
            <span>
              <b>Claude Code</b>
              <small>Uses your Claude subscription through the Claude Code app.</small>
            </span>
          </label>
          <label className={`choice-opt ${engine === "api" ? "on" : ""}`}>
            <input type="radio" name="engine" checked={engine === "api"} onChange={() => setEngine("api")} />
            <span>
              <b>API key</b>
              <small>Pick a provider and models. Calls are billed to your key.</small>
            </span>
          </label>
        </div>
      </section>

      {engine === "claude" && (
        <section aria-labelledby="cc-h">
          <h3 id="cc-h" className="mail-h">Claude Code models</h3>
          <div className="fields">
            <label htmlFor="cc-main">Main model (chat and check-ins)</label>
            <select id="cc-main" value={tiers.main} onChange={(e) => setTiers({ ...tiers, main: e.target.value })}>
              {CLAUDE_MODELS.map((m) => <option key={m.id} value={m.id}>{m.label}</option>)}
            </select>
            <label htmlFor="cc-small">Small model (mail triage and quick checks)</label>
            <select id="cc-small" value={tiers.small} onChange={(e) => setTiers({ ...tiers, small: e.target.value })}>
              {CLAUDE_MODELS.map((m) => <option key={m.id} value={m.id}>{m.label}</option>)}
            </select>
          </div>
        </section>
      )}

      {engine === "api" && (
        <section aria-labelledby="api-h">
          <h3 id="api-h" className="mail-h">Provider and models</h3>
          {spend && spend.runtime === "api" && <p className={`set-lead ${spend.over ? "set-warn" : ""}`} role="status" aria-label="Spend">{spendText(spend)}</p>}
          <div className="fields">
            <label htmlFor="api-provider">Provider</label>
            <select id="api-provider" value={cfg.provider} onChange={(e) => pickProvider(e.target.value)}>
              {s.presets.map((p) => <option key={p.id} value={p.id}>{p.label}</option>)}
            </select>
            {preset && (preset.needs_key || preset.id === "custom") && (
              <>
                <label htmlFor="api-key">{preset.needs_key ? "API key" : "API key (only if the server needs one)"}</label>
                <input id="api-key" type="password" autoComplete="off" placeholder={saved ? "Saved in your Keychain" : "Paste your key"} value={key} onChange={(e) => (setKey(e.target.value), setTest(null))} />
                <p className="hint">
                  {saved && <button type="button" className="linkbtn" onClick={() => void forget()}>Forget the saved key</button>}
                  {saved && preset.key_url && " "}
                  {preset.key_url && <button type="button" className="linkbtn" onClick={() => void api.openUrl(preset.key_url)}>Get a {preset.label} key</button>}
                </p>
              </>
            )}
            {preset && !preset.needs_key && (
              <>
                <label htmlFor="api-base">Base URL</label>
                <input id="api-base" type="url" placeholder="https://example.com/v1" value={cfg.base_url} onChange={(e) => set({ base_url: e.target.value })} />
              </>
            )}
            <label htmlFor="api-main">Main model (chat and check-ins)</label>
            <input id="api-main" list="api-models" autoComplete="off" placeholder="Pick or type a model id" value={cfg.main} onChange={(e) => set({ main: e.target.value })} />
            {info(cfg.main) && <p className="hint">{modelLine(info(cfg.main), !!preset?.free)}</p>}
            <label htmlFor="api-small">Small model (mail triage and quick checks)</label>
            <input id="api-small" list="api-models" autoComplete="off" placeholder="A cheaper, faster model" value={cfg.small} onChange={(e) => set({ small: e.target.value })} />
            {info(cfg.small) && <p className="hint">{modelLine(info(cfg.small), !!preset?.free)}</p>}
            <datalist id="api-models">
              {models.map((m) => <option key={m.id} value={m.id}>{m.name ?? m.id}</option>)}
            </datalist>
            {listNote && <p className="hint" role="status">{listNote}</p>}
            <label htmlFor="api-cap">Daily spend cap (US dollars)</label>
            <input id="api-cap" type="number" min={0} step={0.5} value={cfg.daily_cap_usd} onChange={(e) => set({ daily_cap_usd: Number(e.target.value) })} />
            <p className="hint">When today's spend reaches it, check-ins, mail triage and screenshot checks stop until tomorrow. Chat still works. Use 0 for no cap.</p>
          </div>
        </section>
      )}

      <div className="acts">
        {engine === "api" && (
          <button className="btn" disabled={busy !== null || !cfg.main.trim() || !canList} onClick={() => void runTest()}>
            {busy === "test" ? "Testing..." : "Test"}
          </button>
        )}
        <button className="btn pri" disabled={busy !== null} onClick={() => void save()}>{busy === "save" ? "Saving..." : "Save"}</button>
      </div>
      {test && <p className={test.ok ? "set-lead" : "set-err"} role="status" aria-label="Test result">{test.ok ? `OK. ${test.message}` : test.message}</p>}
      {note && <p className="set-lead" role="status">{note}</p>}
      {error && <p className="set-err" role="alert">{error}</p>}
    </div>
  );
}
