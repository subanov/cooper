import { expect, test } from "@playwright/test";
import { ensureAdmin, expectAccessible, signIn } from "./support";

test.beforeAll(async ({ request }) => {
  await ensureAdmin(request);
});

/** Every top-level screen of the Analyst console renders and passes axe-core. */
const SCREENS: [path: string, heading: string][] = [
  ["/", "Good"],
  ["/tickets?view=open", "Tickets"],
  ["/tickets/new?view=open", "New ticket"],
  ["/tasks", "Tasks"],
  ["/assets", "Assets"],
  ["/contracts", "Contracts"],
  ["/suppliers", "Contracts"],
  ["/calendar", "Calendar"],
  ["/knowledge", "Knowledge"],
  ["/service-status/overview", "Service Status"],
  ["/morning-checks/today", "Morning Checks"],
  ["/users/analysts", "Users"],
  ["/administration/settings", "Administration"],
  ["/administration/ticket-statuses", "Administration"],
  ["/administration/mailboxes", "Administration"],
  ["/notifications", "Notifications"],
  ["/account", "My account"],
];

for (const [path, heading] of SCREENS) {
  test(`${path} is usable and accessible`, async ({ page }) => {
    await signIn(page);
    await page.goto(path);
    await expect(page.getByRole("heading", { name: new RegExp(heading) }).first()).toBeVisible();
    await expect(page.getByRole("status").filter({ hasText: "Loading" })).toHaveCount(0);
    await expectAccessible(page);
  });
}
