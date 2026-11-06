import AxeBuilder from "@axe-core/playwright";
import { expect, type Page, test } from "@playwright/test";

const WCAG = ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa"];

async function expectAccessible(page: Page) {
  expect((await new AxeBuilder({ page }).withTags(WCAG).analyze()).violations).toEqual([]);
}

// Signed-out Portal screens against the real server (FR-PRT-001). Signing in needs a
// password set through an emailed link; the API tests cover that flow end to end.
test("a Requester reaches Portal sign-in and the set-password screens", async ({ page }) => {
  await page.goto("/portal/tickets");
  await expect(page.getByRole("heading", { name: "Sign in" })).toBeVisible();
  await expect(page).toHaveURL(/\/portal\/sign-in\?redirect=/);
  await expectAccessible(page);
  // Self-Registration is off by default.
  await expect(page.getByRole("link", { name: /^New here\?/ })).toHaveCount(0);

  await page.getByLabel("Email address").fill("nobody@example.com");
  await page.getByLabel("Password").fill("not the password");
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(page.getByRole("alert")).toHaveText("The email address or password is wrong.");

  await page.getByRole("link", { name: /^Forgot your password/ }).click();
  await expect(page.getByRole("heading", { name: "Get a password link" })).toBeVisible();
  await page.getByLabel("Email address").fill("nobody@example.com");
  await page.getByRole("button", { name: "Send the link" }).click();
  await expect(page.getByRole("status")).toContainText("If the address is known");
  await expectAccessible(page);

  await page.goto("/portal/set-password?token=not-a-real-token");
  await page.getByLabel("New password", { exact: true }).fill("a long new password");
  await page.getByLabel("Repeat the new password").fill("a long new password");
  await page.getByRole("button", { name: "Save the password" }).click();
  await expect(page.getByRole("alert")).toContainText("This link is invalid");

  await page.emulateMedia({ colorScheme: "dark" });
  await expectAccessible(page);
});

test("Self-Registration explains that it is turned off", async ({ page }) => {
  await page.goto("/portal/register");
  await page.getByLabel("Your name").fill("Nia New");
  await page.getByLabel("Work email address").fill("nia@example.com");
  await page.getByRole("button", { name: "Send the confirmation link" }).click();
  await expect(page.getByRole("alert")).toHaveText(
    "Registration is turned off. Contact your service desk for access.",
  );
  await expectAccessible(page);
});
