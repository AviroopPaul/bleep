import { expect, test, type Page } from "@playwright/test";

const calls = (page: Page) => page.evaluate(() => (window as unknown as { __calls: string[] }).__calls);

test("avatar settings: pick a buddy, color and finish", async ({ page }) => {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await page.goto("/?mock=1&g=builtin#settings");
  await page.getByRole("tab", { name: "Avatar" }).click();
  await expect(page.getByRole("heading", { name: "Your buddy" })).toBeVisible();

  const preview = page.getByTestId("av-preview").locator("svg");
  await expect(page.getByRole("radio", { name: "Puff" })).toHaveAttribute("aria-checked", "true");
  await expect(preview).toHaveAttribute("data-kind", "puff");
  await expect(preview).toHaveAttribute("data-finish", "glossy");

  await page.getByRole("radio", { name: "Blip" }).click();
  await expect(preview).toHaveAttribute("data-kind", "blip");
  await page.getByRole("radio", { name: "Pink" }).click();
  await expect(page.getByRole("radio", { name: "Pink" })).toHaveAttribute("aria-checked", "true");
  await page.getByRole("radio", { name: "Plush" }).click();
  await expect(preview).toHaveAttribute("data-finish", "plush");

  // Moods only change the preview.
  await page.getByRole("button", { name: "Thinking" }).click();
  await expect(page.getByTestId("av-preview").locator(".av")).toHaveAttribute("data-mood", "thinking");

  // Boop is pixel art: the finish is fixed.
  await page.getByRole("radio", { name: "Boop" }).click();
  await expect(page.getByText("Boop is pixel art, so it is always flat.")).toBeVisible();
  await expect(page.getByRole("radio", { name: "Plush" })).toBeDisabled();

  // The classic droid has no color or finish.
  await page.getByRole("radio", { name: "Droid" }).click();
  await expect(page.getByText("The droid keeps its own colors.")).toBeVisible();
  await expect(page.getByRole("radio", { name: "Pink" })).toHaveCount(0);

  expect(await calls(page)).toEqual(expect.arrayContaining([
    "avatar_set:blip:#3D6BFF:glossy",
    "avatar_set:blip:#FF4FA3:glossy",
    "avatar_set:blip:#FF4FA3:plush",
    "avatar_set:boop:#FF4FA3:plush",
    "avatar_set:droid:#FF4FA3:plush",
  ]));
  expect(errors).toEqual([]);
});

test("the floating view draws the saved buddy and follows moods", async ({ page }) => {
  await page.goto("/?mock=1&avatar=sprout:flat:1FC48E#droid");
  const av = page.locator(".droid-stage .av");
  await expect(av.locator("svg")).toHaveAttribute("data-kind", "sprout");
  await expect(av.locator("svg")).toHaveAttribute("data-finish", "flat");
  await expect(av).toHaveAttribute("data-mood", "idle");
  await page.evaluate(() => (window as unknown as { __voice: (e: unknown) => void }).__voice({ kind: "wake" }));
  await expect(av).toHaveAttribute("data-mood", "listening");
});

test("the classic droid still works", async ({ page }) => {
  await page.goto("/?mock=1&avatar=droid#droid");
  await expect(page.locator(".droid-stage .droid .lens")).toBeVisible();
});

test("chat header shows the buddy", async ({ page }) => {
  await page.goto("/?mock=1&avatar=spark#chat");
  await expect(page.locator(".head-droid svg")).toHaveAttribute("data-kind", "spark");
});
