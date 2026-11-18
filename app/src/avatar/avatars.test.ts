import { describe, expect, it } from "vitest";
import { BUDDIES, DEFAULT_AVATAR, avatarSvg, finishOf, mix, normalize, switchKind } from "./avatars";

describe("avatars", () => {
  it("normalizes bad values like the Rust side", () => {
    expect(normalize(null)).toEqual(DEFAULT_AVATAR);
    expect(normalize({ kind: "dragon" as never, color: "#123456", finish: "flat" })).toEqual(DEFAULT_AVATAR);
    expect(normalize({ kind: "sprout", color: "red", finish: "shiny" as never })).toEqual({ kind: "sprout", color: "#FF6B4A", finish: "glossy" });
    expect(normalize({ kind: "boop", color: "#ff4fa3", finish: "flat" }).color).toBe("#FF4FA3");
  });

  it("switching buddy swaps a default color but keeps a picked one", () => {
    expect(switchKind(DEFAULT_AVATAR, "blip").color).toBe("#3D6BFF");
    expect(switchKind({ ...DEFAULT_AVATAR, color: "#FF4FA3" }, "blip").color).toBe("#FF4FA3");
  });

  it("draws every buddy in every finish", () => {
    for (const b of BUDDIES) {
      for (const finish of ["flat", "glossy", "plush"] as const) {
        const svg = avatarSvg({ kind: b.id, color: b.color, finish });
        expect(svg.startsWith("<svg") || svg.trim().startsWith("<svg"), b.id).toBe(true);
      }
    }
  });

  it("uses the finish: gradients for glossy, a felt filter for plush", () => {
    const glossy = avatarSvg({ kind: "blip", color: "#3D6BFF", finish: "glossy" });
    expect(glossy).toContain("linearGradient");
    const plush = avatarSvg({ kind: "gumdrop", color: "#1FC48E", finish: "plush" });
    expect(plush).toContain("feTurbulence");
    const flat = avatarSvg({ kind: "puff", color: "#38BDF8", finish: "flat" });
    expect(flat).not.toContain("Gradient");
    expect(flat).toContain('fill="#38BDF8"');
  });

  it("gives each drawing its own ids so many can share a page", () => {
    const a = avatarSvg({ kind: "blip", color: "#3D6BFF", finish: "plush" });
    const b = avatarSvg({ kind: "blip", color: "#3D6BFF", finish: "plush" });
    const id = (s: string) => s.match(/id="(av\d+)g"/)![1];
    expect(id(a)).not.toBe(id(b));
  });

  it("Boop is always flat and the droid has no finish", () => {
    expect(finishOf({ kind: "boop", color: "#8B5CF6", finish: "plush" })).toBe("flat");
    expect(avatarSvg({ kind: "boop", color: "#8B5CF6", finish: "plush" })).not.toContain("feTurbulence");
    expect(finishOf({ kind: "droid", color: "#2F5C93", finish: "glossy" })).toBeNull();
    expect(avatarSvg({ kind: "droid", color: "#2F5C93", finish: "glossy" })).toContain('class="lens"');
  });

  it("still drawings have no shadow or mood effects", () => {
    const s = avatarSvg({ kind: "spark", color: "#FFB020", finish: "glossy" }, { still: true });
    expect(s).not.toContain('class="shadow"');
    expect(s).not.toContain('class="fx"');
  });

  it("mixes colors", () => {
    expect(mix("#000000", "#ffffff", 0.5)).toBe("#808080");
  });
});
