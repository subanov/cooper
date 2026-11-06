import type { StorybookConfig } from "@storybook/react-vite";

/** Storybook for the design system in src/components (ADR-0010). */
const config: StorybookConfig = {
  stories: ["../src/**/*.stories.@(ts|tsx)"],
  framework: { name: "@storybook/react-vite", options: {} },
};

export default config;
