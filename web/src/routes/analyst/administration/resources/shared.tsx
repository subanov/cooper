import { useQueryClient } from "@tanstack/react-query";
import { Fragment, type ReactNode, useState } from "react";
import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { useText } from "@/i18n/useText";
import { useFlash } from "../../users/Flash";

/** Refreshes everything an admin change can affect (lists here, pickers elsewhere). */
export function useInvalidateAdmin() {
  const queryClient = useQueryClient();
  return async () => {
    await Promise.all(
      [
        "administration",
        "lookups",
        "users",
        "service-status",
        "morning-checks",
        "tasks",
        "knowledge",
        "assets",
      ].map((key) => queryClient.invalidateQueries({ queryKey: [key] })),
    );
  };
}

export { useFlash };

export interface Column<T> {
  header: string;
  cell: (row: T) => ReactNode;
}

interface ResourceAdminProps<T> {
  /** Visible heading. */
  title: string;
  /** Intro under the heading. */
  intro?: string;
  rows: readonly T[] | undefined;
  loading: boolean;
  failed: boolean;
  getId: (row: T) => string;
  getName: (row: T) => string;
  columns: readonly Column<T>[];
  addLabel: string;
  /** The form for a new row (`null`) or an existing one; `done` closes it. */
  renderForm: (row: T | null, done: () => void) => ReactNode;
}

/** A table of rows with an Add button and an inline edit form under the chosen row. */
export function ResourceAdmin<T>(props: ResourceAdminProps<T>) {
  const t = useText();
  const [editing, setEditing] = useState<string | null>(null);
  const done = () => setEditing(null);
  const colSpan = props.columns.length + 1;
  return (
    <div className="grid gap-3">
      <div className="flex items-center gap-3">
        <h2 className="text-lg font-semibold">{props.title}</h2>
        <Button className="ml-auto" size="sm" onClick={() => setEditing("new")}>
          {props.addLabel}
        </Button>
      </div>
      {props.intro ? <p className="text-muted-foreground">{props.intro}</p> : null}
      {props.failed ? <Alert variant="error">{t("common.error")}</Alert> : null}
      {props.loading ? <p>{t("common.loading")}</p> : null}
      {editing === "new" ? (
        <div className="rounded-lg border border-border p-4">{props.renderForm(null, done)}</div>
      ) : null}
      {props.rows && props.rows.length === 0 ? (
        <p className="text-muted-foreground">{t("admin.res.empty")}</p>
      ) : null}
      {props.rows && props.rows.length > 0 ? (
        <table className="w-full text-left text-sm">
          <caption className="sr-only">{props.title}</caption>
          <thead>
            <tr className="border-b border-border">
              {props.columns.map((c) => (
                <th key={c.header} scope="col" className="py-2 pr-3">
                  {c.header}
                </th>
              ))}
              <td className="py-2">
                <span className="sr-only">{t("admin.res.actions")}</span>
              </td>
            </tr>
          </thead>
          <tbody>
            {props.rows.map((row) => {
              const id = props.getId(row);
              return (
                <Fragment key={id}>
                  <tr className="border-b border-border align-top">
                    {props.columns.map((c) => (
                      <td key={c.header} className="py-2 pr-3">
                        {c.cell(row)}
                      </td>
                    ))}
                    <td className="py-2 text-right">
                      <Button
                        size="sm"
                        variant="ghost"
                        aria-expanded={editing === id}
                        aria-label={`${t("admin.res.edit")} ${props.getName(row)}`}
                        onClick={() => setEditing(editing === id ? null : id)}
                      >
                        {t("admin.res.edit")}
                      </Button>
                    </td>
                  </tr>
                  {editing === id ? (
                    <tr className="border-b border-border">
                      <td colSpan={colSpan} className="p-4">
                        {props.renderForm(row, done)}
                      </td>
                    </tr>
                  ) : null}
                </Fragment>
              );
            })}
          </tbody>
        </table>
      ) : null}
    </div>
  );
}

interface CheckListProps {
  legend: string;
  options: readonly { id: string; label: string; hint?: string | undefined }[];
  selected: ReadonlySet<string>;
  onChange: (next: Set<string>) => void;
}

/** A group of checkboxes for choosing several things. */
export function CheckList({ legend, options, selected, onChange }: CheckListProps) {
  return (
    <fieldset className="grid gap-1.5">
      <legend className="mb-1 text-sm font-medium">{legend}</legend>
      {options.map((o) => (
        <label key={o.id} className="flex items-start gap-2 text-sm">
          <input
            type="checkbox"
            checked={selected.has(o.id)}
            onChange={(e) => {
              const next = new Set(selected);
              if (e.target.checked) {
                next.add(o.id);
              } else {
                next.delete(o.id);
              }
              onChange(next);
            }}
          />
          <span>
            {o.label}
            {o.hint ? <span className="block text-xs text-muted-foreground">{o.hint}</span> : null}
          </span>
        </label>
      ))}
    </fieldset>
  );
}

/** A single labelled checkbox. */
export function CheckField(props: {
  label: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
}) {
  return (
    <label className="flex items-center gap-2 text-sm">
      <input
        type="checkbox"
        checked={props.checked}
        onChange={(e) => props.onChange(e.target.checked)}
      />
      {props.label}
    </label>
  );
}

/** Save and Cancel buttons with the result message. */
export function FormActions(props: {
  saveLabel: string;
  onCancel: () => void;
  message: ReactNode;
}) {
  const t = useText();
  return (
    <div className="flex items-center gap-3">
      <Button type="submit" size="sm">
        {props.saveLabel}
      </Button>
      <Button type="button" size="sm" variant="ghost" onClick={props.onCancel}>
        {t("admin.res.cancel")}
      </Button>
      {props.message}
    </div>
  );
}
