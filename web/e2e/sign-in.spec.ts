import { expect, test } from "@playwright/test";
import { ensureAdmin, LOGIN, PASSWORD } from "./support";

test.beforeAll(async ({ request }) => {
  await ensureAdmin(request);
});

test("an Analyst signs in and lands where they were going", async ({ page }) => {
  await page.goto("/tickets?view=open");
  await expect(page).toHaveURL(/\/sign-in\?redirect=/);
  await page.getByLabel("Username or email").fill(LOGIN);
  await page.getByLabel("Password").fill(PASSWORD);
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(page).toHaveURL(/\/tickets\?view=open/);
  await expect(page.getByRole("navigation", { name: "Main" })).toBeVisible();
});

test("a wrong password is explained without saying which part was wrong", async ({ page }) => {
  await page.goto("/sign-in");
  await page.getByLabel("Username or email").fill(LOGIN);
  await page.getByLabel("Password").fill("not the password");
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(page.getByRole("alert")).toContainText("The username or password is wrong");
  await expect(page.getByLabel("Username or email")).toHaveValue(LOGIN);
});
