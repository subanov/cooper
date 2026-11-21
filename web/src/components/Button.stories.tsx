import type { Meta, StoryObj } from "@storybook/react-vite";
import { Plus, Trash2 } from "lucide-react";
import { Button, IconButton } from "./Button";

const meta: Meta<typeof Button> = { title: "Design system/Button", component: Button };
export default meta;
type Story = StoryObj<typeof Button>;

export const Variants: Story = {
  render: () => (
    <div className="flex flex-wrap items-center gap-2">
      <Button variant="primary" icon={<Plus aria-hidden="true" />}>
        New ticket
      </Button>
      <Button>Assign</Button>
      <Button variant="ghost">Cancel</Button>
      <Button variant="quiet">See all</Button>
      <Button variant="danger" icon={<Trash2 aria-hidden="true" />}>
        Delete ticket
      </Button>
    </div>
  ),
};
export const Sizes: Story = {
  render: () => (
    <div className="flex flex-wrap items-center gap-2">
      <Button size="sm">Small</Button>
      <Button>Medium</Button>
      <Button size="lg">Large</Button>
      <IconButton label="Add" icon={<Plus aria-hidden="true" />} variant="secondary" />
    </div>
  ),
};
export const Loading: Story = { args: { loading: true, variant: "primary", children: "Saving" } };
export const Disabled: Story = { args: { disabled: true, children: "Assign to me" } };
