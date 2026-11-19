import { expect, test, type Page } from "@playwright/test";

const calls = (page: Page) => page.evaluate(() => (window as unknown as { __calls: string[] }).__calls);

test("stop ends a streaming reply and keeps what came", async ({ page }) => {
  await page.goto("/?mock=1#chat");
  const box = page.getByLabel("Message bleep");
  await box.fill("slow please");
  await box.press("Enter");
  await expect(page.locator(".steps-sum")).toContainText("Checked your calendar...");
  await expect(page.locator(".msg.bot").last()).toContainText("Let me look at your");
  await page.getByRole("button", { name: "Stop" }).click();
  await expect(page.locator(".msg.bot").last().locator(".note")).toHaveText("Stopped");
  await expect(page.getByRole("button", { name: "Send" })).toBeVisible();
  await expect(box).toBeEnabled();
  expect(await calls(page)).toContain("chat_stop");
});

test("tool steps fold into one line that opens", async ({ page }) => {
  await page.goto("/?mock=1#chat");
  const box = page.getByLabel("Message bleep");
  await box.fill("what time is it in Tokyo");
  await box.press("Enter");
  await expect(page.locator(".msg.bot").last()).toContainText("It is 8:05 PM in Tokyo.");
  const sum = page.locator(".steps-sum").last();
  await expect(sum).toHaveText("Checked the time");
  await expect(page.locator(".trace").last()).toBeHidden();
  await sum.click();
  await expect(sum).toHaveAttribute("aria-expanded", "true");
  await expect(page.locator(".trace li").last()).toBeVisible();
});

test("mic button listens with voice on, and opens Settings with voice off", async ({ page }) => {
  await page.goto("/?mock=1&voice=on#chat");
  const box = page.getByLabel("Message bleep");
  await expect(page.locator(".msg.me")).toHaveText(["hello"]);
  await page.getByRole("button", { name: "Talk to bleep" }).click();
  await expect(box).toHaveAttribute("placeholder", "Listening...");
  await page.getByRole("button", { name: "Stop listening" }).click();
  await expect(box).toHaveAttribute("placeholder", "Ask bleep anything about your day");
  expect(await calls(page)).toEqual(expect.arrayContaining(["voice_listen:true", "voice_listen:false"]));

  await page.goto("/?mock=1#chat");
  await expect(page.locator(".msg.me")).toHaveText(["hello"]);
  await page.getByRole("button", { name: "Talk to bleep" }).click();
  await expect.poll(() => calls(page)).toContain("settings_open:voice");
});

test("reply actions read aloud and ask again", async ({ page }) => {
  await page.goto("/?mock=1#chat");
  const bot = page.locator(".msg.bot").first();
  await expect(bot).toHaveText("Hi! I am bleep.");
  await bot.hover();
  await page.getByRole("button", { name: "Read aloud" }).first().click();
  await expect.poll(() => calls(page)).toContain("voice_speak:Hi! I am bleep.");
  await page.getByRole("button", { name: "Ask again" }).first().click();
  await expect(page.locator(".msg.me")).toHaveText(["hello", "hello"]);
});

test("jump to latest shows when scrolled up", async ({ page }) => {
  await page.goto("/?mock=1&long=1#chat");
  await expect(page.locator(".msg.me").last()).toHaveText("question 14");
  const jump = page.getByRole("button", { name: "Jump to latest" });
  await expect(jump).toHaveCount(0);
  await page.locator(".log").evaluate((el) => el.scrollTo({ top: 200 }));
  await expect(jump).toBeVisible();
  await jump.click();
  await expect(jump).toHaveCount(0);
});

test("an empty chat greets and suggests from connected tools", async ({ page }) => {
  await page.goto("/?mock=1&fresh=1#chat");
  await expect(page.locator(".hello h2")).toHaveText(/^(Good (morning|afternoon|evening)|Up late\?)$/);
  await expect(page.locator(".hello p")).toHaveText("Calendar, Gmail and Reminders are on.");
  const first = page.locator(".sugs button").first();
  await expect(first).toContainText("What's left on my calendar today?");
  await first.click();
  await expect(page.locator(".msg.me")).toHaveText(["What's left on my calendar today?"]);
  await expect(page.locator(".hello")).toHaveCount(0);
  await page.screenshot({ path: test.info().outputPath("chat-empty.png") });
});
