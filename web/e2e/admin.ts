import { type APIRequestContext, expect } from "@playwright/test";

// A fresh database (CI) gets this Administrator through first-run setup. Locally, set
// COOPER_E2E_LOGIN and COOPER_E2E_PASSWORD to an existing Analyst.
export const LOGIN = process.env.COOPER_E2E_LOGIN ?? "e2e-admin";
export const PASSWORD = process.env.COOPER_E2E_PASSWORD ?? "e2e correct horse battery";

/** Runs first-run setup if needed. Specs run in parallel, so another may win the race. */
export async function ensureAdmin(request: APIRequestContext) {
  const status = await request.get("/api/v1/setup");
  if (!(await status.json()).required) {
    return;
  }
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
