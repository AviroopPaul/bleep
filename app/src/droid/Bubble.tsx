import { useEffect, useState } from "react";
import { api, onBubbleText } from "../ipc";

/** A short line over the droid. Clicking it opens the chat. */
export function Bubble() {
  const [text, setText] = useState("");
  useEffect(() => {
    const un = onBubbleText(setText);
    return () => void un.then((f) => f());
  }, []);
  if (!text) return null;
  return (
    <div className="bubble-wrap">
      <button className="bubble" onClick={() => void api.bubbleClick()}>{text}</button>
    </div>
  );
}
