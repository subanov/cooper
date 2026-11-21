import { expect, test } from "@playwright/test";
import { ensureAdmin, expectAccessible, signIn } from "./support";

test.beforeAll(async ({ request }) => {
  await ensureAdmin(request);
});

test("an Analyst triages a ticket from the list by keyboard", async ({ page }) => {
  const { csrf_token: csrf, analyst } = await signIn(page);
  const companies = await (await page.request.get("/api/v1/companies")).json();
  const subject = `E2E printer jam ${Date.now()}`;
  const created = await page.request.post("/api/v1/tickets", {
    headers: { "X-CSRF-Token": csrf },
    data: {
      subject,
      description: "Paper stuck in tray 2.",
      company_id: companies.items[0].id,
      analyst_id: analyst.id,
    },
  });
  expect(created.status(), await created.text()).toBe(201);

  await page.goto(`/tickets?view=mine&q=${encodeURIComponent(subject)}`);
  const list = page.getByRole("list", { name: "Tickets in My tickets" });
  await expect(list.getByRole("link", { name: subject })).toBeVisible();
  await expectAccessible(page);

  await page.keyboard.press("j");
  await expect(page.getByRole("heading", { name: subject, level: 2 })).toBeFocused();
  await expect(page.getByText("Paper stuck in tray 2.")).toBeVisible();

  await page.keyboard.press("n");
  await page.keyboard.type("Ordered a new roller.");
  await page.getByRole("button", { name: "Add Note" }).click();
  const timeline = page.getByRole("region", { name: "Timeline" });
  await expect(timeline.getByText("Ordered a new roller.")).toBeVisible();
  await expect(timeline.getByText("Internal Note")).toBeVisible();
  await expectAccessible(page);
});

test("the command palette opens a ticket by its number", async ({ page }) => {
  const { csrf_token: csrf } = await signIn(page);
  const companies = await (await page.request.get("/api/v1/companies")).json();
  const created = await page.request.post("/api/v1/tickets", {
    headers: { "X-CSRF-Token": csrf },
    data: { subject: `E2E palette ${Date.now()}`, company_id: companies.items[0].id },
  });
  const tk = await created.json();
  await page.goto("/");
  await expect(page.getByRole("navigation", { name: "Main" })).toBeVisible();
  await page.keyboard.press("Control+k");
  await page.getByPlaceholder(/Type a screen/).fill(tk.number);
  await page.keyboard.press("Enter");
  await expect(page.getByRole("heading", { name: tk.subject, level: 2 })).toBeVisible();
});
