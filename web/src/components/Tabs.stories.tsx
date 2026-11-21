import type { Meta, StoryObj } from "@storybook/react-vite";
import { useState } from "react";
import { Segmented, Tabs } from "./Tabs";

const meta: Meta = { title: "Design system/Tabs" };
export default meta;
type Story = StoryObj;

function Toggle() {
  const [value, setValue] = useState<"reply" | "note">("reply");
  return (
    <Segmented
      label="What to write"
      value={value}
      onChange={setValue}
      options={[
        { value: "reply", label: "Reply to Requester" },
        { value: "note", label: "Note" },
      ]}
    />
  );
}

export const SegmentedControl: Story = { render: () => <Toggle /> };
export const RecordTabs: Story = {
  render: () => (
    <div className="w-96">
      <Tabs
        label="Parts of the Team"
        tabs={[
          {
            id: "details",
            label: "Details",
            content: <p className="text-sm">Name and description.</p>,
          },
          {
            id: "members",
            label: "Members",
            content: <p className="text-sm">Analysts in the Team.</p>,
          },
        ]}
      />
    </div>
  ),
};
