import { expect, test, type Page } from "@playwright/test";

test.use({ viewport: { width: 860, height: 760 } });
const calls = (page: Page) => page.evaluate(() => (window as unknown as { __calls: string[] }).__calls);

async function open(page: Page, extra = "") {
  await page.goto(`/?mock=1&g=builtin${extra}#settings`);
  await expect(page.getByRole("tab", { name: "Model" })).toHaveAttribute("aria-selected", "true");
}

test("Model is the first setting and starts on Claude Code", async ({ page }) => {
  await open(page);
  await expect(page.getByRole("tab").first()).toHaveAccessibleName("Model");
  await expect(page.getByRole("radio", { name: /Claude Code/ })).toBeChecked();
  await expect(page.getByLabel("Main model (chat and check-ins)")).toHaveValue("sonnet");
  await page.getByLabel("Main model (chat and check-ins)").selectOption("opus");
  await page.getByRole("button", { name: "Save" }).click();
  await expect(page.getByText("Saved. bleep now runs on Claude Code.")).toBeVisible();
});

test("switch to an API key: pick a provider, paste a key, pick models, test, save", async ({ page }) => {
  await open(page);
  await page.getByRole("radio", { name: /API key/ }).check();
  await expect(page.getByLabel("Provider", { exact: true })).toHaveValue("openrouter");
  await expect(page.getByRole("button", { name: "Test" })).toBeDisabled();
  await page.getByRole("button", { name: "Save" }).click();
  await expect(page.getByRole("alert")).toHaveText("Pick a main model and a small model.");

  await page.getByLabel("API key", { exact: true }).fill("sk-or-test");
  await expect.poll(async () => (await calls(page)).includes("model_list:openrouter:sk-or-test")).toBe(true);
  await page.getByLabel("Main model (chat and check-ins)").fill("anthropic/claude-sonnet-4.5");
  await expect(page.getByText("Claude Sonnet 4.5. 1M context. $3 in, $15 out per million tokens")).toBeVisible();
  await page.getByLabel("Small model (mail triage and quick checks)").fill("google/gemini-2.5-flash");
  await expect(page.getByText("Gemini 2.5 Flash. 1M context. $0.3 in, $2.5 out per million tokens")).toBeVisible();

  await page.getByRole("button", { name: "Test" }).click();
  await expect(page.getByRole("status", { name: "Test result" })).toHaveText("OK. anthropic/claude-sonnet-4.5 answered in 0.8 s.");
  await page.getByRole("button", { name: "Save" }).click();
  await expect(page.getByText("Saved. bleep now runs on OpenRouter.")).toBeVisible();
  expect(await calls(page)).toContain("model_save:api:openrouter:anthropic/claude-sonnet-4.5:google/gemini-2.5-flash:key");
  await expect(page.getByLabel("API key", { exact: true })).toHaveValue("");
  await expect(page.getByLabel("API key", { exact: true })).toHaveAttribute("placeholder", "Saved in your Keychain");
});

test("a rejected key and a failed test say why", async ({ page }) => {
  await open(page);
  await page.getByRole("radio", { name: /API key/ }).check();
  await page.getByLabel("API key", { exact: true }).fill("bad");
  await expect(page.getByText(/Could not load models: The provider rejected the key/)).toBeVisible();
  await page.getByLabel("Main model (chat and check-ins)").fill("nope/model");
  await page.getByRole("button", { name: "Test" }).click();
  await expect(page.getByRole("status", { name: "Test result" })).toContainText("no such model");
});

test("a local provider needs no key and fills its base URL", async ({ page }) => {
  await open(page);
  await page.getByRole("radio", { name: /API key/ }).check();
  await page.getByLabel("Provider", { exact: true }).selectOption("ollama");
  await expect(page.getByLabel("API key", { exact: true })).toHaveCount(0);
  await expect(page.getByLabel("Base URL", { exact: true })).toHaveValue("http://localhost:11434/v1");
  await expect.poll(async () => (await calls(page)).some((c) => c.startsWith("model_list:ollama"))).toBe(true);
  await page.getByLabel("Main model (chat and check-ins)").fill("llama3.1:8b");
  await page.getByLabel("Small model (mail triage and quick checks)").fill("llama3.1:8b");
  await expect(page.getByText("Free").first()).toBeVisible();
  await page.getByRole("button", { name: "Save" }).click();
  await expect(page.getByText("Saved. bleep now runs on Ollama (on this Mac).")).toBeVisible();
});

test("with a saved key: spend shows here and in Activity, and the key can be forgotten", async ({ page }) => {
  await open(page, "&model=1");
  await expect(page.getByRole("radio", { name: /API key/ })).toBeChecked();
  await expect(page.getByRole("status", { name: "Spend" })).toHaveText("Today: $0.42 of $1.00.");
  await page.getByRole("button", { name: "Forget the saved key" }).click();
  await expect(page.getByLabel("API key", { exact: true })).toHaveAttribute("placeholder", "Paste your key");
  expect(await calls(page)).toContain("model_forget_key:openrouter");
  await page.getByRole("tab", { name: "Activity" }).click();
  await expect(page.getByRole("status", { name: "Spend" })).toHaveText("Today: $0.42 of $1.00.");
});

test("the Model page fits a narrow window", async ({ page }) => {
  await page.setViewportSize({ width: 380, height: 700 });
  await open(page, "&model=1");
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(380);
});
