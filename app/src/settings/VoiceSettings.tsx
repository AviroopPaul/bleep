import { useCallback, useEffect, useState } from "react";
import { api, type VoiceConfig, type VoiceStatus, type VoiceView } from "../ipc";
import { PILL, SENSITIVITY, deviceOptions, levelPct, phaseText, speakNote } from "./voiceText";

export function VoiceSettingsTab() {
  const [v, setV] = useState<VoiceView | null>(null);
  const [status, setStatus] = useState<VoiceStatus | null>(null);
  const [phrase, setPhrase] = useState("");
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(() => api.voiceSettings().then((view) => {
    setV(view);
    setStatus(view.status);
    setPhrase(view.config.phrase);
  }).catch(() => {}), []);

  useEffect(() => {
    void load();
  }, [load]);

  // The status changes on its own (downloads, the level, what it heard), so poll while open.
  useEffect(() => {
    const t = window.setInterval(() => {
      api.voiceStatus().then(setStatus).catch(() => {});
    }, 400);
    return () => window.clearInterval(t);
  }, []);

  const save = async (next: VoiceConfig) => {
    setError(null);
    try {
      const view = await api.voiceSet(next);
      setV(view);
      setPhrase(view.config.phrase);
    } catch (e) {
      setError(String(e));
    }
  };

  if (!v || !status) return null;
  const c = v.config;
  const live = status.phase === "listening" || status.phase === "awake";
  const speakerNames = v.speakers.map((s) => s.name);

  return (
    <>
      <section className="set-card" aria-labelledby="wake-h">
        <div className="set-head">
          <h2 id="wake-h">Wake word</h2>
          <span className={`pill ${PILL[status.phase]}`} role="status">{phaseText(status, c)}</span>
        </div>
        <p className="set-lead">
          Say the phrase and bleep opens the chat and listens for your request. Speech to text runs on this Mac. Audio is never saved or sent anywhere.
          {!v.model_ready && ` Turning this on downloads a ${v.model_mb} MB speech model, once.`}
        </p>
        <label className="toggle">
          <input type="checkbox" role="switch" checked={c.wake} onChange={(e) => void save({ ...c, wake: e.target.checked })} />
          <span>Listen for the wake phrase</span>
        </label>
        <form className="fields" onSubmit={(e) => { e.preventDefault(); void save({ ...c, phrase }); }}>
          <label htmlFor="wake-phrase">Wake phrase</label>
          <div className="row-input">
            <input id="wake-phrase" value={phrase} onChange={(e) => setPhrase(e.target.value)} spellCheck={false} autoComplete="off" maxLength={40} />
            <button type="submit" className="btn" disabled={phrase.trim() === c.phrase}>Save</button>
          </div>
          <label htmlFor="wake-mic">Microphone</label>
          <select id="wake-mic" value={c.mic} onChange={(e) => void save({ ...c, mic: e.target.value })}>
            {deviceOptions(v.mics, v.default_mic, c.mic).map((o) => <option key={o.value} value={o.value}>{o.label}</option>)}
          </select>
          <label htmlFor="wake-sens">Sensitivity</label>
          <select id="wake-sens" value={c.sensitivity} onChange={(e) => void save({ ...c, sensitivity: e.target.value as VoiceConfig["sensitivity"] })}>
            {SENSITIVITY.map((o) => <option key={o.value} value={o.value}>{o.label}</option>)}
          </select>
        </form>
        {live && (
          <div className="voice-live">
            <div className="level" role="meter" aria-label="Mic level" aria-valuemin={0} aria-valuemax={100} aria-valuenow={levelPct(status.level)}>
              <i style={{ width: `${levelPct(status.level)}%` }} />
            </div>
            <p className="set-lead">
              {status.mic && <>Using {status.mic}. </>}
              {status.last_heard ? <>Last heard: "{status.last_heard}"</> : "Nothing heard yet."}
            </p>
          </div>
        )}
        {status.phase === "downloading" && (
          <div className="level" role="progressbar" aria-label="Download" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round((status.progress ?? 0) * 100)}>
            <i style={{ width: `${Math.round((status.progress ?? 0) * 100)}%` }} />
          </div>
        )}
        {status.detail && <p className={status.phase === "error" ? "set-err" : "set-lead"} role={status.phase === "error" ? "alert" : undefined}>{status.detail}</p>}
        {error && <p className="set-err" role="alert">{error}</p>}

      </section>

      <section className="set-card" aria-labelledby="speak-h">
        <div className="set-head">
          <h2 id="speak-h">Spoken replies</h2>
        </div>
        <p className="set-lead">Uses the voices built into macOS. Free, and nothing leaves your Mac.</p>
        <label className="toggle">
          <input type="checkbox" role="switch" checked={c.speak} onChange={(e) => void save({ ...c, speak: e.target.checked })} />
          <span>Read replies aloud</span>
        </label>
        <p className="set-lead">{speakNote(c)}</p>
        <div className="fields">
          <label htmlFor="tts-speaker">Speaker</label>
          <select id="tts-speaker" value={c.speaker} onChange={(e) => void save({ ...c, speaker: e.target.value })}>
            {deviceOptions(speakerNames, null, c.speaker).map((o) => <option key={o.value} value={o.value}>{o.label}</option>)}
          </select>
          <label htmlFor="tts-voice">Voice</label>
          <select id="tts-voice" value={c.voice} onChange={(e) => void save({ ...c, voice: e.target.value })}>
            <option value="">System voice</option>
            {v.voices.map((x) => <option key={x.name} value={x.name}>{`${x.name.replace(/ \(.*\)$/, "")}, ${x.locale.replace("_", " ")}`}</option>)}
          </select>
          <div className="acts">
            <button type="button" className="btn" onClick={() => void api.voiceTestSpeaker(c.voice, c.speaker).catch((e) => setError(String(e)))}>Play a sample</button>
          </div>
        </div>
      </section>
    </>
  );
}
