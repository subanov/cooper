import type { Meta, StoryObj } from "@storybook/react-vite";
import { TriangleAlert } from "lucide-react";
import { Badge, ColourLabel } from "./Badge";

const meta: Meta<typeof Badge> = { title: "Design system/Badge", component: Badge };
export default meta;
type Story = StoryObj<typeof Badge>;

export const Tones: Story = {
  render: () => (
    <div className="flex flex-wrap items-center gap-2">
      <Badge tone="neutral">Open</Badge>
      <Badge tone="muted">Normal</Badge>
      <Badge tone="accent">In Progress</Badge>
      <Badge tone="success">On track</Badge>
      <Badge tone="warning">Confidential</Badge>
      <Badge tone="danger" icon={<TriangleAlert aria-hidden="true" />}>
        2h overdue
      </Badge>
      <Badge tone="info">Public</Badge>
      <ColourLabel colour="#28a745">Green</ColourLabel>
    </div>
  ),
};
