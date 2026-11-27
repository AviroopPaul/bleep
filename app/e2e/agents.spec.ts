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
  await expect(claude.getByRole("button")).toHaveCount(1); // the row itself opens the chat
  await expect(view).toContainText('Approved by rule "npm test"');
  await expect(page.getByLabel("Message bleep")).toBeHidden();
  await page.getByRole("tab", { name: "Chat" }).click();
  await expect(page.getByLabel("Message bleep")).toBeVisible();
});

test("Codex ask Approve calls agents_decide", async ({ page }) => {
  await page.goto("/?mock=1&agents=1#chat");
  await page.getByRole("tab", { name: /Agents/ }).click();
  await page.getByRole("button", { name: "Open api" }).click();
  await page.getByRole("button", { name: "Approve" }).click();
  await page.getByRole("button", { name: "Back to agents" }).click();
  expect(await calls(page)).toContain("agents_decide:21:true");
  await expect(page.getByRole("region", { name: "Coding agents" }).getByRole("listitem").filter({ hasText: "rm -rf dist" }).first()).toContainText("Approved by you");
});

test("badge counts waiting sessions and the header still fits", async ({ page }) => {
  await page.goto("/?mock=1&agents=1#chat");
  await expect(page.getByRole("tab", { name: "Agents, 2 need you" })).toBeVisible();
  const fits = await page.locator(".panel-head").evaluate((el) => el.scrollWidth <= el.clientWidth);
  expect(fits).toBe(true);
  await expect(page.locator(".panel-head .badge")).toHaveCount(0);
});

test("no agents shows the empty state", async ({ page }) => {
  await page.goto("/?mock=1#chat");
  await page.getByRole("tab", { name: "Agents" }).click();
  await expect(page.getByText("No coding agents running.")).toBeVisible();
});

const openAgents = async (page: Page) => {
  await page.goto("/?mock=1&agents=1&answers=1#chat");
  await page.getByRole("tab", { name: /Agents/ }).click();
};

test("a waiting session opens to its chat and the question", async ({ page }) => {
  await openAgents(page);
  const row = page.getByRole("listitem").filter({ hasText: "landing" });
  await expect(row).toContainText("Tap to answer");
  await page.getByRole("button", { name: "Open landing" }).click();
  const chat = page.getByRole("region", { name: "landing chat" });
  await expect(chat).toContainText("Add a waitlist form to the landing page.");
  await expect(chat).toContainText("Read: page.tsx");
  await expect(chat.getByRole("group", { name: /Which database/ })).toBeVisible();
  const bot = chat.locator(".msg.bot").filter({ hasText: "Next.js" });
  await expect(bot.locator("strong")).toHaveText("Next.js");
  await expect(bot.locator("li")).toHaveCount(2);
  await expect(bot.locator("code").first()).toHaveText("<input type=email>");
  await expect(bot.locator("b")).toHaveCount(0);
  await expect(bot).toContainText("<b>raw html stays text</b>");
  await page.getByRole("button", { name: "Back to agents" }).click();
  await expect(page.getByRole("region", { name: "Coding agents" })).toBeVisible();
});

test("picking options answers an AskUserQuestion", async ({ page }) => {
  await openAgents(page);
  await page.getByRole("button", { name: "Open landing" }).click();
  const send = page.getByRole("button", { name: "Send answer" });
  await expect(send).toBeDisabled();
  await page.getByRole("button", { name: /Neon Postgres/ }).click();
  await page.getByRole("button", { name: /^Email/ }).click();
  await page.getByRole("button", { name: /^Name/ }).click();
  await expect(page.getByRole("button", { name: /^Name/ })).toHaveAttribute("aria-pressed", "true");
  await send.click();
  expect(await calls(page)).toContain('agents_choose:22:{"Which database should the waitlist use?":"Neon Postgres","What should the form collect?":"Email, Name"}');
  await expect(page.getByRole("status")).toContainText("Sent. Claude goes on.");
  await expect(page.getByRole("region", { name: "landing chat" })).toContainText("running");
});

test("a reply goes to the stopped session", async ({ page }) => {
  await openAgents(page);
  await page.getByRole("button", { name: "Open bleep" }).click();
  await expect(page.getByRole("region", { name: "Reply" })).toContainText("Should I also update the README?");
  const box = page.getByLabel("Reply to Claude");
  await box.fill("Yes, and add a line to the changelog.");
  await box.press("Enter");
  expect(await calls(page)).toContain("agents_reply:claude:f:Yes, and add a line to the changelog.");
  await expect(page.getByRole("status")).toContainText("Sent. Claude goes on.");
  await expect(page.getByLabel("Reply to Claude")).toBeHidden();
});

test("a held Claude permission can be approved from bleep", async ({ page }) => {
  await openAgents(page);
  await page.getByRole("button", { name: "Open shop" }).click();
  const card = page.getByRole("region", { name: "Permission ask" });
  await expect(card).toContainText("git push --force origin main");
  await expect(card).toContainText('matches "git push"');
  await card.getByRole("button", { name: "Deny" }).click();
  expect(await calls(page)).toContain("agents_decide:20:false");
});

test("a long reply fits the narrow panel", async ({ page }) => {
  await page.setViewportSize({ width: 410, height: 610 });
  await page.goto("/?mock=1&answers=1&long=1#chat");
  await page.getByRole("tab", { name: /Agents/ }).click();
  await page.getByRole("button", { name: "Open bleep" }).click();
  const send = page.getByRole("button", { name: "Send" });
  const box = await send.boundingBox();
  expect(box && box.x + box.width <= 410 && box.width === box.height).toBeTruthy();
  const fit = await page.evaluate(() => {
    const pre = document.querySelector(".ag-log .md pre") as HTMLElement;
    // A one-word header stays on one line: words never break inside a cell.
    const ths = [...document.querySelectorAll(".ag-log .md th")] as HTMLElement[];
    return { pre: pre.scrollWidth <= pre.clientWidth, page: document.documentElement.scrollWidth <= 410, cells: ths.filter((t) => t.textContent === "Part").every((t) => {
      const r = document.createRange();
      r.selectNodeContents(t);
      return r.getClientRects().length === 1;
    }) };
  });
  expect(fit).toEqual({ pre: true, page: true, cells: true });
});
