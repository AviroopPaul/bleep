import { describe, expect, it } from "vitest";
import { activityTitle } from "./activityText";

const item = (kind: string, name: string, detail: string | null = null, ok = true) => ({ kind, name, detail, ok, at: "2026-11-12T10:00:00.000Z" });

describe("activityTitle", () => {
  it("says what happened in plain words", () => {
    expect(activityTitle(item("tool", "calendar_list"))).toBe("Checked your calendar");
    expect(activityTitle(item("tool", "gmail_draft"))).toBe("Saved a draft");
    expect(activityTitle(item("approval", "calendar_create", "done"))).toBe("Done: add to your calendar");
    expect(activityTitle(item("approval", "gmail_send", "declined"))).toBe("Declined: send an email");
    expect(activityTitle(item("approval", "gmail_send", "pending"))).toBe("Waiting: send an email");
    expect(activityTitle(item("model", "triage", "small"))).toBe("Sorted mail (small model)");
    expect(activityTitle(item("model", "chat", "main"))).toBe("Chat reply (main model)");
    expect(activityTitle(item("tool", "dayblocks_tasks"))).toBe("dayblocks_tasks");
  });
});
