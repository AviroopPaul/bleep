import { useEffect, useState } from "react";
import { api, onBubbleText, type BubbleText } from "../ipc";

/** A short line over the droid. It stays until clicked or closed, and is saved to the chat.
 * Clicking it opens the chat. Mail pings can be marked "Not useful". */
export function Bubble() {
  const [b, setB] = useState<BubbleText | null>(null);
  useEffect(() => {
    const un = onBubbleText(setB);
    return () => void un.then((f) => f());
  }, []);
  if (!b?.text) return null;
  const pingId = b.ping_id;
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
      </div>
    </div>
  );
}
