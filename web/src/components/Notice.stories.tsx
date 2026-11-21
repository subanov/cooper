import type { Meta, StoryObj } from "@storybook/react-vite";
import { Button } from "./Button";
import { Notice } from "./Notice";

const meta = {
  title: "Design system/Notice",
  component: Notice,
  args: { children: "Your password is changed." },
} satisfies Meta<typeof Notice>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Info: Story = { args: { tone: "info", title: "Cooper is not set up yet." } };
export const Success: Story = { args: { tone: "success" } };
export const Warning: Story = {
  args: { tone: "warning", children: "Grace Hopper is writing a reply or Note." },
};
export const Danger: Story = {
  args: {
    tone: "danger",
    title: "Some values were not accepted",
    children: "The Ticket Code must be two to five letters.",
    action: <Button size="sm">Try again</Button>,
  },
};
