import type { Meta, StoryObj } from "@storybook/react-vite";
import { Field } from "./field";

const meta = {
  title: "Design system/Field",
  component: Field,
  args: { label: "Username or email", name: "login" },
  decorators: [
    (Story) => (
      <div className="w-80">
        <Story />
      </div>
    ),
  ],
} satisfies Meta<typeof Field>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Empty: Story = {};
export const WithHint: Story = {
  args: { label: "New password", type: "password", hint: "At least 12 characters." },
};
export const WithError: Story = {
  args: {
    label: "Repeat the new password",
    type: "password",
    defaultValue: "secret",
    error: "The passwords do not match.",
  },
};
