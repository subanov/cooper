import { expect, test } from "@playwright/test";
import { expectAccessible } from "./support";

test("the Portal asks signed-out Requesters to sign in", async ({ page }) => {
  await page.goto("/portal/tickets");
  await expect(page).toHaveURL(/\/portal\/sign-in/);
  await expect(page.getByRole("heading", { name: "Sign in to the Help Centre" })).toBeVisible();
  await expect(page.getByRole("link", { name: /Forgot your password/ })).toBeVisible();
  await expectAccessible(page);
});

test("a Portal password reset never reveals whether the address exists", async ({ page }) => {
  await page.goto("/portal/reset-password");
  await page.getByLabel("Email address").fill("nobody@example.invalid");
  await page.getByRole("button", { name: "Email me a link" }).click();
  await expect(page.getByRole("status")).toContainText("If the address is known");
});
