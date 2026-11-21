import type { Meta, StoryObj } from "@storybook/react-vite";
import { Inbox } from "lucide-react";
import { Button } from "./Button";
import { EmptyState, LoadingBlock, Notice } from "./Feedback";

const meta: Meta = { title: "Design system/Feedback" };
export default meta;
type Story = StoryObj;

export const Notices: Story = {
  render: () => (
    <div className="grid w-[32rem] gap-3">
      <Notice tone="info">The ticket is linked to the Task when it is created.</Notice>
      <Notice tone="success">Every Service is running normally.</Notice>
      <Notice tone="warning">Grace Hopper is writing a reply or Note.</Notice>
      <Notice tone="danger" title="Some values were not accepted">
        The Ticket Code is already in use.
      </Notice>
    </div>
  ),
};
export const Empty: Story = {
  render: () => (
    <EmptyState
      icon={<Inbox aria-hidden="true" />}
      title="No tickets here"
      action={<Button variant="primary">New ticket</Button>}
    >
      Nothing assigned to you. Pick up a ticket from Unassigned or your team's Queue.
    </EmptyState>
  ),
};
export const Loading: Story = { render: () => <LoadingBlock lines={4} className="w-96" /> };
