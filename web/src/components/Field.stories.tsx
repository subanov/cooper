import type { Meta, StoryObj } from "@storybook/react-vite";
import { Checkbox, SelectField, Switch, TextAreaField, TextField } from "./Field";

const meta = {
  title: "Design system/Field",
  component: TextField,
  args: { label: "Subject" },
} satisfies Meta<typeof TextField>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Empty: Story = {};
export const WithHint: Story = {
  args: { hint: "One line, for example “Laptop will not start”.", required: true },
};
export const WithError: Story = { args: { required: true, error: "Enter a subject." } };
export const Controls: Story = {
  render: () => (
    <div className="grid w-80 gap-4">
      <SelectField
        label="Priority"
        placeholder="Default"
        options={[
          { value: "h", label: "High" },
          { value: "n", label: "Normal" },
        ]}
      />
      <TextAreaField label="Description" rows={3} />
      <Checkbox label="Visible to the Requester" hint="Public Notes appear on the Portal." />
      <Switch label="Tasks Module" checked onChange={() => {}} stateLabels={["On", "Off"]} />
    </div>
  ),
};
