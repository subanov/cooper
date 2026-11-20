import { expect, test } from "@playwright/test";
import { expectAccessible } from "./support";

for (const path of ["/sign-in", "/reset-password", "/portal/sign-in", "/portal/reset-password"]) {
  test(`${path} loads under the CSP and passes axe-core in both themes`, async ({ page }) => {
    const errors: string[] = [];
    page.on("console", (message) => {
      if (message.type() === "error") errors.push(message.text());
    });
    await page.goto(path);
    await expect(page.getByRole("main")).toBeVisible();
    await expect(page.getByRole("heading", { level: 1 })).toBeVisible();
    await expectAccessible(page);
    // A Content-Security-Policy violation shows up as a console error; the signed-out
    // session request (401) is expected.
    expect(errors.filter((e) => !e.includes("401"))).toEqual([]);
  });
}
