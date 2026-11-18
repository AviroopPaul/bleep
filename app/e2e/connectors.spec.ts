import { expect, test, type Page } from "@playwright/test";

test.use({ viewport: { width: 460, height: 640 } });
const calls = (page: Page) => page.evaluate(() => (window as unknown as { __calls: string[] }).__calls);

async function open(page: Page, extra = "&connectors=1") {
  await page.goto(`/?mock=1&g=builtin${extra}#settings`);
  await page.getByRole("tab", { name: "Connectors" }).click();
}

test("lists built-in connectors, servers, tools and why a tool is left out", async ({ page }) => {
  await open(page);
  const day = page.getByRole("group", { name: "dayblocks" });
  await expect(day.getByRole("status")).toHaveText("Connected");
  await expect(day).toContainText("Signs in through your browser. 2 tools.");
  await expect(day.getByLabel("Mode for list_tasks")).toHaveValue("auto");
  await expect(day.getByLabel("Mode for move_task")).toHaveValue("ask");
  await expect(day).toContainText("Not used: Its name is too long.");
  await expect(page.getByRole("group", { name: "wiki" }).getByRole("status")).toHaveText("Needs sign-in");
  for (const b of ["Gmail", "Google Calendar", "Google Drive", "Memory", "Date and time", "Reminders"]) await expect(page.getByRole("tabpanel").getByText(b, { exact: true })).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(460);
  expect(await page.locator(".tabs").evaluate((el) => el.scrollWidth <= el.clientWidth + 1)).toBe(true);
});

test("add an open server", async ({ page }) => {
  await open(page, "");
  await page.getByLabel("Server URL").fill("https://mcp.deepwiki.com/mcp");
  await page.getByRole("button", { name: "Add", exact: true }).click();
  await expect(page.getByText("Added deepwiki. Its tools work in the chat from your next message.")).toBeVisible();
  await expect(page.getByRole("group", { name: "deepwiki" })).toContainText("No sign-in. 1 tool.");
  await expect(page.getByLabel("Server URL")).toHaveValue("");
});

test("a server without registration asks for a client ID", async ({ page }) => {
  await open(page, "");
  await page.getByLabel("Server URL").fill("https://noreg.example.com/mcp");
  await page.getByRole("button", { name: "Add", exact: true }).click();
  await expect(page.getByText("This server does not let bleep register itself.", { exact: false })).toBeVisible();
  await page.getByLabel("Client ID").fill("pre-1");
  await page.getByRole("button", { name: "Add", exact: true }).click();
  await expect(page.getByRole("group", { name: "noreg" })).toBeVisible();
  expect(await calls(page)).toContain("remote_add:https://noreg.example.com/mcp::pre-1");
});

test("a server with no OAuth asks for a token", async ({ page }) => {
  await open(page, "");
  await page.getByLabel("Server URL").fill("https://tokenonly.example.com/mcp");
  await page.getByRole("button", { name: "Add", exact: true }).click();
  await expect(page.getByText("Paste a token for it instead.", { exact: false })).toBeVisible();
  await page.getByLabel("Token", { exact: true }).fill("abc");
  await page.getByRole("button", { name: "Add", exact: true }).click();
  await expect(page.getByRole("group", { name: "tokenonly" })).toContainText("Uses a token you pasted");
});

test("a failed add shows the reason", async ({ page }) => {
  await open(page, "");
  await page.getByLabel("Server URL").fill("https://down.example.com/mcp");
  await page.getByRole("button", { name: "Add", exact: true }).click();
  await expect(page.getByRole("alert")).toHaveText("Could not reach down. It could not be reached.");
});

test("cancel stops a sign-in", async ({ page }) => {
  await open(page, "");
  await page.getByLabel("Server URL").fill("https://slow.example.com/mcp");
  await page.getByRole("button", { name: "Add", exact: true }).click();
  await expect(page.getByText("Checking how it signs in. If a browser tab opens, sign in there.")).toBeVisible();
  await page.getByRole("button", { name: "Cancel" }).click();
  await expect(page.getByRole("alert")).toHaveText("Sign-in was cancelled.");
  await expect(page.getByRole("group", { name: "slow" })).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Add", exact: true })).toBeEnabled();
});

