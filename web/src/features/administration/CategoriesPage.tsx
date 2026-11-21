import { useQuery } from "@tanstack/react-query";
import { Pencil, Plus } from "lucide-react";
import { useState } from "react";
import { Badge } from "@/components/Badge";
import { Button, IconButton } from "@/components/Button";
import { CheckboxList } from "@/components/CheckboxList";
import { FormDialog } from "@/components/Dialog";
import { EmptyState, ProblemNotice, QueryState } from "@/components/Feedback";
import { Checkbox, FormGrid, SelectField, TextField } from "@/components/Field";
import { PageHeader, Panel } from "@/components/Layout";
import { categoriesQuery, namesById, ticketLookupQuery } from "@/features/reference/queries";
import { useAction } from "@/features/shared/mutation";
import { useText } from "@/i18n/useText";
import { api, type Schemas } from "@/lib/api";
import { cn } from "@/lib/cn";
import { call } from "@/lib/request";

type Category = Schemas["CategoryDto"];

/** The Category tree (FR-TKT-010): up to three levels, Portal visibility, Ticket Types. */
export function CategoriesPage() {
  const t = useText();
  const categories = useQuery(categoriesQuery);
  const types = useQuery(ticketLookupQuery("ticket-types"));
  const [editing, setEditing] = useState<Category | { parent: string | null } | null>(null);
  const typeNames = namesById(types.data);
  return (
    <>
      <PageHeader
        level={2}
        title={t("admin.ticketCategories")}
        description={t("admin.categories.intro")}
        actions={
          <Button
            variant="primary"
            icon={<Plus aria-hidden="true" />}
            onClick={() => setEditing({ parent: null })}
          >
            {t("ref.add")}
          </Button>
        }
      />
      <QueryState query={categories}>
        {(rows) =>
          rows.length === 0 ? (
            <EmptyState title={t("ref.empty")} />
          ) : (
            <Panel flush>
              <ul aria-label={t("admin.ticketCategories")} className="divide-y divide-line">
                {rows.map((c) => (
                  <li key={c.id} className="flex flex-wrap items-center gap-2 px-4 py-2">
                    <span
                      className={cn(
                        "flex flex-1 flex-wrap items-center gap-2",
                        c.depth === 1 && "pl-6",
                        c.depth >= 2 && "pl-12",
                      )}
                    >
                      {c.depth > 0 ? (
                        <span aria-hidden="true" className="text-fg-muted">
                          └
                        </span>
                      ) : null}
                      <span
                        className={cn("font-medium", !c.is_active && "text-fg-muted line-through")}
                      >
                        {c.name}
                      </span>
                      {!c.is_active ? <Badge tone="muted">{t("common.inactive")}</Badge> : null}
                      {c.portal_visible ? (
                        <Badge tone="accent">{t("admin.categories.onPortal")}</Badge>
                      ) : null}
                      {c.type_ids.length > 0 ? (
                        <span className="text-xs text-fg-muted">
                          {c.type_ids.map((id) => typeNames.get(id) ?? "…").join(", ")}
                        </span>
                      ) : null}
                    </span>
                    {c.depth < 2 ? (
                      <IconButton
                        size="sm"
                        tooltip="top"
                        label={t("admin.categories.addInside", { name: c.name })}
                        icon={<Plus aria-hidden="true" />}
                        onClick={() => setEditing({ parent: c.id })}
                      />
                    ) : null}
                    <IconButton
                      size="sm"
                      tooltip="top"
                      label={t("ref.edit", { name: c.name })}
                      icon={<Pencil aria-hidden="true" />}
                      onClick={() => setEditing(c)}
                    />
                  </li>
                ))}
              </ul>
            </Panel>
          )
        }
      </QueryState>
      {editing ? (
        <CategoryDialog
          category={"id" in editing ? editing : undefined}
          parent={"id" in editing ? (editing.parent_id ?? null) : editing.parent}
          rows={categories.data ?? []}
          onClose={() => setEditing(null)}
        />
      ) : null}
    </>
  );
}

function CategoryDialog({
  category,
  parent,
  rows,
  onClose,
}: {
  category?: Category | undefined;
  parent: string | null;
  rows: Category[];
  onClose: () => void;
}) {
  const t = useText();
  const types = useQuery(ticketLookupQuery("ticket-types"));
  const [form, setForm] = useState({
    name: category?.name ?? "",
    parent_id: parent ?? "",
    sort_order: String(category?.sort_order ?? 0),
    portal_visible: category?.portal_visible ?? false,
    is_active: category?.is_active ?? true,
    type_ids: category?.type_ids ?? [],
  });
  const body = (): Schemas["CategoryRequest"] => ({
    name: form.name.trim(),
    parent_id: form.parent_id || null,
    sort_order: Number(form.sort_order) || 0,
    portal_visible: form.portal_visible,
    is_active: form.is_active,
    type_ids: form.type_ids,
  });
  const save = useAction(
    () =>
      category
        ? call(
            api.PUT("/api/v1/ticket-categories/{id}", {
              params: { path: { id: category.id } },
              body: body(),
            }),
          )
        : call(api.POST("/api/v1/ticket-categories", { body: body() })),
    {
      invalidate: [["reference", "ticket-categories"]],
      success: () => t("ref.saved", { name: form.name.trim() }),
      onSuccess: onClose,
    },
  );
  return (
    <FormDialog
      open
      onOpenChange={(o) => !o && onClose()}
      title={category ? t("ref.edit", { name: category.name }) : t("ref.add")}
      submitLabel={t("common.save")}
      busy={save.isPending}
      submitDisabled={!form.name.trim()}
      onSubmit={() => save.mutate()}
    >
      <ProblemNotice error={save.error} />
      <FormGrid>
        <TextField
          label={t("ref.name")}
          required
          autoFocus
          value={form.name}
          onChange={(e) => setForm((x) => ({ ...x, name: e.target.value }))}
        />
        <TextField
          label={t("ref.order")}
          type="number"
          value={form.sort_order}
          onChange={(e) => setForm((x) => ({ ...x, sort_order: e.target.value }))}
        />
        <SelectField
          label={t("admin.categories.parent")}
          hint={category ? t("admin.categories.parentFixed") : undefined}
          disabled={Boolean(category)}
          value={form.parent_id}
          placeholder={t("ref.topLevel")}
          onChange={(e) => setForm((x) => ({ ...x, parent_id: e.target.value }))}
          options={rows
            .filter((r) => r.depth < 2 && r.id !== category?.id)
            .map((r) => ({ value: r.id, label: `${"› ".repeat(r.depth)}${r.name}` }))}
        />
      </FormGrid>
      <fieldset className="grid gap-2">
        <legend className="mb-1 text-[0.8125rem] font-medium">{t("admin.categories.types")}</legend>
        <CheckboxList
          legend={t("admin.categories.types")}
          options={(types.data ?? [])
            .filter((x) => x.is_active)
            .map((x) => ({ value: x.id, label: x.name }))}
          value={form.type_ids}
          onChange={(type_ids) => setForm((x) => ({ ...x, type_ids }))}
        />
      </fieldset>
      <Checkbox
        label={t("admin.categories.portal")}
        checked={form.portal_visible}
        onChange={(e) => setForm((x) => ({ ...x, portal_visible: e.target.checked }))}
      />
      <Checkbox
        label={t("common.active")}
        checked={form.is_active}
        onChange={(e) => setForm((x) => ({ ...x, is_active: e.target.checked }))}
      />
    </FormDialog>
  );
}
