import type { Meta, StoryObj } from "@storybook/react-vite";
import { Inbox } from "lucide-react";
import { Button } from "./Button";
import { EmptyState } from "./EmptyState";

const meta = {
  title: "Design system/EmptyState",
  component: EmptyState,
  args: {
    icon: <Inbox aria-hidden="true" />,
    title: "No tickets here",
    children: "Nothing assigned to you. Pick up a ticket from Unassigned or your team's Queue.",
    action: <Button variant="primary">New ticket</Button>,
  },
} satisfies Meta<typeof EmptyState>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};