test("tool modes change and the default clears", async ({ page }) => {
  await open(page);
  const day = page.getByRole("group", { name: "dayblocks" });
  await day.getByLabel("Mode for move_task").selectOption("auto");
  await expect(day.getByLabel("Mode for move_task")).toHaveValue("auto");
  await day.getByLabel("Mode for move_task").selectOption("ask");
  await expect(day.getByLabel("Mode for move_task")).toHaveValue("ask");
  await day.getByLabel("Mode for list_tasks").selectOption("off");
  await expect(day).toContainText("1 tool.");
  expect(await calls(page)).toEqual(expect.arrayContaining(["remote_set_tool:dayblocks:move_task:auto", "remote_set_tool:dayblocks:move_task:default", "remote_set_tool:dayblocks:list_tasks:off"]));
});

test("remove asks first", async ({ page }) => {
  await open(page);
  const day = page.getByRole("group", { name: "dayblocks" });
  await day.getByRole("button", { name: "Remove" }).click();
  await expect(day.getByRole("alert")).toContainText("Remove dayblocks? Its tools and sign-in are deleted.");
  await day.getByRole("button", { name: "Keep" }).click();
  await expect(day.getByRole("alert")).toHaveCount(0);
  await day.getByRole("button", { name: "Remove" }).click();
  await day.locator(".btn.danger").click();
  await expect(page.getByRole("group", { name: "dayblocks" })).toHaveCount(0);
  expect(await calls(page)).toContain("remote_remove:dayblocks");
});

test("a token server signs in again with a new token", async ({ page }) => {
  await open(page);
  const wiki = page.getByRole("group", { name: "wiki" });
  await wiki.getByLabel("New token").fill("fresh");
  await wiki.getByRole("button", { name: "Sign in again" }).click();
  await expect(wiki.getByRole("status")).toHaveText("Connected");
  expect(await calls(page)).toContain("remote_sign_in:wiki");
});

test("the catalog turns Google connectors on and off, one sign-in each", async ({ page }) => {
  await open(page, "&catalog=on");
  await expect(page.locator(".cat-ico svg path")).toHaveCount(7);
  const drive = page.getByRole("group", { name: "Google Drive" });
  await expect(drive.getByRole("status")).toHaveCount(0);
  await drive.getByRole("switch", { name: "Google Drive on" }).check();
  await expect.poll(() => calls(page)).toContain("catalog_set:drive:true:");
  await expect(drive.getByRole("status")).toHaveText("On");
  const gmail = page.getByRole("group", { name: "Gmail" });
  await expect(gmail.getByRole("switch", { name: "Gmail on" })).toBeChecked();
  await gmail.getByRole("switch", { name: "Gmail on" }).uncheck();
  await expect.poll(() => calls(page)).toContain("catalog_set:gmail:false:");
  await expect(gmail.getByRole("switch", { name: "Gmail on" })).not.toBeChecked();
});

test("apps: Linear turns on, GitHub needs a token first", async ({ page }) => {
  await open(page, "");
  const linear = page.getByRole("group", { name: "Linear" });
  await linear.getByRole("switch", { name: "Linear on" }).check();
  await expect(linear.getByRole("status")).toHaveText("On, 12 tools");
  const gh = page.getByRole("group", { name: "GitHub" });
  const sw = gh.getByRole("switch", { name: "GitHub on" });
  await expect(sw).toBeDisabled();
  await gh.getByLabel("GitHub token").fill("ghp_test");
  await sw.check();
  await expect.poll(() => calls(page)).toContain("catalog_set:github:true:ghp_test");
  await expect(gh.getByLabel("GitHub token")).toHaveCount(0);
  for (const s of ["Date and time", "Memory", "Reminders"]) await expect(page.locator(".cat-always").getByText(s, { exact: true })).toBeVisible();
});

test("a failed sign-in leaves the connector off and says why", async ({ page }) => {
  await open(page, "&catalog=fail");
  const cal = page.getByRole("group", { name: "Google Calendar" });
  await cal.getByRole("switch", { name: "Google Calendar on" }).click();
  await expect(page.getByRole("alert")).toContainText("access_denied");
  await expect(cal.getByRole("switch", { name: "Google Calendar on" })).not.toBeChecked();
});

test("Files on this Mac turns on with no sign-in", async ({ page }) => {
  await open(page, "");
  await expect(page.getByRole("heading", { name: "On this Mac", exact: true })).toBeVisible();
  const files = page.getByRole("group", { name: "Files on this Mac" });
  await files.getByRole("switch", { name: "Files on this Mac on" }).check();
  await expect.poll(() => calls(page)).toContain("catalog_set:files:true:");
  await expect(files.getByRole("status")).toHaveText("On");
});
