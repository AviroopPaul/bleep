import { expect, test, type Page } from "@playwright/test";

const calls = (page: Page) => page.evaluate(() => (window as unknown as { __calls: string[] }).__calls);

test("reminders tab lists due and upcoming, with Done, Snooze and Delete", async ({ page }) => {
  await page.goto("/?mock=1&rem=1#chat");
  const tab = page.getByRole("tab", { name: "Reminders, 1 due" });
  await expect(tab).toBeVisible();
  await tab.click();
  const panel = page.getByRole("tabpanel", { name: "Reminders" });
  const due = panel.getByRole("region", { name: "Due now" });
  await expect(due).toContainText("Pay the electricity bill");
  const up = panel.getByRole("region", { name: "Upcoming" });
  await expect(up).toContainText("Call mom");
  await expect(up).toContainText("Weekdays");
  await due.getByRole("button", { name: "1 hour" }).click();
  await expect.poll(() => calls(page)).toContain("reminder_snooze:1:60");
  await expect(page.getByRole("tab", { name: "Reminders", exact: true })).toBeVisible();
  await panel.getByRole("button", { name: "Delete Call mom" }).click();
  await expect.poll(() => calls(page)).toContain("reminder_delete:2");
  await expect(panel).not.toContainText("Call mom");
});

test("quick add makes a reminder", async ({ page }) => {
  await page.goto("/?mock=1#chat");
  await page.getByRole("tab", { name: "Reminders" }).click();
  const panel = page.getByRole("tabpanel", { name: "Reminders" });
  await expect(panel.getByText("No reminders yet.")).toBeVisible();
  await expect(panel.getByRole("button", { name: "Add" })).toBeDisabled();
  await panel.getByLabel("Remind me to").fill("Water the plants");
  await panel.getByLabel("When").fill("2030-01-02T18:30");
  await panel.getByLabel("Repeat").selectOption("weekly");
  await panel.getByRole("button", { name: "Add" }).click();
  await expect.poll(() => calls(page)).toContain("reminder_add:Water the plants:2030-01-02T18:30:weekly");
  await expect(panel.getByRole("region", { name: "Upcoming" })).toContainText("Water the plants");
  await expect(panel.getByLabel("Remind me to")).toHaveValue("");
});

test("a reminder bubble has Done and Snooze", async ({ page }) => {
  await page.goto("/?mock=1&bubble=Reminder%3A%20Call%20mom&remind=7#bubble");
  await expect(page.getByRole("button", { name: "Reminder: Call mom" })).toBeVisible();
  await page.getByRole("button", { name: "10 min" }).click();
  await expect.poll(() => calls(page)).toContain("reminder_snooze:7:10");
  await page.getByRole("button", { name: "Done" }).click();
  await expect.poll(() => calls(page)).toContain("reminder_done:7");
  await expect(page.getByRole("button", { name: "Not useful" })).toHaveCount(0);
});
