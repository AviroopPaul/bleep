// Copy for the Reminders tab. Reminder times are local "YYYY-MM-DDTHH:MM". No em dashes.
import type { ReminderItem, RepeatKind } from "../ipc";

export const REPEATS: { value: RepeatKind; label: string }[] = [
  { value: "none", label: "Once" },
  { value: "daily", label: "Every day" },
  { value: "weekdays", label: "Weekdays" },
  { value: "weekly", label: "Every week" },
  { value: "monthly", label: "Every month" },
];

export const repeatLabel = (r: RepeatKind) => (r === "none" ? "" : REPEATS.find((x) => x.value === r)?.label ?? r);

function parse(local: string): Date {
  const [d, t = "00:00"] = local.split("T");
  const [y, m, day] = d.split("-").map(Number);
  const [h, min] = t.split(":").map(Number);
  return new Date(y, m - 1, day, h, min);
}

const pad = (n: number) => String(n).padStart(2, "0");

/** A Date as the value of a datetime-local input. */
export const toLocalInput = (d: Date) => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`;

/** The quick-add default: the next full hour, at least 30 minutes away. */
export function defaultAt(now: Date): string {
  const d = new Date(now);
  d.setMinutes(0, 0, 0);
  d.setHours(d.getHours() + (now.getMinutes() >= 30 ? 2 : 1));
  return toLocalInput(d);
}

/** "Today, 7:00 PM", "Tomorrow, 9:00 AM", "Fri 21 Nov, 9:00 AM". */
export function whenLabel(local: string, now: Date): string {
  const d = parse(local);
  const time = new Intl.DateTimeFormat("en-US", { hour: "numeric", minute: "2-digit" }).format(d);
  const day = (x: Date) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
  const diff = Math.round((day(d) - day(now)) / 86_400_000);
  if (diff === 0) return `Today, ${time}`;
  if (diff === 1) return `Tomorrow, ${time}`;
  if (diff === -1) return `Yesterday, ${time}`;
  const date = new Intl.DateTimeFormat("en-GB", { weekday: "short", day: "numeric", month: "short" }).format(d);
  return `${date.replace(",", "")}, ${time}`;
}

export interface Groups {
  due: ReminderItem[];
  upcoming: ReminderItem[];
  done: ReminderItem[];
}

export function group(list: ReminderItem[]): Groups {
  return {
    due: list.filter((r) => r.status === "fired"),
    upcoming: list.filter((r) => r.status === "scheduled"),
    done: list.filter((r) => r.status === "done"),
  };
}

export const SNOOZES = [
  { minutes: 10, label: "10 min" },
  { minutes: 60, label: "1 hour" },
  { minutes: 24 * 60, label: "Tomorrow" },
];
