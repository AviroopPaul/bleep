import { describe, expect, it } from "vitest";
import { defaultAt, group, repeatLabel, whenLabel } from "./remindersText";
import type { ReminderItem } from "../ipc";

const item = (id: number, status: ReminderItem["status"]): ReminderItem => ({ id, text: `r${id}`, due: "2026-11-17T19:00", next: "2026-11-17T19:00", repeat: "none", status, snoozed: false });

describe("reminders text", () => {
  const now = new Date(2026, 10, 17, 10, 15); // Tue 17 Nov 2026, 10:15

  it("labels times relative to today", () => {
    expect(whenLabel("2026-11-17T19:00", now)).toBe("Today, 7:00 PM");
    expect(whenLabel("2026-11-18T09:05", now)).toBe("Tomorrow, 9:05 AM");
    expect(whenLabel("2026-11-16T09:00", now)).toBe("Yesterday, 9:00 AM");
    expect(whenLabel("2026-11-20T09:00", now)).toBe("Fri 20 Nov, 9:00 AM");
  });

  it("defaults quick add to the next full hour, 30 minutes away at least", () => {
    expect(defaultAt(now)).toBe("2026-11-17T11:00");
    expect(defaultAt(new Date(2026, 10, 17, 10, 45))).toBe("2026-11-17T12:00");
    expect(defaultAt(new Date(2026, 10, 17, 23, 40))).toBe("2026-11-18T01:00");
  });

  it("groups by status and labels repeats", () => {
    const g = group([item(1, "fired"), item(2, "scheduled"), item(3, "done")]);
    expect([g.due[0].id, g.upcoming[0].id, g.done[0].id]).toEqual([1, 2, 3]);
    expect(repeatLabel("none")).toBe("");
    expect(repeatLabel("weekdays")).toBe("Weekdays");
  });
});
