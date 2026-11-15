import { describe, expect, it } from "vitest";
import { authText, modeLabel, statusText, toolCount } from "./connectorsText";

describe("connectorsText", () => {
  it("says each status in plain words", () => {
    expect(statusText({ state: "connected", account: "dayblocks.vercel.app" })).toBe("Connected");
    expect(statusText({ state: "needs_sign_in" })).toBe("Needs sign-in");
    expect(statusText({ state: "needs_setup" })).toBe("Not set up");
    expect(statusText({ state: "error", message: "Could not reach dayblocks. It could not be reached." })).toBe("Could not reach dayblocks. It could not be reached.");
  });

  it("names how a server signs in", () => {
    expect(authText("oauth")).toBe("Signs in through your browser");
    expect(authText("token")).toBe("Uses a token you pasted");
    expect(authText("none")).toBe("No sign-in");
  });

  it("labels tool modes, with the default marked", () => {
    expect(modeLabel("ask", "ask")).toBe("Ask first (default)");
    expect(modeLabel("auto", "ask")).toBe("Run at once");
    expect(modeLabel("off", "auto")).toBe("Off");
  });

  it("counts the tools bleep can use", () => {
    const t = (mode: "ask" | "auto" | "off", skipped: string | null = null) => ({ name: "x", description: "", mode, default_mode: "ask" as const, skipped });
    expect(toolCount([t("ask"), t("auto"), t("off"), t("ask", "Its name is too long.")])).toBe("2 tools");
    expect(toolCount([t("auto")])).toBe("1 tool");
    expect(toolCount([])).toBe("No tools");
  });
});
