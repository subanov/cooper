import { Pencil, Plus } from "lucide-react";
import { type ReactNode, useState } from "react";
import { Badge } from "@/components/Badge";
import { Button, IconButton } from "@/components/Button";
import { Card } from "@/components/Card";
import { type Column, DataTable } from "@/components/DataTable";
import { EmptyState } from "@/components/EmptyState";
import { ProblemNotice } from "@/components/Notice";
import { SectionHeader } from "@/components/Page";
import { useText } from "@/i18n/useText";

/**
 * The shared shape of organisation lists (Companies, Teams, Departments, Cost Centres,
 * Roles): a table with an Edit button per row and an Add button, editing in a dialog.
 */
export function OrganisationList<T extends { id: string; name: string; is_active?: boolean }>({
  title,
  intro,
  rows,
  loading,
  error,
  columns,
  canAdd = true,
  renderEditor,
}: {
  title: string;
  intro: string;
  rows: T[] | undefined;
  loading: boolean;
  error: unknown;
  columns: Column<T>[];
  canAdd?: boolean;
  renderEditor: (row: T | undefined, close: () => void) => ReactNode;
}) {
  const t = useText();
  const [editing, setEditing] = useState<T | "new" | null>(null);
  const all: Column<T>[] = [
    {
      id: "name",
      header: t("ref.name"),
      rowHeader: true,
      cell: (r) => <span className="font-medium">{r.name}</span>,
    },
    ...columns,
    {
      id: "status",
      header: t("ref.status"),
      cell: (r) =>
        r.is_active === false ? (
          <Badge tone="muted">{t("common.inactive")}</Badge>
        ) : (
          <Badge tone="success">{t("common.active")}</Badge>
        ),
    },
    {
      id: "edit",
      header: <span className="sr-only">{t("common.actions")}</span>,
      className: "w-12 text-right",
      cell: (r) => (
        <IconButton
          size="sm"
          tooltip="top"
          label={t("ref.edit", { name: r.name })}
          icon={<Pencil aria-hidden="true" className="size-3.5" />}
          onClick={() => setEditing(r)}
        />
      ),
    },
  ];
  return (
    <>
      <SectionHeader
        title={title}
        description={intro}
        actions={
          canAdd ? (
            <Button
              variant="primary"
              icon={<Plus aria-hidden="true" className="size-4" />}
              onClick={() => setEditing("new")}
            >
              {t("ref.add")}
            </Button>
          ) : undefined
        }
      />
      <ProblemNotice error={error} />
      <Card flush>
        <DataTable
          caption={title}
          rows={rows}
          loading={loading}
          rowKey={(r) => r.id}
          empty={<EmptyState title={t("ref.empty")} />}
          columns={all}
        />
      </Card>
      {editing
        ? renderEditor(editing === "new" ? undefined : editing, () => setEditing(null))
        : null}
    </>
  );
}
