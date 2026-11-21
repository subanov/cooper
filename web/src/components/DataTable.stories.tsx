import type { Meta, StoryObj } from "@storybook/react-vite";
import { Badge } from "./Badge";
import { DataTable } from "./DataTable";
import { EmptyState } from "./Feedback";

const meta: Meta = { title: "Design system/DataTable" };
export default meta;
type Story = StoryObj;

const rows = [
  { id: "1", name: "Open", active: true },
  { id: "2", name: "Awaiting Response", active: true },
  { id: "3", name: "Legacy", active: false },
];

export const Rows: Story = {
  render: () => (
    <div className="w-[32rem] rounded-lg border border-line bg-surface">
      <DataTable
        caption="Ticket Statuses"
        rows={rows}
        rowKey={(r) => r.id}
        columns={[
          { key: "name", header: "Name", cell: (r) => r.name },
          {
            key: "status",
            header: "Status",
            cell: (r) => (
              <Badge tone={r.active ? "success" : "muted"}>
                {r.active ? "Active" : "Inactive"}
              </Badge>
            ),
          },
        ]}
      />
    </div>
  ),
};
export const Empty: Story = {
  render: () => (
    <DataTable
      caption="Ticket Statuses"
      rows={[] as typeof rows}
      rowKey={(r) => r.id}
      columns={[{ key: "name", header: "Name", cell: (r) => r.name }]}
      empty={<EmptyState title="Nothing here yet." />}
    />
  ),
};
