import { expect, test, type Page } from "@playwright/test";

const calls = (page: Page) => page.evaluate(() => (window as unknown as { __calls: string[] }).__calls);
const voice = (page: Page, e: unknown) => page.evaluate((ev) => (window as unknown as { __voice: (e: unknown) => void }).__voice(ev), e);

test("voice settings: turn on, change phrase, mic, speaker and voice", async ({ page }) => {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await page.goto("/?mock=1&g=builtin#settings");
  await page.getByRole("tab", { name: "Voice" }).click();
  await expect(page.getByRole("heading", { name: "Wake word" })).toBeVisible();
  await expect(page.locator("#wake-h + .pill")).toHaveText("Off");

  await page.getByRole("switch", { name: "Listen for the wake phrase" }).click();
  await expect(page.locator("#wake-h + .pill")).toHaveText("Listening");
  await expect(page.getByText("Using MacBook Air Microphone.")).toBeVisible();
  await expect(page.getByRole("meter", { name: "Mic level" })).toBeVisible();

  // Auto is first and names what macOS uses now.
  const mic = page.getByLabel("Microphone");
  await expect(mic.locator("option").first()).toHaveText("Auto (MacBook Air Microphone)");
  await mic.selectOption("AirPods Pro");

  const phrase = page.getByLabel("Wake phrase", { exact: true });
  await phrase.fill("hi");
  await page.getByRole("button", { name: "Save", exact: true }).click();
  await expect(page.getByRole("alert")).toHaveText("Use a phrase with at least 4 letters.");
  await phrase.fill("Okay, Jarvis!");
  await page.getByRole("button", { name: "Save", exact: true }).click();
  await expect(phrase).toHaveValue("okay jarvis");

  await page.getByLabel("Sensitivity").selectOption("loose");
  await page.getByLabel("Speaker").selectOption("AirPods Pro");
  await page.locator("#tts-voice").selectOption("Samantha");
  await page.getByRole("button", { name: "Play a sample" }).click();
  await page.getByRole("switch", { name: "Read replies aloud" }).click();
  await expect(page.getByText("Replies show in the chat only.")).toBeVisible();

  expect(await calls(page)).toEqual(expect.arrayContaining([
    "voice_set:true:hey bleep::normal:true::",
    "voice_set:true:hey bleep:AirPods Pro:normal:true::",
    "voice_set:true:okay jarvis:AirPods Pro:normal:true::",
    "voice_set:true:okay jarvis:AirPods Pro:loose:true::",
    "voice_test_speaker:Samantha:AirPods Pro",
    "voice_set:true:okay jarvis:AirPods Pro:loose:false:AirPods Pro:Samantha",
  ]));
  expect(errors).toEqual([]);
});

test("wake phrase opens listening, the request is sent, and the reply is read aloud", async ({ page }) => {
  await page.goto("/?mock=1#chat");
  const box = page.getByLabel("Message bleep");
  await expect(box).toHaveAttribute("placeholder", "Ask bleep anything about your day");
  await voice(page, { kind: "wake" });
  await expect(box).toHaveAttribute("placeholder", "Listening...");
  await voice(page, { kind: "heard", text: "what time is it in Tokyo" });
  await expect(page.locator(".msg.me").last()).toHaveText("what time is it in Tokyo");
  await expect(page.locator(".msg.bot").last()).toContainText("It is 8:05 PM in Tokyo.");
  await expect(box).toHaveAttribute("placeholder", "Ask bleep anything about your day");
  await expect.poll(() => calls(page)).toContain("voice_speak:It is 8:05 PM in Tokyo.");
});

test("typed questions are not read aloud, and giving up clears listening", async ({ page }) => {
  await page.goto("/?mock=1#chat");
  const box = page.getByLabel("Message bleep");
  await expect(box).toHaveAttribute("placeholder", "Ask bleep anything about your day");
  await voice(page, { kind: "wake" });
  await expect(box).toHaveAttribute("placeholder", "Listening...");
  await voice(page, { kind: "idle" });
  await expect(box).toHaveAttribute("placeholder", "Ask bleep anything about your day");
  await box.fill("what time is it in Tokyo");
  await box.press("Enter");
  await expect(page.locator(".msg.bot").last()).toContainText("It is 8:05 PM in Tokyo.");
  await page.waitForTimeout(300);
  expect((await calls(page)).filter((c) => c.startsWith("voice_speak"))).toEqual([]);
});
