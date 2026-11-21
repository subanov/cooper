import type { Meta, StoryObj } from "@storybook/react-vite";
import { useState } from "react";
import { Combobox, type PickerOption } from "./Combobox";

const meta: Meta = { title: "Design system/Combobox" };
export default meta;
type Story = StoryObj;

const people: PickerOption[] = [
  { id: "1", label: "Ada Lovelace", description: "ada@example.com" },
  { id: "2", label: "Grace Hopper", description: "grace@example.com" },
];

function Picker({ initial }: { initial: PickerOption | null }) {
  const [value, setValue] = useState(initial);
  return (
    <div className="h-64 w-80">
      <Combobox
        label="Requester"
        hint="Search by name or email."
        value={value}
        onChange={setValue}
        options={people}
      />
    </div>
  );
}

export const Empty: Story = { render: () => <Picker initial={null} /> };
export const Selected: Story = { render: () => <Picker initial={people[0] ?? null} /> };
