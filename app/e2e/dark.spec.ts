import { expect, test, type Page } from "@playwright/test";

test.use({ colorScheme: "dark" });

/** WCAG contrast of an element's text against the nearest opaque background behind it. */
async function contrast(page: Page, selector: string): Promise<number> {
  return page.locator(selector).first().evaluate((el) => {
    const rgba = (s: string) => (s.match(/[\d.]+/g) ?? []).map(Number);
    const lum = ([r, g, b]: number[]) => {
      const c = [r, g, b].map((v) => {
        const x = v / 255;
        return x <= 0.03928 ? x / 12.92 : ((x + 0.055) / 1.055) ** 2.4;
      });
      return 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
    };
    let bg: number[] | null = null;
    for (let n: Element | null = el; n; n = n.parentElement) {
      const c = rgba(getComputedStyle(n).backgroundColor);
      if (c.length >= 3 && (c.length === 3 || c[3] >= 0.95)) {
        bg = c;
        break;
      }
    }
    bg ??= [30, 33, 38]; // the dark panel the see-through windows sit on
    const fg = rgba(getComputedStyle(el).color);
    const [a, b] = [lum(fg), lum(bg)].sort((x, y) => y - x);
    return (a + 0.05) / (b + 0.05);
  });
}

test("dark mode: settings text is readable", async ({ page }) => {
  await page.goto("/?mock=1&g=builtin#settings");
  await expect(page.getByRole("heading", { name: "Settings" })).toBeVisible();
  for (const sel of [".settings h1", ".set-head h2", ".set-lead", ".pill", ".own summary"]) {
    expect(await contrast(page, sel), sel).toBeGreaterThanOrEqual(4.5);
  }
  await page.getByText("Use your own Google Cloud app").click();
  for (const sel of [".linkbtn", ".fields label", ".steps p"]) {
    expect(await contrast(page, sel), sel).toBeGreaterThanOrEqual(4.5);
  }
  await page.screenshot({ path: test.info().outputPath("settings-dark.png") });
});

test("dark mode: chat Send button and card text are readable", async ({ page }) => {
  await page.goto("/?mock=1#chat");
  await page.getByLabel("Message bleep").fill("hi");
  expect(await contrast(page, ".composer button"), "Send").toBeGreaterThanOrEqual(4.5);
  await page.getByLabel("Message bleep").fill("add a reminder to pay rent at 6 PM");
  await page.getByLabel("Message bleep").press("Enter");
  await expect(page.getByRole("region", { name: "Add to your calendar?" })).toBeVisible();
  for (const sel of [".card-h", ".card dt", ".card dd"]) {
    expect(await contrast(page, sel), sel).toBeGreaterThanOrEqual(4.5);
  }
  await page.screenshot({ path: test.info().outputPath("chat-dark.png") });
});

test("dark mode: droid bubble is readable", async ({ page }) => {
  await page.setViewportSize({ width: 240, height: 72 });
  await page.goto("/?mock=1&bubble=Design%20review%20in%201%20hour#bubble");
  await expect(page.locator(".bubble")).toBeVisible();
  expect(await contrast(page, ".bubble")).toBeGreaterThanOrEqual(4.5);
});

test("dark mode: mail and activity tabs are readable", async ({ page }) => {
  await page.goto("/?mock=1&g=builtin#settings");
  await page.getByRole("tab", { name: "Mail" }).click();
  for (const sel of [".tab[aria-selected=true]", ".tab[aria-selected=false]", ".chip", ".mail-h"]) {
    expect(await contrast(page, sel), sel).toBeGreaterThanOrEqual(4.5);
  }
  await page.getByRole("tab", { name: "Activity" }).click();
  for (const sel of [".act-title", ".act-time", ".act-bad"]) {
    expect(await contrast(page, sel), sel).toBeGreaterThanOrEqual(4.5);
  }
});
