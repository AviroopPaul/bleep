import { expect, test, type Page } from "@playwright/test";

const calls = (page: Page) => page.evaluate(() => (window as unknown as { __calls: string[] }).__calls);

test("usage tab shows tiles, heatmap, bars and models, and switches range", async ({ page }) => {
  await page.goto("/?mock=1&memory=1&usage=1#chat");
  await page.getByRole("tab", { name: "Usage", exact: true }).click();
  const panel = page.getByRole("tabpanel", { name: "Usage" });
  await expect(panel.getByText("Model calls")).toBeVisible();
  await expect(panel.getByRole("meter", { name: "Chat context" })).toHaveAttribute("aria-valuetext", "120K of 250K");
  await expect(panel.getByText("Claude plan, no API bill")).toBeVisible();
  await expect(panel.getByRole("img", { name: "Model calls by weekday and hour" })).toBeVisible();
  await expect(panel.locator(".us-hrow i")).toHaveCount(7 * 24);
  await expect(panel.getByText("Busiest: ")).toBeVisible();
  await expect(panel.locator(".us-bars rect").first()).toBeVisible();
  await expect(panel.getByRole("cell", { name: /sonnet/ })).toBeVisible();
  await expect(panel.getByText("calendar_list")).toBeVisible();
  await expect(panel.getByText("auto-approved")).toBeVisible();
  expect(await calls(page)).toContain("usage_summary:7");
  await panel.getByRole("radio", { name: "30 days" }).click();
  await expect.poll(() => calls(page)).toContain("usage_summary:30");
  await expect(panel.locator(".us-bars rect")).not.toHaveCount(0);
  // Nothing spills sideways at 400px.
  const fits = await panel.evaluate((el) => el.scrollWidth <= el.clientWidth);
  expect(fits).toBe(true);
});

test("usage tab with no calls says so", async ({ page }) => {
  await page.goto("/?mock=1&usage=empty#chat");
  await page.getByRole("tab", { name: "Usage", exact: true }).click();
  await expect(page.getByText("No model calls in this range yet.")).toBeVisible();
  await expect(page.locator(".us-heat")).toHaveCount(0);
});

test("usage shows API cost when an API key ran", async ({ page }) => {
  await page.goto("/?mock=1&usage=api#chat");
  await page.getByRole("tab", { name: "Usage", exact: true }).click();
  await expect(page.locator(".us-v").filter({ hasText: "$0.42" })).toBeVisible();
});

test("dev build shows (dev) in the header and the header still fits", async ({ page }) => {
  await page.goto("/?mock=1&dev=1&agents=1&rem=1&ctxhigh=1#chat");
  await expect(page.getByRole("heading", { name: "bleep (dev)" })).toBeVisible();
  await expect(page.locator(".head-dev")).toHaveText("dev");
  const fits = await page.locator(".panel-head").evaluate((el) => el.scrollWidth <= el.clientWidth);
  expect(fits).toBe(true);
});
