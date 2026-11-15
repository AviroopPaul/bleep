import { describe, expect, it } from "vitest";
import { modelLine, priceText, spendText, usd, windowText } from "./modelText";

describe("model text", () => {
  it("formats dollars", () => {
    expect(usd(0)).toBe("$0");
    expect(usd(0.0042)).toBe("$0.0042");
    expect(usd(1)).toBe("$1.00");
    expect(usd(0.125)).toBe("$0.13");
  });

  it("says the price per million tokens, free, or unknown", () => {
    expect(priceText({ input: 3, output: 15, cache_read: 0.3, cache_write: 3.75 })).toBe("$3 in, $15 out per million tokens");
    expect(priceText({ input: 0.075, output: 0.3, cache_read: 0, cache_write: 0 })).toBe("$0.075 in, $0.3 out per million tokens");
    expect(priceText(null, true)).toBe("Free");
    expect(priceText({ input: 0, output: 0, cache_read: 0, cache_write: 0 })).toBe("Free");
    expect(priceText(null)).toBe("No price known, so no cost estimate");
  });

  it("shows context windows in K or M", () => {
    expect(windowText(200_000)).toBe("200K context");
    expect(windowText(1_048_576)).toBe("1M context");
    expect(windowText(null)).toBe("");
  });

  it("joins a model line without the name when it is the id", () => {
    expect(modelLine({ id: "a/b", name: "Model B", window: 128_000, price: null }, false)).toBe("Model B. 128K context. No price known, so no cost estimate");
    expect(modelLine({ id: "x", name: "x", window: null, price: null }, true)).toBe("Free");
    expect(modelLine(undefined, false)).toBe("");
  });

  it("explains today's spend and the cap", () => {
    expect(spendText({ runtime: "api", spend: { cost: 0.12, unpriced: 0, calls: 4 }, cap: 1, over: false })).toBe("Today: $0.12 of $1.00.");
    const over = spendText({ runtime: "api", spend: { cost: 1.2, unpriced: 1, calls: 9 }, cap: 1, over: true });
    expect(over).toContain("1 call has no price");
    expect(over).toContain("Chat still works.");
    expect(over).not.toContain("—");
  });
});
