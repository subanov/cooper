import type { Meta, StoryObj } from "@storybook/react-vite";
import { Property, PropertyList, PropertySelect } from "./PropertyList";

const meta: Meta = { title: "Design system/PropertyList" };
export default meta;
type Story = StoryObj;

export const Properties: Story = {
  render: () => (
    <div className="w-80 rounded-lg border border-line bg-surface p-4">
      <PropertyList>
        <Property label="Requester">Rita Requester</Property>
        <PropertySelect
          label="Status"
          value="o"
          onChange={() => {}}
          options={[{ value: "o", label: "Open" }]}
        />
        <PropertySelect
          label="Priority"
          value="h"
          saving
          onChange={() => {}}
          options={[{ value: "h", label: "High" }]}
        />
        <PropertySelect
          label="Assignee"
          value=""
          placeholder="Unassigned"
          onChange={() => {}}
          options={[]}
        />
      </PropertyList>
    </div>
  ),
};
