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

test("an Analyst raises a ticket from the New ticket form", async ({ page }) => {
  const subject = `E2E new ticket ${Date.now()}`;
  await page.goto("/tickets");
  await page.getByRole("link", { name: "New ticket" }).click();
  await expect(page).toHaveURL(/\/tickets\/new$/);
  const form = page.getByRole("form", { name: "New ticket" });
  await expect(form).toBeVisible();
  expect((await new AxeBuilder({ page }).withTags(WCAG).analyze()).violations).toEqual([]);

  await form.getByRole("button", { name: "Create ticket" }).click();
  await expect(form.getByText("Enter a subject.")).toBeVisible();

  await form.getByLabel("Subject").fill(subject);
  await form.getByLabel("Description").fill("Created by the e2e suite.");
  await form.getByRole("button", { name: "Create ticket" }).click();

  await expect(page).toHaveURL(/\/tickets\/[0-9a-f-]{36}/);
  await expect(page.getByRole("heading", { name: subject })).toBeVisible();
});

test("an Administrator creates an Analyst and a Requester", async ({ page }) => {
  const stamp = Date.now();
  await page.goto("/users");
  await expect(page).toHaveURL(/\/users\/analysts$/);
  expect((await new AxeBuilder({ page }).withTags(WCAG).analyze()).violations).toEqual([]);

  await page.getByRole("link", { name: "New Analyst" }).click();
  await expect(page.getByRole("heading", { level: 1 })).toBeFocused();
  await page.getByLabel("Full name").fill(`E2E Analyst ${stamp}`);
  await page.getByLabel("Username").fill(`e2e-${stamp}`);
  await page.getByLabel("Email").fill(`e2e-${stamp}@example.com`);
  await page.getByLabel("Initial password").fill("e2e correct horse battery");
  await page.getByRole("button", { name: "Create", exact: true }).click();
  await expect(page.getByRole("heading", { name: `E2E Analyst ${stamp}` })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Roles" })).toBeVisible();
  expect((await new AxeBuilder({ page }).withTags(WCAG).analyze()).violations).toEqual([]);

  await page.goto("/users/requesters/new");
  await page.getByLabel("Full name").fill(`E2E Requester ${stamp}`);
  await page.getByLabel("Email").fill(`req-${stamp}@example.com`);
  await page.getByRole("button", { name: "Create", exact: true }).click();
  await expect(page.getByRole("heading", { name: `E2E Requester ${stamp}` })).toBeVisible();
  expect((await new AxeBuilder({ page }).withTags(WCAG).analyze()).violations).toEqual([]);
});
