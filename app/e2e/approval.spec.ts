import { expect, test, type Page } from "@playwright/test";

async function askForRent(page: Page) {
  await page.goto("/?mock=1#chat");
  const box = page.getByLabel("Message bleep");
  await box.fill("add a reminder to pay rent at 6 PM");
  await box.press("Enter");
  const card = page.getByRole("region", { name: "Add to your calendar?" });
  await expect(card).toBeVisible();
  await expect(card).toContainText("Pay rent");
  await expect(card).toContainText("When");
  await expect(page.locator(".trace li").last()).toContainText("Adding to your calendar");
  return card;
}

test("rent reminder: card appears, Approve adds it, buttons disappear", async ({ page }) => {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  const card = await askForRent(page);
  await expect(page.getByLabel("Message bleep")).toBeDisabled();
  await card.getByRole("button", { name: "Approve" }).click();
  await expect(card.getByRole("status")).toHaveText("Added");
  await expect(card.getByRole("button")).toHaveCount(0);
  await expect(page.locator(".msg.bot").last()).toContainText("Done. Pay rent is on your calendar at 6 PM.");
  await expect(page.getByLabel("Message bleep")).toBeEnabled();
  await page.screenshot({ path: test.info().outputPath("approval.png") });
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(400);
  expect(errors).toEqual([]);
});

test("Decline shows Declined and the reply says nothing changed", async ({ page }) => {
  const card = await askForRent(page);
  await card.getByRole("button", { name: "Decline" }).click();
  await expect(card.getByRole("status")).toHaveText("Declined");
  await expect(page.locator(".msg.bot").last()).toContainText("I did not add it");
});

test("Approve twice sends one decide call", async ({ page }) => {
  const card = await askForRent(page);
  // Two clicks in the same tick: React has no chance to disable the button between them.
  await card.getByRole("button", { name: "Approve" }).evaluate((b: HTMLButtonElement) => {
    b.click();
    b.click();
  });
  await expect(card.getByRole("status")).toHaveText("Added");
  expect(await page.evaluate(() => (window as unknown as { __decideCalls: number }).__decideCalls)).toBe(1);
});
