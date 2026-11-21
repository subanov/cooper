import type { Meta, StoryObj } from "@storybook/react-vite";
import { Plus, Trash2 } from "lucide-react";
import { Button, IconButton } from "./Button";

const meta = {
  title: "Design system/Button",
  component: Button,
  args: { children: "Save changes" },
} satisfies Meta<typeof Button>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Primary: Story = {
  args: { variant: "primary", icon: <Plus aria-hidden="true" className="size-4" /> },
};
export const Secondary: Story = { args: { variant: "secondary" } };
export const Ghost: Story = { args: { variant: "ghost" } };
export const Danger: Story = {
  args: {
    variant: "danger",
    children: "Delete ticket",
    icon: <Trash2 aria-hidden="true" className="size-4" />,
  },
};
export const Loading: Story = { args: { variant: "primary", loading: true } };
export const Disabled: Story = { args: { disabled: true } };
export const Sizes: Story = {
  render: () => (
    <div className="flex items-center gap-2">
      <Button size="sm">Small</Button>
      <Button>Medium</Button>
      <Button size="lg" variant="primary">
        Large (touch)
      </Button>
      <IconButton
        label="Add"
        icon={<Plus aria-hidden="true" className="size-4" />}
        variant="secondary"
      />
    </div>
  ),
};
