import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "@playwright/test";
import { ensureAdmin, LOGIN, PASSWORD } from "./admin";

const WCAG = ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa"];

test.beforeAll(async ({ request }) => {
  await ensureAdmin(request);
});

test("an Analyst signs in, navigates by keyboard and signs out", async ({ page }) => {
  await page.goto("/tickets");
  await expect(page.getByRole("heading", { name: "Sign in to Cooper" })).toBeVisible();
  expect((await new AxeBuilder({ page }).withTags(WCAG).analyze()).violations).toEqual([]);

  await page.getByLabel("Username or email").fill(LOGIN);
  await page.getByLabel("Password").fill(PASSWORD);
  await page.getByRole("button", { name: "Sign in" }).click();

  // Back where the Analyst wanted to go.
  await expect(page.getByRole("heading", { name: "Tickets" })).toBeVisible();
  await expect(page).toHaveURL(/\/tickets$/);
  const nav = page.getByRole("navigation", { name: "Main" });
  await expect(nav.getByRole("link", { name: "Administration" })).toBeVisible();
  expect((await new AxeBuilder({ page }).withTags(WCAG).analyze()).violations).toEqual([]);

  await page.keyboard.press("Control+k");
  const palette = page.getByRole("dialog");
  await palette.getByRole("combobox").fill("home");
  await page.keyboard.press("Enter");
  await expect(page.getByRole("heading", { name: /^Welcome, / })).toBeVisible();

  // Dark theme passes contrast checks too.
  await page.getByRole("button", { name: /^Account menu for / }).click();
  await page.getByRole("menuitemradio", { name: "Dark" }).click();
  await expect(page.locator("html")).toHaveClass(/dark/);
  expect((await new AxeBuilder({ page }).withTags(WCAG).analyze()).violations).toEqual([]);

  await page.getByRole("button", { name: /^Account menu for / }).click();
  await page.getByRole("menuitem", { name: "Sign out" }).click();
  await expect(page.getByRole("heading", { name: "Sign in to Cooper" })).toBeVisible();
  await page.goto("/");
  await expect(page.getByRole("heading", { name: "Sign in to Cooper" })).toBeVisible();
});
