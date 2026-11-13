import { stepLabel } from "../chat/chatReducer";
import type { ActivityItem } from "../ipc";

const WHAT: Record<string, string> = {
  calendar_create: "add to your calendar",
  calendar_update: "change an event",
  calendar_delete: "delete an event",
  calendar_respond: "reply to an invite",
  gmail_send: "send an email",
};
const STATUS: Record<string, string> = { pending: "Waiting", approved: "Approved", declined: "Declined", expired: "Expired", done: "Done", failed: "Failed" };
const MODEL: Record<string, string> = { triage: "Sorted mail", chat: "Chat reply" };

/** One Activity row in plain words. */
export function activityTitle(a: Pick<ActivityItem, "kind" | "name" | "detail">): string {
  if (a.kind === "approval") return `${STATUS[a.detail ?? ""] ?? "Card"}: ${WHAT[a.name] ?? a.name}`;
  if (a.kind === "model") return `${MODEL[a.name] ?? a.name} (${a.detail ?? "model"} model)`;
  return stepLabel(a.name);
}

export function activityTime(at: string): string {
  return new Intl.DateTimeFormat("en-GB", { weekday: "short", day: "numeric", month: "short", hour: "numeric", minute: "2-digit", hour12: true })
    .format(new Date(at))
    .replace(/ /g, " ");
}
