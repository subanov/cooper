import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "@playwright/test";
import { ensureAdmin, LOGIN, PASSWORD } from "./admin";

const WCAG = ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa"];
const clean = async (page: import("@playwright/test").Page) =>
  expect((await new AxeBuilder({ page }).withTags(WCAG).analyze()).violations).toEqual([]);

test.beforeAll(async ({ request }) => {
  await ensureAdmin(request);
});

test.beforeEach(async ({ page }) => {
  const session = await page.request.post("/api/v1/session", {
    data: { login: LOGIN, password: PASSWORD, totp_code: null, remember_device: false },
  });
  expect(session.status()).toBe(200);
});

test("an Analyst creates a Task and sees it on the board", async ({ page }) => {
  const title = `E2E task ${Date.now()}`;
  await page.goto("/tasks");
  await clean(page);
  await page.getByRole("link", { name: "New task" }).click();
  await page.getByLabel("Title").fill(title);
  await page.getByRole("button", { name: "Create" }).click();
  await expect(page.getByRole("heading", { name: title })).toBeVisible();
  await page.getByRole("link", { name: "Back to the board" }).click();
  await expect(page.getByRole("link", { name: title })).toBeVisible();
});

test("an Analyst writes and publishes a Knowledge Article", async ({ page }) => {
  const title = `E2E article ${Date.now()}`;
  await page.goto("/knowledge/new");
  await page.getByLabel("Title").fill(title);
  await page.getByLabel("Body (HTML)").fill("<p>Restart the router.</p><script>alert(1)</script>");
  await page.getByRole("button", { name: "Create" }).click();
  await expect(page.getByRole("heading", { name: title })).toBeVisible();
  await expect(page.getByText("Restart the router.")).toBeVisible();
  await clean(page);
  await page.getByRole("button", { name: "Publish" }).click();
  await expect(page.getByText("Published.")).toBeVisible();
});

test("an Administrator adds a service and opens an Incident", async ({ page }) => {
  const stamp = Date.now();
  await page.goto("/service-status/services");
  await page.getByRole("button", { name: "Add service" }).click();
  await page.getByLabel("Name").fill(`E2E service ${stamp}`);
  await page.getByRole("button", { name: "Save" }).click();
  await expect(page.getByRole("cell", { name: `E2E service ${stamp}`, exact: true })).toBeVisible();

  await page.goto("/service-status/incidents/new");
  await page.getByLabel("Title").fill(`E2E incident ${stamp}`);
  const level = page.getByLabel(`E2E service ${stamp}`);
  await level.selectOption({ index: 1 });
  await clean(page);
  await page.getByRole("button", { name: "Open an Incident" }).click();
  await expect(page.getByRole("heading", { name: `E2E incident ${stamp}` })).toBeVisible();
  await page.goto("/service-status/overview");
  await expect(page.getByRole("cell", { name: new RegExp(`E2E service ${stamp}`) })).toBeVisible();
  await clean(page);
});

test("an Analyst adds a Check and records its result", async ({ page }) => {
  const name = `E2E check ${Date.now()}`;
  await page.goto("/morning-checks/checks");
  await page.getByRole("button", { name: "Add Check" }).click();
  await page.getByLabel("Name", { exact: true }).fill(name);
  await page.getByRole("button", { name: "Save" }).click();
  await expect(page.getByRole("cell", { name, exact: true })).toBeVisible();

  await page.goto("/morning-checks");
  const row = page.getByRole("listitem").filter({ hasText: name });
  await expect(row).toBeVisible();
  await row.getByLabel(`Status: ${name}`).selectOption({ index: 1 });
  await clean(page);
  await row.getByRole("button", { name: "Record" }).click();
  await expect(row.getByText("Recorded.")).toBeVisible();
});
