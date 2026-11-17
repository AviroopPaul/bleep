import { describe, expect, it } from "vitest";
import type { VoiceConfig, VoiceStatus } from "../ipc";
import { SENSITIVITY, deviceOptions, levelPct, phaseText, speakNote } from "./voiceText";

const cfg: VoiceConfig = { wake: true, phrase: "hey bleep", sensitivity: "normal", mic: "", speak: true, speaker: "", voice: "" };
const st = (p: Partial<VoiceStatus>): VoiceStatus => ({ phase: "off", detail: "", mic: null, level: 0, last_heard: null, progress: null, ...p });

describe("voice settings text", () => {
  it("names each phase", () => {
    expect(phaseText(st({}), { ...cfg, wake: false })).toBe("Off");
    expect(phaseText(st({}), cfg)).toBe("Starting...");
    expect(phaseText(st({ phase: "downloading", progress: 0.456 }), cfg)).toBe("Downloading 46%");
    expect(phaseText(st({ phase: "listening" }), cfg)).toBe("Listening");
    expect(phaseText(st({ phase: "listening", detail: "No sound from the mic." }), cfg)).toBe("No sound");
    expect(phaseText(st({ phase: "error" }), cfg)).toBe("Error");
  });

  it("offers Auto first and keeps an unplugged choice", () => {
    expect(deviceOptions(["MacBook Air Microphone", "AirPods"], "AirPods", "")).toEqual([
      { value: "", label: "Auto (AirPods)" },
      { value: "MacBook Air Microphone", label: "MacBook Air Microphone" },
      { value: "AirPods", label: "AirPods" },
    ]);
    const o = deviceOptions(["MacBook Air Microphone"], null, "Yeti");
    expect(o[0].label).toBe("Auto (system default)");
    expect(o[o.length - 1]).toEqual({ value: "Yeti", label: "Yeti (not connected)" });
  });

  it("scales the level bar", () => {
    expect(levelPct(0)).toBe(0);
    expect(levelPct(0.2)).toBe(100);
    expect(levelPct(5)).toBe(100);
    expect(levelPct(0.05)).toBe(50);
  });

  it("lists the three sensitivities, normal in the middle", () => {
    expect(SENSITIVITY.map((o) => o.value)).toEqual(["strict", "normal", "loose"]);
  });

  it("explains when replies are spoken", () => {
    expect(speakNote(cfg)).toMatch(/aloud/);
    expect(speakNote({ ...cfg, speak: false })).toBe("Replies show in the chat only.");
  });
});
