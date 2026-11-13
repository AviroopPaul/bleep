import { expect, test, type Page } from "@playwright/test";

test.use({ viewport: { width: 520, height: 640 } });
const calls = (page: Page) => page.evaluate(() => (window as unknown as { __calls: string[] }).__calls);

test("a fresh install walks Welcome, Claude, Google, Agents, Done and finishes", async ({ page }) => {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await page.goto("/?mock=1&g=builtin#setup");
  await expect(page.getByRole("heading", { name: "Hi, I'm bleep." })).toBeVisible();
  await page.getByRole("button", { name: "Let's set up" }).click();

  await expect(page.getByRole("heading", { name: "Claude Code" })).toBeVisible();
  await expect(page.getByRole("list", { name: "Claude Code status" })).toContainText("Signed in (pro)");
  await page.getByRole("button", { name: "Next" }).click();

  await expect(page.getByRole("heading", { name: "Gmail and Calendar" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Skip for now" })).toBeVisible();
  await page.getByRole("button", { name: "Connect Google" }).click();
  await expect(page.getByRole("button", { name: "Next" })).toBeVisible();
  await page.getByRole("button", { name: "Next" }).click();

  await expect(page.getByRole("heading", { name: "Your coding agents" })).toBeVisible();
  await page.getByRole("button", { name: "Install hooks" }).click();
  await page.getByRole("button", { name: "Yes, install" }).click();
  await page.getByRole("button", { name: "Next" }).click();

  const done = page.getByRole("list", { name: "What is set up" });
  await expect(page.getByRole("heading", { name: "All set." })).toBeVisible();
  await expect(done).toContainText("Google connected as me@example.com");
  await expect(done.locator(".su-check.ok")).toHaveCount(3);
  await page.getByRole("button", { name: "Start using bleep" }).click();
  expect(await calls(page)).toEqual(expect.arrayContaining(["agents_install_claude", "setup_finish"]));
  expect(errors).toEqual([]);
});

test("without Claude Code it links to the install guide and lets you skip", async ({ page }) => {
  await page.goto("/?mock=1&health=none#setup");
  await page.getByRole("button", { name: "Let's set up" }).click();
  await page.getByRole("button", { name: "How to install Claude Code" }).click();
  expect(await calls(page)).toContain("open_url:https://code.claude.com/docs/en/setup");
  await expect(page.getByRole("button", { name: "Skip for now" })).toBeVisible();
});

test("installed but not signed in offers bleep's own Claude sign-in", async ({ page }) => {
  await page.goto("/?mock=1&health=signin#setup");
  await page.getByRole("button", { name: "Let's set up" }).click();
  await page.getByRole("button", { name: "Sign in to Claude" }).click();
  expect(await calls(page)).toContain("open_claude_login");
  await page.getByRole("button", { name: "Back" }).click();
  await expect(page.getByRole("heading", { name: "Hi, I'm bleep." })).toBeVisible();
});
