import { expect, test } from "@playwright/test";

const calls = (page: import("@playwright/test").Page) => page.evaluate(() => (window as unknown as { __calls: string[] }).__calls);

test("bleep's replies render as Markdown; links open outside the panel", async ({ page }) => {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await page.goto("/?mock=1#chat");
  const box = page.getByLabel("Message bleep");
  await box.fill("show me some markdown **please**");
  await box.press("Enter");
  const bot = page.locator(".msg.bot").last();
  await expect(bot.locator("strong").first()).toHaveText("Done.");
  await expect(bot.locator("li")).toHaveText(["Standup at 10:00", "Design review at 3:00"]);
  await expect(bot.locator("code")).toHaveText("calendar_list");
  // Raw HTML from the model is shown as text, never as an element.
  await expect(bot.locator("img")).toHaveCount(0);
  // The user's own text stays literal.
  await expect(page.locator(".msg.me").last()).toHaveText("show me some markdown **please**");
  const url = page.url();
  await bot.getByRole("link", { name: "the agenda" }).click();
  expect(page.url()).toBe(url);
  expect(await calls(page)).toContain("open_url:https://example.com/agenda");
  await page.screenshot({ path: test.info().outputPath("markdown.png") });
  expect(errors).toEqual([]);
});
