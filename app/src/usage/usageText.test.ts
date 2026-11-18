import { describe, expect, it } from "vitest";
import { busiest, costNote, dayLabel, heatLevel, hourLabel, money, purposeLabel, short, stacks } from "./usageText";
import type { UsageView } from "../ipc";

describe("usage text", () => {
  it("shortens token counts", () => {
    expect(short(950)).toBe("950");
    expect(short(42_300)).toBe("42.3K");
    expect(short(1_500_000)).toBe("1.5M");
  });

  it("formats money and the cost note", () => {
    expect(money(0.004)).toBe("under $0.01");
    expect(money(1.5)).toBe("$1.50");
    const v = { cost: null, unpriced: 0 } as unknown as UsageView;
    expect(costNote(v)).toBe("Claude plan, no API bill");
    expect(costNote({ ...v, cost: 0.2, unpriced: 3 })).toBe("3 calls have no price");
  });

  it("shades the heatmap from 0 to 4", () => {
    expect(heatLevel(0, 10)).toBe(0);
    expect(heatLevel(1, 10)).toBe(1);
    expect(heatLevel(10, 10)).toBe(4);
    expect(heatLevel(3, 0)).toBe(0);
  });

  it("names the busiest hour", () => {
    const heat = Array.from({ length: 7 }, () => Array(24).fill(0));
    expect(busiest(heat)).toBe("");
    heat[1][9] = 5;
    heat[4][21] = 2;
    expect(busiest(heat)).toBe("Busiest: Tue, 9 AM");
    expect(hourLabel(0)).toBe("12 AM");
    expect(hourLabel(12)).toBe("12 PM");
  });

  it("stacks bars bottom up, scaled to the tallest day", () => {
    const s = stacks(
      [
        { day: "2026-11-16", calls: 1, tokens: { chat: 100 } },
        { day: "2026-11-17", calls: 2, tokens: { chat: 150, triage: 50 } },
      ],
      ["chat", "triage"],
      100,
    );
    expect(s[0].parts).toEqual([{ purpose: "chat", tokens: 100, y: 50, h: 50 }]);
    expect(s[1].total).toBe(200);
    expect(s[1].parts.map((p) => [p.purpose, p.y, p.h])).toEqual([["chat", 25, 75], ["triage", 0, 25]]);
  });

  it("labels days and purposes", () => {
    expect(dayLabel("2026-11-17")).toBe("17 Nov");
    expect(purposeLabel("triage")).toBe("Mail sorting");
    expect(purposeLabel("travel")).toBe("Travel");
  });

  it("has no em dashes", () => {
    for (const s of [costNote({ cost: null, unpriced: 0 } as unknown as UsageView), busiest([[1]])]) expect(s).not.toContain("—");
  });
});
