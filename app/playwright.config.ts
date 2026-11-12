import { defineConfig } from "@playwright/test";
import { existsSync, readdirSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

// Headless Chromium only. Use the newest cached headless shell if Playwright's own build is missing.
function headlessShell(): string | undefined {
  const cache = join(homedir(), "Library/Caches/ms-playwright");
  if (!existsSync(cache)) return undefined;
  const dirs = readdirSync(cache).filter((d) => d.startsWith("chromium_headless_shell-")).sort();
  const last = dirs.at(-1);
  const bin = last && join(cache, last, "chrome-headless-shell-mac-arm64/chrome-headless-shell");
  return bin && existsSync(bin) ? bin : undefined;
}

export default defineConfig({
  testDir: "e2e",
  use: {
    baseURL: "http://localhost:1420",
    headless: true,
    viewport: { width: 400, height: 600 },
    launchOptions: { executablePath: process.env.PW_CHROMIUM ?? headlessShell() },
  },
  webServer: { command: "npm run dev", url: "http://localhost:1420", reuseExistingServer: true, timeout: 60_000 },
});
