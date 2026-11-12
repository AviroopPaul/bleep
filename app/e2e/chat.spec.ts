import { expect, test } from "@playwright/test";

test("chat shows history, streams a tool step and a reply, and locks input while streaming", async ({ page }) => {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await page.goto("/?mock=1#chat");

  await expect(page.locator(".msg.me")).toHaveText(["hello"]);
  await expect(page.locator(".msg.bot")).toHaveText(["Hi! I am bleep."]);
  await expect(page.locator(".badge")).toHaveText("Claude Code");

  const box = page.getByLabel("Message bleep");
  await box.fill("what time is it in Tokyo");
  await box.press("Enter");

  await expect(page.locator(".msg.me").last()).toHaveText("what time is it in Tokyo");
  await expect(box).toBeDisabled();
  await expect(page.locator(".trace li").first()).toContainText("Checked the time");
  await expect(page.locator(".msg.bot").last()).toContainText("It is 8:05 PM in Tokyo.");
  await expect(box).toBeEnabled();
  await expect(box).toHaveValue("");

  await page.screenshot({ path: test.info().outputPath("chat.png") });
  const width = await page.evaluate(() => document.documentElement.scrollWidth);
  expect(width).toBeLessThanOrEqual(400);
  expect(errors).toEqual([]);
});

test("droid view renders the droid", async ({ page }) => {
  await page.goto("/?mock=1#droid");
  await expect(page.getByRole("button", { name: /bleep/ })).toBeVisible();
  await expect(page.locator(".droid svg")).toHaveCount(1);
  await page.setViewportSize({ width: 132, height: 132 });
  await page.screenshot({ path: test.info().outputPath("droid.png") });
});

test("a failed send is shown and the text is kept", async ({ page }) => {
  await page.goto("/?mock=1#chat");
  const box = page.getByLabel("Message bleep");
  await expect(page.locator(".msg.me")).toHaveText(["hello"]);
  await box.fill("fail please");
  await box.press("Enter");
  await expect(page.locator(".msg.sys.err")).toContainText("could not reach Claude Code");
  await expect(box).toBeEnabled();
  await expect(box).toHaveValue("fail please");
});
