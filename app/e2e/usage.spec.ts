import { expect, test, type Page } from "@playwright/test";

const calls = (page: Page) => page.evaluate(() => (window as unknown as { __calls: string[] }).__calls);

test("Settings > Usage shows tiles, heatmap, bars and models, and switches range", async ({ page }) => {
  await page.goto("/?mock=1&memory=1&usage=1#settings");
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
  // Nothing spills sideways in the narrow window.
  const fits = await panel.evaluate((el) => el.scrollWidth <= el.clientWidth);
  expect(fits).toBe(true);
});

test("Usage with no calls says so", async ({ page }) => {
  await page.goto("/?mock=1&usage=empty#settings/usage");
  await expect(page.getByText("No model calls in this range yet.")).toBeVisible();
  await expect(page.locator(".us-heat")).toHaveCount(0);
});

test("usage shows API cost when an API key ran", async ({ page }) => {
  await page.goto("/?mock=1&usage=api#settings/usage");
  await expect(page.locator(".us-v").filter({ hasText: "$0.42" })).toBeVisible();
});

test("local build shows (local) in the header and the header still fits", async ({ page }) => {
  await page.goto("/?mock=1&flavor=local&agents=1&rem=1&ctxhigh=1#chat");
  await expect(page.getByRole("heading", { name: "bleep (local)" })).toBeVisible();
  await expect(page.locator(".head-dev")).toHaveText("local");
  const fits = await page.locator(".panel-head").evaluate((el) => el.scrollWidth <= el.clientWidth);
  expect(fits).toBe(true);
});

test("chat has no Usage tab; the gear opens Settings", async ({ page }) => {
  await page.goto("/?mock=1#chat");
  await expect(page.getByRole("tab", { name: "Usage", exact: true })).toHaveCount(0);
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  await expect.poll(() => calls(page)).toContain("settings_open:");
});

test("a link to a section opens Settings there", async ({ page }) => {
  await page.goto("/?mock=1&g=builtin#settings/connectors");
  await expect(page.getByRole("tab", { name: "Connectors" })).toHaveAttribute("aria-selected", "true");
  await expect(page.getByRole("heading", { name: "Connectors", level: 2 })).toBeVisible();
  await page.goto("/?mock=1#settings/nope");
  await expect(page.getByRole("tab", { name: "Model" })).toHaveAttribute("aria-selected", "true");
});
