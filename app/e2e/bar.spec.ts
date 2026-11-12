import { expect, test } from "@playwright/test";

type W = { __calls: string[] };
const calls = (page: import("@playwright/test").Page) => page.evaluate(() => (window as unknown as W).__calls);

test.use({ viewport: { width: 560, height: 64 } });

test("bar shows title, time and Join only with a link", async ({ page }) => {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await page.goto("/?mock=1&cue=hour#bar");
  const bar = page.getByRole("region", { name: "Next meeting" });
  await expect(bar).toContainText("Design review");
  await expect(bar).toContainText("in 42 min");
  await expect(bar.getByRole("button", { name: "Join" })).toBeVisible();
  await expect(bar.getByRole("button", { name: "Snooze" })).toBeVisible();
  await page.screenshot({ path: test.info().outputPath("bar.png") });
  await page.goto("/?mock=1&cue=nolink#bar");
  await expect(page.getByRole("region", { name: "Next meeting" })).toContainText("Design review");
  await expect(page.getByRole("button", { name: "Join" })).toHaveCount(0);
  expect(errors).toEqual([]);
});

test("Snooze is hidden at the 10 minute cue", async ({ page }) => {
  await page.goto("/?mock=1&cue=ten#bar");
  await expect(page.getByRole("region", { name: "Next meeting" })).toContainText("in 8 min");
  await expect(page.getByRole("button", { name: "Snooze" })).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Prep me" })).toBeVisible();
});

test("buttons call their commands", async ({ page }) => {
  await page.goto("/?mock=1&cue=hour#bar");
  await page.getByRole("button", { name: "Prep me" }).click();
  await page.getByRole("button", { name: "Snooze" }).click();
  await page.getByRole("button", { name: "Join" }).click();
  await page.getByRole("button", { name: "Dismiss" }).click();
  expect(await calls(page)).toEqual(["bar_prep", "bar_snooze", "bar_join", "bar_dismiss"]);
});

test("bubble shows text and click calls bubble_click", async ({ page }) => {
  await page.setViewportSize({ width: 240, height: 72 });
  await page.goto("/?mock=1&bubble=Design%20review%20in%201%20hour#bubble");
  const b = page.getByRole("button", { name: /Design review in 1 hour/ });
  await expect(b).toBeVisible();
  await b.click();
  expect(await calls(page)).toEqual(["bubble_click"]);
});

test("bar fits 560 px with a 120 char title", async ({ page }) => {
  await page.goto("/?mock=1&cue=long#bar");
  await expect(page.getByRole("region", { name: "Next meeting" })).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(560);
  await expect(page.getByRole("button", { name: "Dismiss" })).toBeInViewport();
});
