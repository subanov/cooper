import AxeBuilder from "@axe-core/playwright";
import { type APIRequestContext, expect, type Page } from "@playwright/test";

// A fresh database (CI) gets this Administrator through first-run setup. Locally, set
// COOPER_E2E_LOGIN and COOPER_E2E_PASSWORD to an existing Analyst.
export const LOGIN = process.env.COOPER_E2E_LOGIN ?? "e2e-admin";
export const PASSWORD = process.env.COOPER_E2E_PASSWORD ?? "e2e correct horse battery";

const WCAG = ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa"];

/** Runs first-run setup if needed. Specs run in parallel, so another may win the race. */
export async function ensureAdmin(request: APIRequestContext) {
  const status = await request.get("/api/v1/setup");
  if (!(await status.json()).required) return;
  const setup = await request.post("/api/v1/setup", {
    data: {
      company_name: "E2E Company",
      ticket_code: "E2E",
      sender_domains: [],
      username: LOGIN,
      email: "e2e-admin@example.com",
      full_name: "Eve Admin",
      password: PASSWORD,
    },
  });
  expect([201, 409]).toContain(setup.status());
}

/** Signs in through the API; returns the CSRF token and the Analyst. */
export async function signIn(page: Page) {
  const session = await page.request.post("/api/v1/session", {
    data: { login: LOGIN, password: PASSWORD, totp_code: null, remember_device: false },
  });
  expect(session.status()).toBe(200);
  return (await session.json()) as { csrf_token: string; analyst: { id: string } };
}

/** WCAG 2.1 AA violations on the page, in the light and in the dark theme. */
export async function expectAccessible(page: Page) {
  for (const colorScheme of ["light", "dark"] as const) {
    await page.emulateMedia({ colorScheme, reducedMotion: "reduce" });
    const results = await new AxeBuilder({ page }).withTags(WCAG).analyze();
    expect(results.violations.map((v) => `${colorScheme}: ${v.id}`)).toEqual([]);
  }
}
