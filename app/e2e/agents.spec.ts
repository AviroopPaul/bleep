import { expect, test, type Page } from "@playwright/test";

test.use({ viewport: { width: 380, height: 600 } });
const calls = (page: Page) => page.evaluate(() => (window as unknown as { __calls: string[] }).__calls);

test("agents view groups sessions and shows the terminal note for Claude", async ({ page }) => {
  await page.goto("/?mock=1&agents=1#chat");
  await page.getByRole("tab", { name: /Agents/ }).click();
  const view = page.getByRole("region", { name: "Coding agents" });
  for (const h of ["Needs you", "Running", "Done", "Handled"]) await expect(view.getByRole("heading", { name: h })).toBeVisible();
  const claude = view.getByRole("listitem").filter({ hasText: "git push --force" });
  await expect(claude).toContainText("shop");
  await expect(claude).toContainText("Answer in the terminal");
  await expect(claude.getByRole("button")).toHaveCount(0);
  await expect(view).toContainText('Approved by rule "npm test"');
  await expect(page.getByLabel("Message bleep")).toBeHidden();
  await page.getByRole("tab", { name: "Chat" }).click();
  await expect(page.getByLabel("Message bleep")).toBeVisible();
});

test("Codex ask Approve calls agents_decide", async ({ page }) => {
  await page.goto("/?mock=1&agents=1#chat");
  await page.getByRole("tab", { name: /Agents/ }).click();
  const codex = page.getByRole("listitem").filter({ hasText: "rm -rf dist" });
  await codex.getByRole("button", { name: "Approve" }).click();
  expect(await calls(page)).toContain("agents_decide:21:true");
  await expect(page.getByRole("region", { name: "Coding agents" }).getByRole("listitem").filter({ hasText: "rm -rf dist" }).first()).toContainText("Approved by you");
});

test("badge counts waiting sessions and the header still fits", async ({ page }) => {
  await page.goto("/?mock=1&agents=1#chat");
  await expect(page.getByRole("tab", { name: "Agents, 2 need you" })).toBeVisible();
  const fits = await page.locator(".panel-head").evaluate((el) => el.scrollWidth <= el.clientWidth);
  expect(fits).toBe(true);
  await expect(page.locator(".panel-head .badge")).toBeVisible();
});

test("no agents shows the empty state", async ({ page }) => {
  await page.goto("/?mock=1#chat");
  await page.getByRole("tab", { name: "Agents" }).click();
  await expect(page.getByText("No coding agents running.")).toBeVisible();
});
