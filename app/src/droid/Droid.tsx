import { useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Avatar, useAvatar } from "../avatar/Avatar";
import { api, onChatEvent, onDroidMood, onVoice } from "../ipc";
import { NAME, TAG } from "../flavor";

import type { Mood } from "../avatar/avatars";
const DRAG_PX = 4;
const DOUBLE_MS = 260;

export function Droid() {
  const [mood, setMood] = useState<Mood>("idle");
  const avatar = useAvatar();
  const timer = useRef<number | undefined>(undefined);
  const press = useRef<{ x: number; y: number; dragged: boolean } | null>(null);
  const clickTimer = useRef<number | undefined>(undefined);

  useEffect(() => {
    const brief = (m: Mood, ms: number) => {
      setMood(m);
      window.clearTimeout(timer.current);
      timer.current = window.setTimeout(() => setMood("idle"), ms);
    };
    const un = onChatEvent(({ event }) => {
      if (event.type === "text_delta" || event.type === "tool_start") {
        window.clearTimeout(timer.current);
        setMood("thinking");
      } else if (event.type === "error" || event.type === "rate_limited") brief("alert", 2000);
      else if (event.type === "done") setMood((m) => (m === "alert" ? m : "happy")), (timer.current = window.setTimeout(() => setMood("idle"), 2000));
    });
    // A meeting bar on screen keeps the antenna light on until it goes away.
    const unMood = onDroidMood((m) => {
      window.clearTimeout(timer.current);
      setMood(m === "alert" ? "alert" : "idle");
    });
    // The wake phrase lights the antenna green until the request is heard.
    const unVoice = onVoice((e) => {
      window.clearTimeout(timer.current);
      if (e.kind === "wake") setMood("listening");
      else setMood((m) => (m === "listening" ? (e.kind === "heard" ? "thinking" : "idle") : m));
    });
    return () => {
      void un.then((f) => f());
      void unMood.then((f) => f());
      void unVoice.then((f) => f());
    };
  }, []);

  const onPointerDown = (e: React.PointerEvent) => {
    if (e.button !== 0) return;
    press.current = { x: e.screenX, y: e.screenY, dragged: false };
  };
  const onPointerMove = (e: React.PointerEvent) => {
    const p = press.current;
    if (!p || p.dragged) return;
    if (Math.abs(e.screenX - p.x) + Math.abs(e.screenY - p.y) > DRAG_PX) {
      p.dragged = true;
      void getCurrentWindow().startDragging();
    }
  };
  const onPointerUp = () => {
    const p = press.current;
    press.current = null;
    if (!p || p.dragged) return;
    if (clickTimer.current) {
      window.clearTimeout(clickTimer.current);
      clickTimer.current = undefined;
      void api.toggleTuck();
    } else {
      clickTimer.current = window.setTimeout(() => {
        clickTimer.current = undefined;
        void api.openChat("droid");
      }, DOUBLE_MS);
    }
  };

  return (
    <button
      className="droid-stage"
      aria-label={`${NAME}. Click to open the chat, double click to tuck away, drag to move.`}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
    >
      {avatar && <Avatar cfg={avatar} mood={mood} />}
      {TAG && <span className="droid-dev" aria-hidden="true">{TAG}</span>}
    </button>
  );
}
