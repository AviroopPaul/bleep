import { describe, expect, it } from "vitest";
import type { ToolStep } from "../ipc";
import { greeting, joinAnd, reachLine, stepsSummary, suggestions } from "./chatText";

const step = (name: string, ok: boolean | null): ToolStep => ({ id: name, name, args: {}, ok, preview: null });

describe("stepsSummary", () => {
  it("uses the step label for one step", () => {
    expect(stepsSummary([step("bleep_now", true)])).toEqual({ running: null, text: "Checked the time", failed: 0 });
  });
  it("names the sources for several steps", () => {
    expect(stepsSummary([step("calendar_list", true), step("gmail_search", true), step("calendar_next", true)]).text).toBe("Used Calendar and Gmail");
  });
  it("shows the running step", () => {
    expect(stepsSummary([step("calendar_list", true), step("gmail_search", null)]).running).toBe("Searched your mail...");
  });
  it("counts failures and names remote servers", () => {
    const s = stepsSummary([step("linear__list_issues", false), step("files_read", true)]);
    expect(s).toEqual({ running: null, text: "Used Linear and your files", failed: 1 });
  });
});

describe("text helpers", () => {
  it("joins with and", () => {
    expect(joinAnd(["a"])).toBe("a");
    expect(joinAnd(["a", "b", "c"])).toBe("a, b and c");
  });
  it("greets by the hour", () => {
    expect(greeting(new Date(2026, 0, 1, 9))).toBe("Good morning");
    expect(greeting(new Date(2026, 0, 1, 14))).toBe("Good afternoon");
    expect(greeting(new Date(2026, 0, 1, 20))).toBe("Good evening");
  });
  it("says what is on", () => {
    expect(reachLine([])).toBe("Reminders is on.");
    expect(reachLine(["Calendar", "Gmail"])).toBe("Calendar, Gmail and Reminders are on.");
  });
});

describe("suggestions", () => {
  it("follows what is connected", () => {
    expect(suggestions(new Set(["calendar", "gmail"]), 0, 0).map((s) => s.source)).toEqual(["Calendar", "Gmail", "Reminders"]);
  });
  it("puts waiting agents first after mail", () => {
    expect(suggestions(new Set(["gmail"]), 3, 2).map((s) => s.text)).toEqual(["Catch me up on unread mail", "Which 2 agents need me?", "Remind me at 5 to stretch"]);
  });
  it("offers help when nothing is connected", () => {
    expect(suggestions(new Set(), 0, 0).map((s) => s.icon)).toEqual(["reminder", "help"]);
  });
});
