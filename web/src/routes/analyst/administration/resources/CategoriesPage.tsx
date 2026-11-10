import { useQuery } from "@tanstack/react-query";
import { type FormEvent, useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Field } from "@/components/ui/field";
import { SelectField } from "@/components/ui/select-field";
import { createCategory, updateCategory } from "@/features/administration/resources";
import { problemText } from "@/features/result";
import { categoriesQuery, typesQuery } from "@/features/tickets/queries";
import { useText } from "@/i18n/useText";
import type { Schemas } from "@/lib/api";
import {
  CheckField,
  CheckList,
  FormActions,
  ResourceAdmin,
  useFlash,
  useInvalidateAdmin,
} from "./shared";

type Category = Schemas["CategoryDto"];

/** Ticket Categories: a tree, optionally limited to some Ticket Types. */
export function CategoriesPage() {
  const t = useText();
  const rows = useQuery(categoriesQuery);
  return (
    <ResourceAdmin
      title={t("admin.categories")}
      intro={t("admin.categories.intro")}
      rows={rows.data}
      loading={rows.isPending}
      failed={rows.isError}
      getId={(r: Category) => r.id}
      getName={(r) => r.name}
      addLabel={t("admin.categories.add")}
      columns={[
        {
          header: t("admin.res.name"),
          cell: (r) => (
            <span className="font-medium" style={{ paddingLeft: `${r.depth * 1.25}rem` }}>
              {r.name}
            </span>
          ),
        },
        {
          header: t("admin.categories.portal"),
          cell: (r) => (r.portal_visible ? <Badge>{t("admin.categories.portal")}</Badge> : null),
        },
        {
          header: t("admin.res.status"),
          cell: (r) => (
            <Badge tone={r.is_active ? "ok" : "muted"}>
              {t(r.is_active ? "admin.res.active" : "admin.res.inactive")}
            </Badge>
          ),
        },
      ]}
      renderForm={(row, done) => <CategoryForm row={row} all={rows.data ?? []} done={done} />}
    />
  );
}

function CategoryForm({
  row,
  all,
  done,
}: {
  row: Category | null;
  all: Category[];
  done: () => void;
}) {
  const t = useText();
  const flash = useFlash();
  const invalidate = useInvalidateAdmin();
  const types = useQuery(typesQuery);
  const [name, setName] = useState(row?.name ?? "");
  const [parent, setParent] = useState(row?.parent_id ?? "");
  const [order, setOrder] = useState(String(row?.sort_order ?? 0));
  const [portal, setPortal] = useState(row?.portal_visible ?? false);
  const [active, setActive] = useState(row?.is_active ?? true);
  const [typeIds, setTypeIds] = useState(new Set(row?.type_ids ?? []));

  async function submit(event: FormEvent) {
    event.preventDefault();
    const body: Schemas["CategoryRequest"] = {
      name: name.trim(),
      sort_order: Number(order) || 0,
      portal_visible: portal,
      is_active: active,
      type_ids: [...typeIds],
      ...(!row && parent ? { parent_id: parent } : {}),
    };
    const outcome = row ? await updateCategory(row.id, body) : await createCategory(body);
    if (!outcome.ok) {
      flash.fail(t("admin.res.failed", { detail: problemText(outcome.problem) }));
      return;
    }
    await invalidate();
    done();
  }

  return (
    <form onSubmit={submit} noValidate className="grid max-w-xl gap-3">
      <Field
        label={t("admin.res.name")}
        value={name}
        onChange={(e) => setName(e.target.value)}
        required
      />
      {row ? null : (
        <SelectField
          label={t("admin.categories.parent")}
          value={parent}
          onChange={(e) => setParent(e.target.value)}
        >
          <option value="">{t("admin.categories.noParent")}</option>
          {all.map((c) => (
            <option key={c.id} value={c.id}>
              {`${"– ".repeat(c.depth)}${c.name}`}
            </option>
          ))}
        </SelectField>
      )}
      <Field
        label={t("admin.res.order")}
        type="number"
        value={order}
        onChange={(e) => setOrder(e.target.value)}
      />
      <CheckField
        label={t("admin.categories.portalVisible")}
        checked={portal}
        onChange={setPortal}
      />
      <CheckField label={t("admin.res.active")} checked={active} onChange={setActive} />
      <CheckList
        legend={t("admin.categories.types")}
        options={(types.data ?? []).map((x) => ({ id: x.id, label: x.name }))}
        selected={typeIds}
        onChange={setTypeIds}
      />
      <FormActions saveLabel={t("admin.save")} onCancel={done} message={flash.element} />
    </form>
  );
}
