import { describe, expect, it } from "vitest";
import { fileLabel, meterLabel, meterLevel, sizeLabel, sourceLabel, thresholdNote, tokens } from "./memoryText";

describe("memoryText", () => {
  it("formats token counts", () => {
    expect(tokens(42_300)).toBe("42K");
    expect(tokens(400)).toBe("1K");
    expect(tokens(1_000_000)).toBe("1M");
    expect(tokens(1_500_000)).toBe("1.5M");
  });

  it("labels the context meter", () => {
    expect(meterLabel(42_000, 250_000)).toBe("42K of 250K");
    expect(meterLabel(null, 250_000)).toBe("");
    expect(meterLevel(100_000, 250_000)).toBe("ok");
    expect(meterLevel(200_000, 250_000)).toBe("high");
  });

  it("explains the threshold", () => {
    expect(thresholdNote(250_000, 160_000, 200_000)).toBe("Your plan gives 200K, so bleep compacts at 160K.");
    expect(thresholdNote(250_000, 250_000, 1_000_000)).toBe("Your plan gives 1M. bleep compacts at 250K.");
    expect(thresholdNote(250_000, 160_000, null)).toBe("bleep compacts at 160K. The window shows here after the first reply.");
  });

  it("names sources, files and the size", () => {
    expect(sourceLabel("compaction")).toBe("From a compacted chat");
    expect(sourceLabel("nightly")).toBe("From your day");
    expect(fileLabel("USER.md")).toBe("About you");
    expect(fileLabel("RULES.md")).toBe("Your rules");
    expect(fileLabel("MEMORY.md")).toBe("Learned");
    expect(sizeLabel(3_000, 12_000)).toBe("25% of the memory budget used");
  });

  it("has no em dashes", () => {
    const all = [thresholdNote(1, 1, 1), thresholdNote(1, 1, null), sourceLabel("nightly"), sourceLabel("compaction"), sizeLabel(1, 2)];
    for (const s of all) expect(s).not.toContain("—");
  });
});
