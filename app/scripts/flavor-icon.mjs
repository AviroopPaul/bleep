// Renders the app icon of a nightly or local build: the normal icon with a band that names the
// build, so it is easy to tell apart in Finder, Spotlight and notifications. Headless Chromium only.
// Run from app/:
//   node scripts/flavor-icon.mjs local && npx tauri icon src-tauri/icons-local/source.png -o src-tauri/icons-local
//   node scripts/flavor-icon.mjs nightly && npx tauri icon src-tauri/icons-nightly/source.png -o src-tauri/icons-nightly
import { chromium } from "@playwright/test";
import { readFileSync, mkdirSync, existsSync, readdirSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

const BANDS = {
  local: { text: "LOCAL", color: "#b5532f" },
  nightly: { text: "NIGHTLY", color: "#4b3f8f" },
};
const flavor = process.argv[2];
const band = BANDS[flavor];
if (!band) {
  console.error(`usage: node scripts/flavor-icon.mjs ${Object.keys(BANDS).join("|")}`);
  process.exit(1);
}

const cache = join(homedir(), "Library/Caches/ms-playwright");
const shell = existsSync(cache) && readdirSync(cache).filter((d) => d.startsWith("chromium_headless_shell-")).sort().at(-1);
const executablePath = shell ? join(cache, shell, "chrome-headless-shell-mac-arm64/chrome-headless-shell") : undefined;

const src = readFileSync("src-tauri/icons/icon.png").toString("base64");
const html = `<html><body style="margin:0;background:transparent">
<div style="position:relative;width:1024px;height:1024px">
  <img src="data:image/png;base64,${src}" style="width:1024px;height:1024px;display:block">
  <div style="position:absolute;left:50%;bottom:118px;transform:translateX(-50%);padding:18px 56px;border-radius:999px;
    background:${band.color};color:#fff;font:800 ${band.text.length > 5 ? 110 : 140}px/1 ui-rounded,-apple-system,Helvetica,sans-serif;
    letter-spacing:4px;white-space:nowrap;box-shadow:0 10px 30px rgba(0,0,0,.25)">${band.text}</div>
</div></body></html>`;

const dir = `src-tauri/icons-${flavor}`;
mkdirSync(dir, { recursive: true });
const browser = await chromium.launch({ headless: true, executablePath });
const page = await browser.newPage({ viewport: { width: 1024, height: 1024 } });
await page.setContent(html);
await page.screenshot({ path: `${dir}/source.png`, omitBackground: true });
await browser.close();
console.log(`wrote ${dir}/source.png`);
