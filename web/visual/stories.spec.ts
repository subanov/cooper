import { readFileSync } from "node:fs";
import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "@playwright/test";

interface Entry {
  id: string;
  type: string;
}

// The stories of the built Storybook (`pnpm build-storybook`).
const index = JSON.parse(readFileSync("storybook-static/index.json", "utf8")) as {
  entries: Record<string, Entry>;
};
const stories = Object.values(index.entries).filter((e) => e.type === "story");

for (const theme of ["light", "dark"] as const) {
  for (const story of stories) {
    test(`${story.id} (${theme})`, async ({ page }) => {
      await page.emulateMedia({ colorScheme: theme, reducedMotion: "reduce" });
      await page.goto(`/iframe.html?id=${story.id}&viewMode=story`);
      const root = page.locator("#storybook-root");
      await expect(root).not.toBeEmpty();
      await expect(root).toHaveScreenshot(`${story.id}-${theme}.png`);
      const results = await new AxeBuilder({ page })
        .include("#storybook-root")
        .withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa"])
        .analyze();
      expect(results.violations.map((v) => v.id)).toEqual([]);
    });
  }
}
