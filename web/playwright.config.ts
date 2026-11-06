import { defineConfig, devices } from "@playwright/test";

/**
 * End-to-end tests against a running cooper-server that serves the built SPA
 * (`just web-build` then `just server`). Set COOPER_E2E_URL to test another address.
 */
export default defineConfig({
  testDir: "e2e",
  fullyParallel: true,
  forbidOnly: Boolean(process.env.CI),
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? "github" : "list",
  use: {
    baseURL: process.env.COOPER_E2E_URL ?? "http://localhost:8080",
    trace: "retain-on-failure",
  },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
});
