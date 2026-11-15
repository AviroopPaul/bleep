import type { ModelInfo, Price, SpendView } from "../ipc";

export function usd(n: number): string {
  if (n === 0) return "$0";
  if (n < 0.01) return `$${n.toFixed(4).replace(/0+$/, "")}`;
  return `$${n.toFixed(2)}`;
}

/** Dollars per million tokens, as short as it can be. */
function perM(n: number): string {
  return `$${Number(n.toFixed(3))}`;
}

export function priceText(p: Price | null, free = false): string {
  if (free || (p && p.input === 0 && p.output === 0)) return "Free";
  if (!p) return "No price known, so no cost estimate";
  return `${perM(p.input)} in, ${perM(p.output)} out per million tokens`;
}

export function windowText(n: number | null): string {
  if (!n) return "";
  if (n >= 1_000_000) return `${Number((n / 1_000_000).toFixed(1))}M context`;
  return `${Math.round(n / 1000)}K context`;
}

export function modelLine(m: ModelInfo | undefined, free: boolean): string {
  if (!m) return "";
  return [m.name && m.name !== m.id ? m.name : "", windowText(m.window), priceText(m.price, free)].filter(Boolean).join(". ");
}

export function spendText(s: SpendView): string {
  let t = `Today: ${usd(s.spend.cost)} of ${usd(s.cap)}.`;
  if (s.spend.unpriced > 0) t += ` ${s.spend.unpriced} ${s.spend.unpriced === 1 ? "call has" : "calls have"} no price, so the real total is higher.`;
  if (s.over) t += " The cap is reached: check-ins, mail triage and screenshot checks wait until tomorrow. Chat still works.";
  return t;
}

export const CLAUDE_MODELS = [
  { id: "opus", label: "Opus (most capable)" },
  { id: "sonnet", label: "Sonnet (balanced)" },
  { id: "haiku", label: "Haiku (fastest)" },
];
