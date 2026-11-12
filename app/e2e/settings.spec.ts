import { expect, test, type Page } from "@playwright/test";

test.use({ viewport: { width: 460, height: 600 } });
const calls = (page: Page) => page.evaluate(() => (window as unknown as { __calls: string[] }).__calls);

test("with bleep's built-in app, Connect is one click and no setup shows", async ({ page }) => {
  await page.goto("/?mock=1&g=builtin#settings");
  await expect(page.getByRole("status")).toHaveText("Ready to connect");
  await expect(page.getByRole("button", { name: "Create a Google Cloud project" })).toBeHidden();
  await expect(page.getByLabel("Client ID")).toBeHidden();
  await page.getByRole("button", { name: "Connect Google" }).click();
  await expect(page.getByRole("status")).toHaveText("Connected as me@example.com");
});

test("your own app can go back to bleep's app", async ({ page }) => {
  await page.goto("/?mock=1&g=own#settings");
  await page.getByText("Use your own Google Cloud app").click();
  await expect(page.getByLabel("Client ID")).toHaveValue("cid.apps.googleusercontent.com");
  await page.getByRole("button", { name: "Use bleep's app" }).click();
  expect(await calls(page)).toContain("google_use_builtin");
  await expect(page.getByRole("button", { name: "Use bleep's app" })).toHaveCount(0);
});

test("Save is disabled until both fields have text", async ({ page }) => {
  await page.goto("/?mock=1&g=setup#settings");
  await expect(page.getByRole("status")).toHaveText("Not set up");
  const save = page.getByRole("button", { name: "Save" });
  await expect(save).toBeDisabled();
  await page.getByLabel("Client ID").fill("cid.apps.googleusercontent.com");
  await expect(save).toBeDisabled();
  await page.getByLabel("Client secret").fill("shh");
  await expect(save).toBeEnabled();
  await save.click();
  await expect(page.getByRole("status")).toHaveText("Ready to connect");
  await expect(page.getByLabel("Client secret")).toHaveValue("");
  expect(await calls(page)).toContain("google_save_client:cid.apps.googleusercontent.com");
});

test("Connect shows the waiting text, then Connected as me@example.com", async ({ page }) => {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await page.goto("/?mock=1&g=ready#settings");
  await page.getByRole("button", { name: "Connect Google" }).click();
  await expect(page.getByRole("button", { name: "Finish sign-in in your browser…" })).toBeDisabled();
  await expect(page.getByRole("status")).toHaveText("Connected as me@example.com");
  await expect(page.getByRole("button", { name: "Disconnect" })).toBeVisible();
  await page.screenshot({ path: test.info().outputPath("settings.png"), fullPage: true });
  expect(errors).toEqual([]);
});

test("a failed sign-in shows why", async ({ page }) => {
  await page.goto("/?mock=1&g=ready&fail=1#settings");
  await page.getByRole("button", { name: "Connect Google" }).click();
  await expect(page.getByRole("alert")).toHaveText("Sign-in was cancelled.");
  await expect(page.getByRole("button", { name: "Connect Google" })).toBeEnabled();
});

test("expired status shows Connect again", async ({ page }) => {
  await page.goto("/?mock=1&g=expired#settings");
  await expect(page.getByRole("status")).toHaveText("Sign-in expired. Connect again.");
  await expect(page.getByRole("button", { name: "Connect again" })).toBeVisible();
});

test("step links call open_link with fixed names", async ({ page }) => {
  await page.goto("/?mock=1&g=setup#settings");
  for (const name of ["Create a Google Cloud project", "Turn on the Calendar API", "Turn on the Gmail API", "Open the consent screen", "Create an OAuth client"]) {
    await page.getByRole("button", { name }).click();
  }
  expect(await calls(page)).toEqual(["open_link:console", "open_link:calendar_api", "open_link:gmail_api", "open_link:consent", "open_link:credentials"]);
});

test("no em dash on the page and no sideways scroll", async ({ page }) => {
  await page.goto("/?mock=1&g=setup#settings");
  await expect(page.getByRole("heading", { name: "Google" })).toBeVisible();
  expect(await page.evaluate(() => document.body.innerText)).not.toMatch(/—/);
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(460);
});
