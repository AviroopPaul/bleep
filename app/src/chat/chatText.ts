import type { ToolStep } from "../ipc";
import { stepLabel } from "./chatReducer";

/** The app or source behind a tool, for the one-line summary. */
const SOURCES: [string, string][] = [
  ["calendar_", "Calendar"],
  ["gmail_", "Gmail"],
  ["drive_", "Drive"],
  ["files_", "your files"],
  ["memory_", "memory"],
  ["bleep_now", "the clock"],
  ["connector_offer", "connectors"],
];

function source(name: string): string {
  const hit = SOURCES.find(([p]) => name === p || name.startsWith(p));
  if (hit) return hit[1];
  // Remote MCP tools arrive as "server__tool".
  const server = name.split("__")[0];
  return server ? server.charAt(0).toUpperCase() + server.slice(1) : name;
}

export function joinAnd(list: string[]): string {
  if (list.length <= 1) return list[0] ?? "";
  return `${list.slice(0, -1).join(", ")} and ${list[list.length - 1]}`;
}

export interface StepsSummary {
  /** The step running now, as "Checked your calendar...". */
  running: string | null;
  /** One line for the whole turn when nothing runs. */
  text: string;
  failed: number;
}

export function stepsSummary(steps: ToolStep[]): StepsSummary {
  const live = [...steps].reverse().find((s) => s.ok === null);
  const failed = steps.filter((s) => s.ok === false).length;
  const sources = [...new Set(steps.map((s) => source(s.name)))];
  const text = steps.length === 1 ? stepLabel(steps[0].name) : `Used ${joinAnd(sources)}`;
  return { running: live ? `${stepLabel(live.name)}...` : null, text, failed };
}

export function greeting(now: Date): string {
  const h = now.getHours();
  if (h < 5) return "Up late?";
  if (h < 12) return "Good morning";
  if (h < 17) return "Good afternoon";
  return "Good evening";
}

export type SuggestionIcon = "calendar" | "mail" | "reminder" | "agents" | "help";
export interface Suggestion {
  icon: SuggestionIcon;
  text: string;
  source: string;
}

/** Starters for an empty chat, from what bleep can reach right now. */
export function suggestions(connected: Set<string>, agents: number, waiting: number): Suggestion[] {
  const out: Suggestion[] = [];
  if (connected.has("calendar")) out.push({ icon: "calendar", text: "What's left on my calendar today?", source: "Calendar" });
  if (connected.has("gmail")) out.push({ icon: "mail", text: "Catch me up on unread mail", source: "Gmail" });
  if (waiting > 0) out.push({ icon: "agents", text: waiting === 1 ? "Which agent needs me?" : `Which ${waiting} agents need me?`, source: "Agents" });
  else if (agents > 0) out.push({ icon: "agents", text: "What are my agents doing?", source: "Agents" });
  out.push({ icon: "reminder", text: "Remind me at 5 to stretch", source: "Reminders" });
  if (!connected.has("calendar") && !connected.has("gmail")) out.push({ icon: "help", text: "What can you do?", source: "bleep" });
  return out.slice(0, 4);
}

/** "Calendar, Gmail and Reminders are on." Reminders are always there. */
export function reachLine(labels: string[]): string {
  const all = [...labels, "Reminders"];
  return `${joinAnd(all)} ${all.length === 1 ? "is" : "are"} on.`;
}
