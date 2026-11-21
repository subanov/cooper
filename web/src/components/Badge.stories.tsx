import type { Meta, StoryObj } from "@storybook/react-vite";
import { TriangleAlert } from "lucide-react";
import { Badge, ColourLabel } from "./Badge";

const meta = {
  title: "Design system/Badge",
  component: Badge,
  args: { children: "Open" },
} satisfies Meta<typeof Badge>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Tones: Story = {
  render: () => (
    <div className="flex flex-wrap items-center gap-2">
      <Badge tone="neutral">Neutral</Badge>
      <Badge tone="accent">Accent</Badge>
      <Badge tone="success">On track</Badge>
      <Badge tone="warning" icon={<TriangleAlert aria-hidden="true" />}>
        Warning · 25m left
      </Badge>
      <Badge tone="danger" icon={<TriangleAlert aria-hidden="true" />}>
        2h overdue
      </Badge>
      <Badge tone="muted">Paused</Badge>
      <ColourLabel colour="#28a745">Green</ColourLabel>
    </div>
  ),
};
