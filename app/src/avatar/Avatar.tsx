import { useEffect, useMemo, useState } from "react";
import { api, onAvatarChanged } from "../ipc";
import { avatarSvg, buddy, normalize, type AvatarConfig, type Mood } from "./avatars";

/** The saved buddy. Follows changes made in Settings from any window. */
export function useAvatar(): AvatarConfig | null {
  const [cfg, setCfg] = useState<AvatarConfig | null>(null);
  useEffect(() => {
    let live = true;
    api.avatarGet().then((c) => live && setCfg(normalize(c))).catch(() => live && setCfg(normalize(null)));
    const un = onAvatarChanged((c) => setCfg(normalize(c)));
    return () => {
      live = false;
      void un.then((f) => f());
    };
  }, []);
  return cfg;
}

/** Draws a buddy. The droid keeps its own classes (`droid <mood>`); the others use
 * `av` with `data-mood`. `still` is the small, calm version for headers. */
export function Avatar({ cfg, mood = "idle", still = false, className = "" }: { cfg: AvatarConfig; mood?: Mood; still?: boolean; className?: string }) {
  const html = useMemo(() => avatarSvg(cfg, { still }), [cfg, still]);
  const droid = buddy(cfg.kind).fixed;
  const cls = droid ? (still ? "" : `droid ${mood === "idle" ? "" : mood}`) : `av${still ? " still" : ""}`;
  return <span className={`${cls} ${className}`.trim()} data-mood={still ? "idle" : mood} aria-hidden="true" dangerouslySetInnerHTML={{ __html: html }} />;
}
