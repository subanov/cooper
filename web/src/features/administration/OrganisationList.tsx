import { Pencil, Plus } from "lucide-react";
import { type ReactNode, useState } from "react";
import { Badge } from "@/components/Badge";
import { Button, IconButton } from "@/components/Button";
import { type Column, DataTable } from "@/components/DataTable";
import { EmptyState, ProblemNotice } from "@/components/Feedback";
import { PageHeader, Panel } from "@/components/Layout";
import { useText } from "@/i18n/useText";

/**
 * The shared shape of Administration lists (Companies, Teams, Departments, Cost Centres,
 * Roles, reference data): a table with Edit per row and Add, editing in a dialog (H4).
 */
export function AdminList<T extends { id: string; name: string; is_active?: boolean }>({
  title,
  intro,
  rows,
  loading,
  error,
  columns,
  canEdit = true,
  name,
  renderEditor,
}: {
  title: string;
  intro: string;
  rows: T[] | undefined;
  loading: boolean;
  error: unknown;
  columns: Column<T>[];
  canEdit?: boolean | undefined;
  /** The name cell (defaults to the plain name). */
  name?: ((row: T) => ReactNode) | undefined;
  renderEditor: (row: T | undefined, close: () => void) => ReactNode;
}) {
  const t = useText();
  const [editing, setEditing] = useState<T | "new" | null>(null);
  const all: Column<T>[] = [
    {
      key: "name",
      header: t("ref.name"),
      cell: (r) => (name ? name(r) : <span className="font-medium">{r.name}</span>),
    },
    ...columns,
    {
      key: "status",
      header: t("ref.status"),
      cell: (r) =>
        r.is_active === false ? (
          <Badge tone="muted">{t("common.inactive")}</Badge>
        ) : (
          <Badge tone="success">{t("common.active")}</Badge>
        ),
    },
  ];
  if (canEdit) {
    all.push({
      key: "edit",
      header: t("common.actions"),
      hideHeader: true,
      className: "w-12 text-right",
      cell: (r) => (
        <IconButton
          size="sm"
          tooltip="top"
          label={t("ref.edit", { name: r.name })}
          icon={<Pencil aria-hidden="true" />}
          onClick={() => setEditing(r)}
        />
      ),
    });
  }
  return (
    <>
      <PageHeader
        level={2}
        title={title}
        description={intro}
        actions={
          canEdit ? (
            <Button
              variant="primary"
              icon={<Plus aria-hidden="true" />}
              onClick={() => setEditing("new")}
            >
              {t("ref.add")}
            </Button>
          ) : undefined
        }
      />
      <ProblemNotice error={error} />
      <Panel flush>
        <DataTable
          caption={title}
          rows={rows}
          loading={loading}
          rowKey={(r) => r.id}
          empty={<EmptyState title={t("ref.empty")} />}
          columns={all}
        />
      </Panel>
      {editing
        ? renderEditor(editing === "new" ? undefined : editing, () => setEditing(null))
        : null}
    </>
  );
}
