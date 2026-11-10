import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "@playwright/test";
import { ensureAdmin, LOGIN, PASSWORD } from "./admin";

const WCAG = ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa"];

test.beforeAll(async ({ request }) => {
  await ensureAdmin(request);
});

test.beforeEach(async ({ page }) => {
  const session = await page.request.post("/api/v1/session", {
    data: { login: LOGIN, password: PASSWORD, totp_code: null, remember_device: false },
  });
  expect(session.status()).toBe(200);
});

test("an Administrator manages ticket reference data and organisation lists", async ({ page }) => {
  const stamp = Date.now();
  await page.goto("/administration/ticket-statuses");
  await expect(page.getByRole("heading", { name: "Ticket Statuses" })).toBeVisible();
  expect((await new AxeBuilder({ page }).withTags(WCAG).analyze()).violations).toEqual([]);

  await page.getByRole("button", { name: "Add Status" }).click();
  await page.getByLabel("Name").fill(`E2E Status ${stamp}`);
  await page.getByRole("button", { name: "Save" }).click();
  await expect(page.getByRole("cell", { name: `E2E Status ${stamp}`, exact: true })).toBeVisible();

  const menu = page.getByRole("navigation", { name: "Administration sections" });
  await menu.getByRole("link", { name: "Roles" }).click();
  await page.getByRole("button", { name: "Add Role" }).click();
  await page.getByLabel("Name", { exact: true }).fill(`E2E Role ${stamp}`);
  await page.getByLabel("tickets.read").check();
  expect((await new AxeBuilder({ page }).withTags(WCAG).analyze()).violations).toEqual([]);
  await page.getByRole("button", { name: "Save" }).click();
  await expect(page.getByRole("cell", { name: `E2E Role ${stamp}`, exact: true })).toBeVisible();

  await menu.getByRole("link", { name: "Teams" }).click();
  await page.getByRole("button", { name: "Add Team" }).click();
  await page.getByLabel("Name", { exact: true }).fill(`E2E Team ${stamp}`);
  await page.getByRole("button", { name: "Save" }).click();
  await expect(page.getByRole("cell", { name: `E2E Team ${stamp}`, exact: true })).toBeVisible();
  expect((await new AxeBuilder({ page }).withTags(WCAG).analyze()).violations).toEqual([]);
});
