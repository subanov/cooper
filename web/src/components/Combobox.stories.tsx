import type { Meta, StoryObj } from "@storybook/react-vite";
import { useState } from "react";
import { Combobox, type PickerOption } from "./Combobox";

const people: PickerOption[] = [
  { id: "1", label: "Ada Lovelace", description: "ada@example.com" },
  { id: "2", label: "Grace Hopper", description: "grace@example.com" },
  { id: "3", label: "Katherine Johnson", description: "katherine@example.com" },
];

function Picker({ initial }: { initial: PickerOption | null }) {
  const [value, setValue] = useState(initial);
  return (
    <div className="w-80">
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

const meta = {
  title: "Design system/Combobox",
  component: Picker,
  args: { initial: null },
} satisfies Meta<typeof Picker>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Empty: Story = {};
export const Selected: Story = { args: { initial: people[1] ?? null } };
