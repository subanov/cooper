import type { Meta, StoryObj } from "@storybook/react-vite";
import { Badge } from "./Badge";
import { Card } from "./Card";
import { DataTable } from "./DataTable";
import { EmptyState } from "./EmptyState";

interface Row {
  id: string;
  name: string;
  status: string;
}

function Table({ rows, loading }: { rows: Row[] | undefined; loading: boolean }) {
  return (
    <Card flush className="w-[36rem]">
      <DataTable
        caption="Assets"
        rows={rows}
        loading={loading}
        rowKey={(r) => r.id}
        empty={<EmptyState title="No Assets match." />}
        columns={[
          {
            id: "name",
            header: "Asset",
            rowHeader: true,
            cell: (r) => <span className="font-medium">{r.name}</span>,
          },
          { id: "status", header: "Status", cell: (r) => <Badge tone="success">{r.status}</Badge> },
        ]}
      />
    </Card>
  );
}

const meta = {
  title: "Design system/DataTable",
  component: Table,
  args: {
    rows: [
      { id: "1", name: "LT-0042", status: "In use" },
      { id: "2", name: "SRV-DB-01", status: "In use" },
    ],
    loading: false,
  },
} satisfies Meta<typeof Table>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Rows: Story = {};
export const Empty: Story = { args: { rows: [] } };
export const Loading: Story = { args: { rows: undefined, loading: true } };
