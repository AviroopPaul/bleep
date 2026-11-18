// Copy and chart math for the Usage tab. No em dashes in any of it.
import type { UsageDay, UsageView } from "../ipc";

export const RANGES = [
  { days: 1, label: "Today" },
  { days: 7, label: "7 days" },
  { days: 30, label: "30 days" },
] as const;

const PURPOSES: Record<string, string> = {
  chat: "Chat",
  triage: "Mail sorting",
  compaction: "Compaction",
  screenshot: "Screenshot checks",
  patterns: "Patterns",
  test: "Key tests",
};
export const purposeLabel = (p: string) => PURPOSES[p] ?? p.charAt(0).toUpperCase() + p.slice(1);

/** Chart colors by purpose rank. CSS variables, so dark mode follows. */
export const SERIES = ["var(--blue)", "var(--ochre)", "var(--sage)", "var(--rust)", "var(--blue-2)", "var(--muted)"];

/** 1234 -> "1,234". */
export const count = (n: number) => new Intl.NumberFormat("en-US").format(n);

/** Token counts: 950 -> "950", 42_300 -> "42.3K", 1_500_000 -> "1.5M". */
export function short(n: number): string {
  if (n >= 1_000_000) return `${Number((n / 1_000_000).toFixed(1))}M`;
  if (n >= 1000) return `${Number((n / 1000).toFixed(1))}K`;
  return String(n);
}

export function money(cost: number): string {
  if (cost > 0 && cost < 0.01) return "under $0.01";
  return `$${cost.toFixed(2)}`;
}

export function costNote(v: UsageView): string {
  if (v.cost == null) return "Claude plan, no API bill";
  return v.unpriced > 0 ? `${v.unpriced} calls have no price` : "API key calls";
}

/** Heatmap shade 0 (none) to 4 (busiest hour in the range). */
export function heatLevel(n: number, max: number): number {
  if (n <= 0 || max <= 0) return 0;
  return Math.min(4, Math.max(1, Math.ceil((n / max) * 4)));
}

export const WEEKDAYS = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

export function hourLabel(h: number): string {
  const ampm = h < 12 ? "AM" : "PM";
  return `${h % 12 === 0 ? 12 : h % 12} ${ampm}`;
}

/** "Busiest: Tue, 9 AM" or "" when nothing ran. */
export function busiest(heat: number[][]): string {
  let best = { d: 0, h: 0, n: 0 };
  heat.forEach((row, d) => row.forEach((n, h) => n > best.n && (best = { d, h, n })));
  return best.n ? `Busiest: ${WEEKDAYS[best.d]}, ${hourLabel(best.h)}` : "";
}

export interface Stack {
  day: string;
  total: number;
  parts: { purpose: string; tokens: number; y: number; h: number }[];
}

/** Stacked bars, bottom up, in purpose order. `height` is the tallest bar in px. */
export function stacks(daily: UsageDay[], purposes: string[], height: number): Stack[] {
  const totals = daily.map((d) => Object.values(d.tokens).reduce((a, b) => a + b, 0));
  const max = Math.max(1, ...totals);
  return daily.map((d, i) => {
    let y = height;
    const parts = purposes
      .filter((p) => (d.tokens[p] ?? 0) > 0)
      .map((p) => {
        const h = Math.max(1, (d.tokens[p] / max) * height);
        y -= h;
        return { purpose: p, tokens: d.tokens[p], y, h };
      });
    return { day: d.day, total: totals[i], parts };
  });
}

/** "17 Nov" from "2026-11-17". */
export function dayLabel(day: string): string {
  const [y, m, d] = day.split("-").map(Number);
  return new Intl.DateTimeFormat("en-GB", { day: "numeric", month: "short" }).format(new Date(y, m - 1, d));
}

export function runtimeLabel(runtime: string): string {
  return runtime === "claude" ? "Claude Code" : runtime === "api" ? "API key" : runtime;
}
