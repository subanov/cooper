import type { Preview } from "@storybook/react-vite";
import { ToastProvider } from "../src/components/Toast";
import { IntlProvider } from "../src/i18n/IntlProvider";
import "../src/styles.css";

/** Components read catalogue messages, so every story gets the providers the app has. */
const preview: Preview = {
  parameters: { layout: "centered" },
  decorators: [
    (Story) => (
      <IntlProvider locale="en">
        <ToastProvider>
          <Story />
        </ToastProvider>
      </IntlProvider>
    ),
  ],
};

export default preview;
