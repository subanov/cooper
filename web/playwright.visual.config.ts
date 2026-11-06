import { defineConfig, devices } from "@playwright/test";

/**
 * Visual regression tests of the design system: every Storybook story is rendered and
 * compared with a committed screenshot (ADR-0010). Run inside the Playwright container
 * (`just web-visual`) so fonts and rendering match CI; `just web-visual-update` refreshes
 * the baselines after an intended change.
 */
export default defineConfig({
  testDir: "visual",
  snapshotPathTemplate: "{testDir}/__screenshots__/{arg}{ext}",
  fullyParallel: true,
  forbidOnly: Boolean(process.env.CI),
  reporter: process.env.CI ? "github" : "list",
  expect: { toHaveScreenshot: { maxDiffPixelRatio: 0.002, animations: "disabled" } },
  use: { baseURL: "http://127.0.0.1:6007", trace: "retain-on-failure" },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
  webServer: {
    command:
      "node node_modules/vite/bin/vite.js preview --outDir storybook-static --port 6007 --strictPort --host 127.0.0.1",
    url: "http://127.0.0.1:6007/index.json",
    reuseExistingServer: !process.env.CI,
  },
});
