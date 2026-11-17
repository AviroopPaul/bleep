import type { VoiceConfig, VoiceStatus } from "../ipc";

/** The status pill. */
export function phaseText(s: VoiceStatus, cfg: VoiceConfig): string {
  switch (s.phase) {
    case "off": return cfg.wake ? "Starting..." : "Off";
    case "downloading": return `Downloading ${Math.round((s.progress ?? 0) * 100)}%`;
    case "loading": return "Loading...";
    case "listening": return s.detail ? "No sound" : "Listening";
    case "awake": return "Awake";
    case "error": return "Error";
  }
}

export const PILL: Record<VoiceStatus["phase"], string> = {
  off: "", downloading: "needs_sign_in", loading: "needs_sign_in", listening: "connected", awake: "connected", error: "error",
};

export interface Opt {
  value: string;
  label: string;
}

/**
 * Device picker options. "" is Auto: whatever macOS uses now, so AirPods take over
 * when they connect. A saved device that is unplugged stays listed, marked.
 */
export function deviceOptions(names: string[], current: string | null, chosen: string): Opt[] {
  const opts: Opt[] = [{ value: "", label: current ? `Auto (${current})` : "Auto (system default)" }];
  for (const n of names) if (!opts.some((o) => o.value === n)) opts.push({ value: n, label: n });
  if (chosen && !names.includes(chosen)) opts.push({ value: chosen, label: `${chosen} (not connected)` });
  return opts;
}

/** Input level as a 0 to 100 bar. Speech RMS sits near 0.02 to 0.2, so a square root reads better. */
export function levelPct(level: number): number {
  return Math.max(0, Math.min(100, Math.round(Math.sqrt(Math.max(0, level) / 0.2) * 100)));
}

/** Spoken replies read only replies to spoken requests. */
export function speakNote(cfg: VoiceConfig): string {
  return cfg.speak ? "bleep reads its reply aloud when you asked by voice. Typed questions stay quiet." : "Replies show in the chat only.";
}

export const SENSITIVITY: Opt[] = [
  { value: "strict", label: "Strict: wakes less by mistake" },
  { value: "normal", label: "Normal" },
  { value: "loose", label: "Loose: for when bleep misses you" },
];
