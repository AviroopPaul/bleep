// Copy for the Memory tab and the context meter. No em dashes in any of it.

/** 42_300 -> "42K", 1_500_000 -> "1.5M". Never "0K". */
export function tokens(n: number): string {
  if (n >= 1_000_000) return `${Number((n / 1_000_000).toFixed(1))}M`;
  return `${Math.max(1, Math.round(n / 1000))}K`;
}

export function meterLabel(used: number | null, threshold: number): string {
  return used == null ? "" : `${tokens(used)} of ${tokens(threshold)}`;
}

/** "high" from 80% of the threshold, when compaction is near. */
export function meterLevel(used: number, threshold: number): "ok" | "high" {
  return used >= threshold * 0.8 ? "high" : "ok";
}

export function thresholdNote(compactAt: number, threshold: number, window: number | null): string {
  if (window == null) return `bleep compacts at ${tokens(threshold)}. The window shows here after the first reply.`;
  if (threshold < compactAt) return `Your plan gives ${tokens(window)}, so bleep compacts at ${tokens(threshold)}.`;
  return `Your plan gives ${tokens(window)}. bleep compacts at ${tokens(threshold)}.`;
}

export function sourceLabel(source: string): string {
  return source === "compaction" ? "From a compacted chat" : "From your day";
}

const FILES: Record<string, string> = { "USER.md": "About you", "RULES.md": "Your rules", "MEMORY.md": "Learned" };
export const fileLabel = (file: string) => FILES[file] ?? file;

export function sizeLabel(size: number, budget: number): string {
  return `${Math.min(100, Math.round((size / budget) * 100))}% of the memory budget used`;
}
