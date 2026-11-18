import { useEffect, useState } from "react";
import { api, onBarCue, onBarHide, type BarCue } from "../ipc";
import { Avatar, useAvatar } from "../avatar/Avatar";
import { clockText, whenText } from "./barText";

/** Slim bar under the menu bar before a meeting. Built from the calendar cache, no model call. */
export function MeetingBar() {
  const [cue, setCue] = useState<BarCue | null>(null);
  const avatar = useAvatar();
  const [, setTick] = useState(0);

  useEffect(() => {
    api.barCurrent().then(setCue).catch(() => {});
    const a = onBarCue(setCue);
    const b = onBarHide(() => setCue(null));
    const t = window.setInterval(() => setTick((x) => x + 1), 15_000);
    return () => {
      void a.then((f) => f());
      void b.then((f) => f());
      window.clearInterval(t);
    };
  }, []);

  if (!cue) return null;
  const start = new Date(cue.start);
  return (
    <section className="bar" aria-label="Next meeting">
      <span className="bar-droid" aria-hidden="true">{avatar && <Avatar cfg={avatar} still />}</span>
      <div className="bar-text">
        <strong className="bar-title" title={cue.title}>{cue.title}</strong>
        <span className="bar-meta">{whenText(start, new Date())} · {clockText(start)}</span>
      </div>
      <div className="bar-acts">
        <button className="bbtn pri" onClick={() => void api.barPrep()}>Prep me</button>
        {cue.kind === "hour" && <button className="bbtn" onClick={() => void api.barSnooze()}>Snooze</button>}
        {cue.meet_link && <button className="bbtn" onClick={() => void api.barJoin()}>Join</button>}
        <button className="bbtn x" aria-label="Dismiss" title="Dismiss" onClick={() => void api.barDismiss()}>
          <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true"><path d="M4 4l8 8M12 4l-8 8" /></svg>
        </button>
      </div>
    </section>
  );
}
