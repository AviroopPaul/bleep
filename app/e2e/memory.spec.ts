import { expect, test, type Page } from "@playwright/test";

test.use({ viewport: { width: 380, height: 640 } });
const calls = (page: Page) => page.evaluate(() => (window as unknown as { __calls: string[] }).__calls);

test("memory tab lists facts and Forget removes one", async ({ page }) => {
  await page.goto("/?mock=1&g=builtin&memory=1#settings");
  await page.getByRole("tab", { name: "Memory" }).click();
  const learned = page.getByRole("group", { name: "Learned" });
  await expect(learned).toContainText("Sync means my 1:1 with Priya");
  await expect(page.getByRole("group", { name: "Your rules" })).toContainText("Nothing yet.");
  await learned.getByRole("button", { name: "Forget: Standup is at 10:00" }).click();
  await expect(learned).not.toContainText("Standup is at 10:00");
  expect(await calls(page)).toContain("memory_forget:MEMORY.md:Standup is at 10:00");
  await expect(page.getByText("1% of the memory budget used")).toBeVisible();
});

test("a suggestion is added only on a tap", async ({ page }) => {
  await page.goto("/?mock=1&g=builtin&memory=1#settings");
  await page.getByRole("tab", { name: "Memory" }).click();
  await expect(page.getByText("From your day")).toBeVisible();
  await page.getByRole("button", { name: "Add: You pay bills the evening they are due" }).click();
  await expect(page.getByRole("group", { name: "Learned" })).toContainText("You pay bills the evening they are due");
  await page.getByRole("button", { name: "Dismiss: Priya leads the design team" }).click();
  await expect(page.getByText("No suggestions right now.")).toBeVisible();
  expect(await calls(page)).toEqual(expect.arrayContaining(["memory_decide:1:true", "memory_decide:2:false"]));
});

test("compaction slider, note and Compact now", async ({ page }) => {
  await page.goto("/?mock=1&g=builtin&memory=1#settings");
  await page.getByRole("tab", { name: "Memory" }).click();
  await expect(page.getByText("Your plan gives 1M. bleep compacts at 250K.")).toBeVisible();
  await page.getByRole("slider").fill("200000");
  await expect(page.getByText("Compact at 200K")).toBeVisible();
  expect(await calls(page)).toContain("memory_set:200000");
  await page.getByRole("button", { name: "Compact now" }).click();
  await expect(page.getByText("Compacted. The chat goes on from the summary.")).toBeVisible();
});

test("chat shows the divider and the context meter, and the header fits", async ({ page }) => {
  await page.goto("/?mock=1&memory=1&agents=1#chat");
  await expect(page.getByRole("separator", { name: "Compacted 250K into a 3K summary. Facts saved to memory." })).toBeVisible();
  const meter = page.getByRole("meter", { name: "Chat context" });
  await expect(meter).toHaveAttribute("aria-valuetext", "120K of 250K");
  const fits = await page.locator(".panel-head").evaluate((el) => el.scrollWidth <= el.clientWidth);
  expect(fits).toBe(true);
  await page.getByLabel("Message bleep").fill("time in Tokyo?");
  await page.getByRole("button", { name: "Send" }).click();
  await expect(meter).toHaveAttribute("aria-valuetext", "215K of 250K");
  await expect(meter).toHaveClass(/high/);
});

test("compacting disables send and reloads the chat", async ({ page }) => {
  await page.goto("/?mock=1&memory=1#chat");
  await expect(page.getByLabel("Message bleep")).toBeEnabled();
  await page.evaluate(() => (window as unknown as { __TAURI_INTERNALS__: { invoke: (c: string) => Promise<unknown> } }).__TAURI_INTERNALS__.invoke("memory_compact_now"));
  await expect(page.getByLabel("Message bleep")).toBeDisabled();
  await expect(page.getByText("Compacting... Facts go to memory first.")).toBeVisible();
  await expect(page.getByLabel("Message bleep")).toBeEnabled();
});

test("remember card shows the exact fact", async ({ page }) => {
  await page.goto("/?mock=1#chat");
  await page.getByLabel("Message bleep").fill("remember: sync means my 1:1 with Priya");
  await page.getByRole("button", { name: "Send" }).click();
  await expect(page.getByText("Remember this?")).toBeVisible();
  await expect(page.getByText("Sync means my 1:1 with Priya", { exact: true })).toBeVisible();
  await expect(page.getByText("MEMORY.md")).toBeVisible();
});
