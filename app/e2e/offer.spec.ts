import { expect, test, type Page } from "@playwright/test";

const calls = (page: Page) => page.evaluate(() => (window as unknown as { __calls: string[] }).__calls);

test("a connector the model offered shows in the chat and turns on there", async ({ page }) => {
  await page.goto("/?mock=1&offer=linear#chat");
  const card = page.locator(".msg.bot").getByRole("group", { name: "Linear" });
  await expect(card).toBeVisible();
  await expect(page.getByText("Found a connector for this")).toBeVisible();
  await card.getByRole("switch", { name: "Linear on" }).check();
  await expect.poll(() => calls(page)).toContain("catalog_set:linear:true:");
  await expect(page.getByText("On. Ask again and bleep can use it.")).toBeVisible();
  const box = await page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth);
  expect(box, "no sideways scroll").toBe(true);
});

test("a token connector offered in the chat asks for the token there", async ({ page }) => {
  await page.goto("/?mock=1&offer=github#chat");
  const card = page.locator(".msg.bot").getByRole("group", { name: "GitHub" });
  await expect(card.getByRole("switch", { name: "GitHub on" })).toBeDisabled();
  await card.getByLabel("GitHub token").fill("ghp_x");
  await card.getByRole("switch", { name: "GitHub on" }).check();
  await expect.poll(() => calls(page)).toContain("catalog_set:github:true:ghp_x");
});
