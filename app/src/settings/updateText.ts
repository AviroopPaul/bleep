// Copy for the Updates tab. No em dashes in any of it.
import type { UpdatePhase } from "../ipc";

export function phaseText(p: UpdatePhase): string {
  switch (p.state) {
    case "idle": return "";
    case "checking": return "Checking for updates...";
    case "up_to_date": return "bleep is up to date.";
    case "available": return `Version ${p.version} is available.`;
    case "downloading": {
      const pct = p.total ? ` ${Math.min(100, Math.floor((p.done / p.total) * 100))}%` : "";
      return `Downloading ${p.version}...${pct}`;
    }
    case "ready": return `Version ${p.version} is installed. Restart bleep to use it.`;
    case "error": return p.message;
  }
}

/** "Checked just now", "Checked 5 min ago", "Checked 3 h ago", "Checked 2 days ago". */
export function checkedText(lastChecked: string | null, now: Date = new Date()): string {
  if (!lastChecked) return "Not checked yet.";
  const mins = Math.max(0, Math.floor((now.getTime() - new Date(lastChecked).getTime()) / 60_000));
  if (mins < 1) return "Checked just now.";
  if (mins < 60) return `Checked ${mins} min ago.`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `Checked ${hours} h ago.`;
  const days = Math.floor(hours / 24);
  return `Checked ${days} ${days === 1 ? "day" : "days"} ago.`;
}
