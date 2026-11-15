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

test("tabs work with the keyboard", async ({ page }) => {
  await page.goto("/?mock=1&g=builtin#settings");
  const google = page.getByRole("tab", { name: "Google" });
  await expect(google).toHaveAttribute("aria-selected", "true");
  await google.focus();
  await page.keyboard.press("ArrowRight");
  await expect(page.getByRole("tab", { name: "Mail" })).toHaveAttribute("aria-selected", "true");
  await expect(page.getByRole("tabpanel")).toContainText("VIPs");
  await page.keyboard.press("ArrowRight");
  await expect(page.getByRole("tab", { name: "Memory" })).toHaveAttribute("aria-selected", "true");
  await page.keyboard.press("ArrowRight");
  await expect(page.getByRole("tab", { name: "Connectors" })).toHaveAttribute("aria-selected", "true");
  await page.keyboard.press("ArrowRight");
  await expect(page.getByRole("tab", { name: "Agents" })).toHaveAttribute("aria-selected", "true");
  await page.keyboard.press("ArrowRight");
  await expect(page.getByRole("tab", { name: "Activity" })).toHaveAttribute("aria-selected", "true");
  await page.keyboard.press("ArrowRight");
  await expect(google).toHaveAttribute("aria-selected", "true");
});

test("mail tab adds a VIP and accepts a mute suggestion", async ({ page }) => {
  await page.goto("/?mock=1&g=builtin#settings");
  await page.getByRole("tab", { name: "Mail" }).click();
  await page.getByLabel("Add a VIP").fill("Boss@Example.com");
  await page.getByRole("button", { name: "Add VIP" }).click();
  await expect(page.getByRole("list", { name: "VIPs" })).toContainText("boss@example.com");
  await expect(page.getByRole("list", { name: "Muted senders" })).toContainText("noisy@x.example");
  await page.getByRole("button", { name: "Remove noisy@x.example" }).click();
  await expect(page.getByRole("list", { name: "Muted senders" })).not.toContainText("noisy@x.example");
  await expect(page.getByText("bills@city.example")).toBeVisible();
  await page.getByRole("button", { name: "Mute bills@city.example" }).click();
  await expect(page.getByRole("list", { name: "Muted senders" })).toContainText("bills@city.example");
  const c = await calls(page);
  expect(c).toContain("mail_set_lists:boss@example.com|noisy@x.example");
  expect(c).toContain("mail_set_lists:boss@example.com|");
  expect(c).toContain("mail_resolve_suggestion:bills@city.example:true");
  await page.screenshot({ path: test.info().outputPath("mail-tab.png") });
});

test("activity tab lists newest first and marks failures", async ({ page }) => {
  await page.goto("/?mock=1&g=builtin#settings");
  await page.getByRole("tab", { name: "Activity" }).click();
  const rows = page.getByRole("list", { name: "Activity" }).getByRole("listitem");
  await expect(rows).toHaveCount(3);
  await expect(rows.nth(0)).toContainText("Sorted mail (small model)");
  await expect(rows.nth(0)).toContainText("failed");
  await expect(rows.nth(1)).toContainText("Declined: add to your calendar");
  await expect(rows.nth(2)).toContainText("Checked your calendar");
  await expect(rows.nth(2)).not.toContainText("failed");
  await page.screenshot({ path: test.info().outputPath("activity-tab.png") });
});

test("agents tab installs Claude Code hooks only after consent", async ({ page }) => {
  await page.goto("/?mock=1&g=builtin#settings");
  await page.getByRole("tab", { name: "Agents" }).click();
  const claude = page.getByRole("region", { name: "Claude Code" });
  await expect(claude.getByRole("status")).toHaveText("Not installed");
  await claude.getByRole("button", { name: "Install hooks" }).click();
  await expect(claude).toContainText("A backup is saved first.");
  expect(await calls(page)).not.toContain("agents_install_claude");
  await claude.getByRole("button", { name: "Yes, install" }).click();
  await expect(claude.getByRole("status")).toHaveText("Installed");
  expect(await calls(page)).toContain("agents_install_claude");
  await claude.getByRole("button", { name: "Remove hooks" }).click();
  await expect(claude.getByRole("status")).toHaveText("Not installed");
  expect(await calls(page)).toContain("agents_remove_claude");
});

test("agents tab toggles call agents_set", async ({ page }) => {
  await page.goto("/?mock=1&g=builtin#settings");
  await page.getByRole("tab", { name: "Agents" }).click();
  const auto = page.getByRole("switch", { name: "Approve safe asks by my rules" });
  await expect(auto).not.toBeChecked();
  await expect(page.getByRole("switch", { name: "Check screenshots with a small model" })).toBeChecked();
  await auto.click();
  await expect(auto).toBeChecked();
  expect(await calls(page)).toContain("agents_set:true:true:false");
  await page.getByRole("button", { name: "Show rules file" }).click();
  expect(await calls(page)).toContain("agents_reveal_rules");
});
