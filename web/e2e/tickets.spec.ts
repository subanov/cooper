import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "@playwright/test";
import { ensureAdmin, LOGIN, PASSWORD } from "./admin";

const WCAG = ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa"];

test.beforeAll(async ({ request }) => {
  await ensureAdmin(request);
});

test("an Analyst triages a ticket from the list by keyboard", async ({ page }) => {
  // Sign in through the API (the sign-in screen has its own spec) and raise a ticket.
  const session = await page.request.post("/api/v1/session", {
    data: { login: LOGIN, password: PASSWORD, totp_code: null, remember_device: false },
  });
  expect(session.status()).toBe(200);
  const { csrf_token: csrf, analyst } = await session.json();
  const companies = await (await page.request.get("/api/v1/companies")).json();
  const companyId: string = companies.items[0].id;
  const subject = `E2E printer jam ${Date.now()}`;
  const created = await page.request.post("/api/v1/tickets", {
    headers: { "X-CSRF-Token": csrf },
    data: {
      subject,
      description: "Paper stuck in tray 2.",
      company_id: companyId,
      analyst_id: analyst.id,
    },
  });
  expect(created.status(), await created.text()).toBe(201);

  await page.goto(`/tickets?view=mine&q=${encodeURIComponent(subject)}`);
  const table = page.getByRole("table", { name: "Tickets: My tickets" });
  await expect(table.getByRole("link", { name: new RegExp(subject) })).toBeVisible();
  expect((await new AxeBuilder({ page }).withTags(WCAG).analyze()).violations).toEqual([]);

  await page.keyboard.press("j");
  const heading = page.getByRole("heading", { name: subject });
  await expect(heading).toBeFocused();
  await expect(page.getByText("Paper stuck in tray 2.")).toBeVisible();

  await page.keyboard.press("n");
  await page.keyboard.type("Ordered a new roller.");
  await page.getByRole("button", { name: "Add Note" }).click();
  const timeline = page.getByRole("region", { name: "Timeline" });
  await expect(timeline.getByText("Ordered a new roller.")).toBeVisible();
  await expect(timeline.getByText("Internal Note")).toBeVisible();
  expect((await new AxeBuilder({ page }).withTags(WCAG).analyze()).violations).toEqual([]);

  await page.emulateMedia({ colorScheme: "dark" });
  expect((await new AxeBuilder({ page }).withTags(WCAG).analyze()).violations).toEqual([]);
});
