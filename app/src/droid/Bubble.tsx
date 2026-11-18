import { useEffect, useState } from "react";
import { api, onBubbleText, type BubbleText } from "../ipc";

/** A short line over the droid. It stays until clicked or closed, and is saved to the chat.
 * Clicking it opens the chat. Mail pings can be marked "Not useful". Reminders have Done
 * and Snooze. */
export function Bubble() {
  const [b, setB] = useState<BubbleText | null>(null);
  useEffect(() => {
    const un = onBubbleText(setB);
    return () => void un.then((f) => f());
  }, []);
  if (!b?.text) return null;
  const pingId = b.ping_id;
  const remId = b.reminder_id;
  return (
    <div className="bubble-wrap">
      <div className="bubble-box">
        <button className="bubble" onClick={() => void api.bubbleClick()}>{b.text}</button>
        <button className="bubble-x" aria-label="Close" title="Close. It stays in the chat." onClick={() => void api.bubbleDismiss()}>
          <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" aria-hidden="true"><path d="M5 5l6 6M11 5l-6 6" /></svg>
        </button>
        {pingId != null && (
          <button className="bubble-nope" onClick={() => void api.bubbleNotUseful(pingId)}>Not useful</button>
        )}
        {remId != null && (
          <div className="bubble-acts">
            <button className="bubble-nope pri" onClick={() => void api.reminderDone(remId)}>Done</button>
            <button className="bubble-nope" onClick={() => void api.reminderSnooze(remId, 10)}>10 min</button>
            <button className="bubble-nope" onClick={() => void api.reminderSnooze(remId, 60)}>1 hour</button>
          </div>
        )}
      </div>
    </div>
  );
}
