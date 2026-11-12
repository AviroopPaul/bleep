// Renders scripts/og/og.html to landing/og.png (1200x630).
// Needs Playwright: run from a folder where `playwright` is installed, e.g.
//   NODE_PATH=$(npm root -g) node scripts/og/render.mjs
// Set CHROME=/path/to/chrome-headless-shell to use an already installed browser.
import { chromium } from 'playwright';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const here = path.dirname(fileURLToPath(import.meta.url));
const browser = await chromium.launch(process.env.CHROME ? { executablePath: process.env.CHROME } : {});
const page = await browser.newPage({ viewport: { width: 1200, height: 630 } });
await page.goto('file://' + path.join(here, 'og.html'));
await page.evaluate(() => document.fonts.ready);
await page.waitForTimeout(300);
await page.screenshot({ path: path.join(here, '../../landing/og.png') });
await browser.close();
console.log('wrote landing/og.png');
