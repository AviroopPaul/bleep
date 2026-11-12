import { describe, expect, it } from "vitest";
import { whenText } from "./barText";

const now = new Date("2026-11-12T09:00:00Z");
const at = (mins: number, secs = 0) => new Date(now.getTime() + mins * 60_000 + secs * 1000);

describe("whenText", () => {
  it("says in 1 hour near 60 and rounds up to whole minutes", () => {
    expect(whenText(at(60), now)).toBe("in 1 hour");
    expect(whenText(at(55), now)).toBe("in 1 hour");
    expect(whenText(at(54), now)).toBe("in 54 min");
    expect(whenText(at(41, 20), now)).toBe("in 42 min");
    expect(whenText(at(0, 30), now)).toBe("in 1 min");
    expect(whenText(at(0), now)).toBe("now");
    expect(whenText(at(-3), now)).toBe("now");
  });
});
