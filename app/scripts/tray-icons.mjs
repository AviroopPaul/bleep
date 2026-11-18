// Draws the menu bar icons: src-tauri/icons/tray/<buddy>.svg -> <buddy>.png.
// Black outlines on a clear background, 132px tall, like icons/tray.png. macOS uses them
// as template images, so only the alpha counts.
// Run from app/: node scripts/tray-icons.mjs
// Set PW_CHROMIUM to a Chromium binary if Playwright's own browser is not installed.
import { chromium } from "@playwright/test";
import fs from "node:fs";
import path from "node:path";

const dir = path.resolve("src-tauri/icons/tray");
const H = 132;
const browser = await chromium.launch({ headless: true, executablePath: process.env.PW_CHROMIUM || undefined });
const page = await browser.newPage({ deviceScaleFactor: 1 });
for (const f of fs.readdirSync(dir).filter((f) => f.endsWith(".svg")).sort()) {
  const svg = fs.readFileSync(path.join(dir, f), "utf8");
  const [, , w, h] = svg.match(/viewBox="([^"]+)"/)[1].split(/\s+/).map(Number);
  const W = Math.round((H * w) / h);
  await page.setViewportSize({ width: W, height: H });
  await page.setContent(`<style>html,body{margin:0;background:transparent}svg{display:block;width:${W}px;height:${H}px}</style>${svg}`);
  const out = path.join(dir, f.replace(/\.svg$/, ".png"));
  await page.screenshot({ path: out, omitBackground: true });
  console.log(`${path.basename(out)} ${W}x${H}`);
}
await browser.close();
