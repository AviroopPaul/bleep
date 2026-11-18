// Renders the "bleep (dev)" app icon: the normal icon with a "DEV" band, so the dev build is
// easy to tell apart in Finder, Spotlight and notifications. Headless Chromium only.
// Run from app/: node scripts/dev-icon.mjs && npx tauri icon src-tauri/icons-dev/source.png -o src-tauri/icons-dev
import { chromium } from "@playwright/test";
import { readFileSync, mkdirSync, existsSync, readdirSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

const cache = join(homedir(), "Library/Caches/ms-playwright");
const shell = existsSync(cache) && readdirSync(cache).filter((d) => d.startsWith("chromium_headless_shell-")).sort().at(-1);
const executablePath = shell ? join(cache, shell, "chrome-headless-shell-mac-arm64/chrome-headless-shell") : undefined;

const src = readFileSync("src-tauri/icons/icon.png").toString("base64");
const html = `<html><body style="margin:0;background:transparent">
<div style="position:relative;width:1024px;height:1024px">
  <img src="data:image/png;base64,${src}" style="width:1024px;height:1024px;display:block">
  <div style="position:absolute;left:50%;bottom:118px;transform:translateX(-50%);padding:18px 64px;border-radius:999px;
    background:#b5532f;color:#fff;font:800 150px/1 ui-rounded,-apple-system,Helvetica,sans-serif;letter-spacing:6px;
    box-shadow:0 10px 30px rgba(0,0,0,.25)">DEV</div>
</div></body></html>`;

mkdirSync("src-tauri/icons-dev", { recursive: true });
const browser = await chromium.launch({ headless: true, executablePath });
const page = await browser.newPage({ viewport: { width: 1024, height: 1024 } });
await page.setContent(html);
await page.screenshot({ path: "src-tauri/icons-dev/source.png", omitBackground: true });
await browser.close();
console.log("wrote src-tauri/icons-dev/source.png");
