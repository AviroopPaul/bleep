import { describe, expect, it } from "vitest";
import { checkedText, phaseText } from "./updateText";

describe("phaseText", () => {
  it("names each phase", () => {
    expect(phaseText({ state: "idle" })).toBe("");
    expect(phaseText({ state: "up_to_date" })).toBe("bleep is up to date.");
    expect(phaseText({ state: "available", version: "0.3.0", notes: null })).toBe("Version 0.3.0 is available.");
    expect(phaseText({ state: "ready", version: "0.3.0" })).toBe("Version 0.3.0 is installed. Restart bleep to use it.");
    expect(phaseText({ state: "error", message: "Could not check for updates: offline" })).toBe("Could not check for updates: offline");
  });

  it("shows download percent only when the size is known", () => {
    expect(phaseText({ state: "downloading", version: "0.3.0", done: 50, total: 200 })).toBe("Downloading 0.3.0... 25%");
    expect(phaseText({ state: "downloading", version: "0.3.0", done: 50, total: null })).toBe("Downloading 0.3.0...");
  });
});

describe("checkedText", () => {
  const now = new Date("2026-11-16T12:00:00Z");
  it("reads as a short age", () => {
    expect(checkedText(null, now)).toBe("Not checked yet.");
    expect(checkedText("2026-11-16T11:59:40Z", now)).toBe("Checked just now.");
    expect(checkedText("2026-11-16T11:55:00Z", now)).toBe("Checked 5 min ago.");
    expect(checkedText("2026-11-16T09:00:00Z", now)).toBe("Checked 3 h ago.");
    expect(checkedText("2026-11-15T11:00:00Z", now)).toBe("Checked 1 day ago.");
  });
});
