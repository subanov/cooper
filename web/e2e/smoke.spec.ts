import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "@playwright/test";

const WCAG = ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa"];

for (const path of ["/", "/portal"]) {
  test(`${path} loads under the CSP and passes axe-core`, async ({ page }) => {
    const errors: string[] = [];
    page.on("console", (message) => {
      if (message.type() === "error") {
        errors.push(message.text());
      }
    });
    await page.goto(path);
    await expect(page.getByRole("main")).toBeVisible();
    const results = await new AxeBuilder({ page }).withTags(WCAG).analyze();
    expect(results.violations.map((v) => v.id)).toEqual([]);
    // A Content-Security-Policy violation shows up as a console error; the signed-out
    // session request (401) is expected.
    expect(errors.filter((e) => !e.includes("401"))).toEqual([]);
  });
}
