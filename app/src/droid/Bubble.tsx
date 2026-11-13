import { useEffect, useState } from "react";
import { api, onBubbleText, type BubbleText } from "../ipc";

/** A short line over the droid. Clicking it opens the chat. Mail pings can be marked "Not useful". */
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
        {pingId != null && (
          <button className="bubble-nope" onClick={() => void api.bubbleNotUseful(pingId)}>Not useful</button>
        )}
      </div>
    </div>
  );
}
