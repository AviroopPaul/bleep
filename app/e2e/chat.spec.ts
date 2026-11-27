import { expect, test } from "@playwright/test";

test("chat shows history, streams a tool step and a reply, and locks input while streaming", async ({ page }) => {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await page.goto("/?mock=1#chat");

  await expect(page.locator(".msg.me")).toHaveText(["hello"]);
  await expect(page.locator(".msg.bot")).toHaveText(["Hi! I am bleep."]);
  await expect(page.getByText("Claude Code")).toHaveCount(0);
  await expect(page.getByLabel("Message bleep")).toHaveAttribute("placeholder", "Ask bleep anything about your day");
  await expect(page.getByRole("button", { name: "Send" })).toBeDisabled();

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

test("droid view renders the buddy", async ({ page }) => {
  await page.goto("/?mock=1#droid");
  await expect(page.getByRole("button", { name: /bleep/ })).toBeVisible();
  await expect(page.locator(".droid-stage .av svg")).toHaveCount(1);
  await page.setViewportSize({ width: 132, height: 132 });
  await page.screenshot({ path: test.info().outputPath("droid.png") });
});

test("a failed send is shown and the text is kept", async ({ page }) => {
  await page.goto("/?mock=1#chat");
  const box = page.getByLabel("Message bleep");
  await expect(page.locator(".msg.me")).toHaveText(["hello"]);
  await box.fill("fail please");
  await box.press("Enter");
  await expect(page.locator(".msg.sys.err")).toHaveText("bleep could not start the reply. Send again to continue.");
  await expect(box).toBeEnabled();
  await expect(box).toHaveValue("fail please");
});

test("an error mid-reply shows once, apart from the reply", async ({ page }) => {
  await page.goto("/?mock=1#chat");
  const box = page.getByLabel("Message bleep");
  await box.fill("break it");
  await box.press("Enter");
  const bot = page.locator(".msg.bot").last();
  await expect(bot.locator(".msg-err")).toHaveText("Claude Code stopped responding. Send again to continue.");
  await expect(bot.locator(".text")).toHaveText("Here is half");
  await expect(page.locator(".msg-err")).toHaveCount(1);
  await expect(page.locator(".msg.sys.err")).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Ask again" }).last()).toBeVisible();
});

test("a droid bubble shows in the chat after it is gone", async ({ page }) => {
  await page.goto("/?mock=1&pinged=1#chat");
  await expect(page.locator(".msg.ping")).toContainText("Design review in 1 hour");
});

test("the enlarge button toggles to shrink", async ({ page }) => {
  await page.goto("/?mock=1#chat");
  const grow = page.getByRole("button", { name: "Enlarge chat" });
  await expect(grow).toHaveAttribute("aria-pressed", "false");
  await grow.click();
  const shrink = page.getByRole("button", { name: "Shrink chat" });
  await expect(shrink).toHaveAttribute("aria-pressed", "true");
  await shrink.click();
  await expect(page.getByRole("button", { name: "Enlarge chat" })).toBeVisible();
});
