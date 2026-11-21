import type { Meta, StoryObj } from "@storybook/react-vite";
import { Checkbox, SelectField, Switch, TextAreaField, TextField } from "./Field";

const meta: Meta = { title: "Design system/Field" };
export default meta;
type Story = StoryObj;

export const Controls: Story = {
  render: () => (
    <div className="grid w-96 gap-4">
      <TextField label="Subject" required defaultValue="Printer on floor 2 is offline" />
      <SelectField label="Priority" options={[{ value: "n", label: "Normal" }]} defaultValue="n" />
      <TextAreaField label="Description" rows={3} />
      <Checkbox label="Visible to the Requester" />
      <Switch label="Tasks Module" checked onCheckedChange={() => {}} />
    </div>
  ),
};
export const WithError: Story = {
  render: () => (
    <div className="w-96">
      <TextField label="Subject" required error="Enter a subject." hint="One line." />
    </div>
  ),
};
