import type { Meta, StoryObj } from "@storybook/react-vite";
import { Alert } from "./alert";

const meta = {
  title: "Design system/Alert",
  component: Alert,
  args: { children: "Enter the code from your authenticator app." },
  decorators: [
    (Story) => (
      <div className="w-96">
        <Story />
      </div>
    ),
  ],
} satisfies Meta<typeof Alert>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Info: Story = {};
export const ErrorMessage: Story = {
  args: { variant: "error", children: "The username or password is wrong." },
};
export const Success: Story = {
  args: { variant: "success", children: "Your password is changed." },
};
