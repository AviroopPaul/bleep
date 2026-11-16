import { expect, test, type Page } from "@playwright/test";

test.use({ viewport: { width: 380, height: 640 } });
const calls = (page: Page) => page.evaluate(() => (window as unknown as { __calls: string[] }).__calls);

test("Check now says up to date", async ({ page }) => {
  await page.goto("/?mock=1&g=builtin#settings");
  await page.getByRole("tab", { name: "Updates" }).click();
  await expect(page.getByRole("heading", { name: "bleep 0.2.0" })).toBeVisible();
  await expect(page.getByText("Not checked yet.")).toBeVisible();
  await page.getByRole("button", { name: "Check now" }).click();
  await expect(page.getByRole("status")).toHaveText("bleep is up to date.");
  await expect(page.getByText("Checked just now.")).toBeVisible();
});

test("with auto-install on, a new version installs and asks for a restart", async ({ page }) => {
  await page.goto("/?mock=1&g=builtin&update=new#settings");
  await page.getByRole("tab", { name: "Updates" }).click();
  await expect(page.getByRole("switch", { name: "Install updates automatically" })).toBeChecked();
  await page.getByRole("button", { name: "Check now" }).click();
  await expect(page.getByRole("status")).toHaveText("Version 0.3.0 is installed. Restart bleep to use it.");
  await page.getByRole("button", { name: "Restart now" }).click();
  expect(await calls(page)).toEqual(expect.arrayContaining(["update_check", "update_restart"]));
});

test("with auto-install off, the update waits for Install", async ({ page }) => {
  await page.goto("/?mock=1&g=builtin&update=new#settings");
  await page.getByRole("tab", { name: "Updates" }).click();
  await page.getByRole("switch", { name: "Install updates automatically" }).click();
  await expect(page.getByRole("switch", { name: "Install updates automatically" })).not.toBeChecked();
  await page.getByRole("button", { name: "Check now" }).click();
  await expect(page.getByRole("status")).toHaveText("Version 0.3.0 is available.");
  await expect(page.getByText("Auto-update from Settings")).toBeVisible();
  await page.getByRole("button", { name: "Install 0.3.0" }).click();
  await expect(page.getByRole("button", { name: "Restart now" })).toBeVisible();
  expect(await calls(page)).toEqual(expect.arrayContaining(["update_set_auto:false", "update_install"]));
});
