import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "@playwright/test";
import { ensureAdmin, LOGIN, PASSWORD } from "./admin";

const WCAG = ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa"];

test.beforeAll(async ({ request }) => {
  await ensureAdmin(request);
});

test("an Administrator reaches every Administration section by keyboard-usable tabs", async ({
  page,
}) => {
  const session = await page.request.post("/api/v1/session", {
    data: { login: LOGIN, password: PASSWORD, totp_code: null, remember_device: false },
  });
  expect(session.status()).toBe(200);

  await page.goto("/administration");
  await expect(page).toHaveURL(/\/administration\/settings$/);
  await expect(page.getByRole("heading", { name: "Administration" })).toBeVisible();
  expect((await new AxeBuilder({ page }).withTags(WCAG).analyze()).violations).toEqual([]);

  const tabs = page.getByRole("navigation", { name: "Administration sections" });
  for (const [name, url] of [
    ["Modules", /\/modules$/],
    ["Background Jobs", /\/jobs$/],
    ["System Log", /\/log$/],
  ] as const) {
    await tabs.getByRole("link", { name }).click();
    await expect(page).toHaveURL(url);
    await expect(tabs.getByRole("link", { name })).toHaveAttribute("aria-current", "page");
    expect((await new AxeBuilder({ page }).withTags(WCAG).analyze()).violations).toEqual([]);
  }

  await page.goto("/administration/modules");
  await expect(page.getByText("Core Module").first()).toBeVisible();
});
