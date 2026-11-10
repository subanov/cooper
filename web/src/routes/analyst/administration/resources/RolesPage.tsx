import { useQuery } from "@tanstack/react-query";
import { type FormEvent, useState } from "react";
import { Button } from "@/components/ui/button";
import { Field } from "@/components/ui/field";
import {
  capabilitiesQuery,
  createRole,
  deleteRole,
  roleRowsQuery,
  updateRole,
} from "@/features/administration/resources";
import { problemText } from "@/features/result";
import { useText } from "@/i18n/useText";
import type { Schemas } from "@/lib/api";
import { CheckList, FormActions, ResourceAdmin, useFlash, useInvalidateAdmin } from "./shared";

type Role = Schemas["RoleDto"];

/** Roles: named sets of Capabilities. */
export function RolesPage() {
  const t = useText();
  const rows = useQuery(roleRowsQuery);
  return (
    <ResourceAdmin
      title={t("admin.roles")}
      intro={t("admin.roles.intro")}
      rows={rows.data}
      loading={rows.isPending}
      failed={rows.isError}
      getId={(r: Role) => r.id}
      getName={(r) => r.name}
      addLabel={t("admin.roles.add")}
      columns={[
        { header: t("admin.res.name"), cell: (r) => <span className="font-medium">{r.name}</span> },
        { header: t("admin.res.description"), cell: (r) => r.description },
        {
          header: t("admin.roles.capabilities"),
          cell: (r) => t("admin.roles.count", { count: r.capabilities.length }),
        },
      ]}
      renderForm={(row, done) => <RoleForm row={row} done={done} />}
    />
  );
}

function RoleForm({ row, done }: { row: Role | null; done: () => void }) {
  const t = useText();
  const flash = useFlash();
  const invalidate = useInvalidateAdmin();
  const capabilities = useQuery(capabilitiesQuery);
  const [name, setName] = useState(row?.name ?? "");
  const [description, setDescription] = useState(row?.description ?? "");
  const [selected, setSelected] = useState(new Set(row?.capabilities ?? []));

  // Group `module.action` by module so a long list stays scannable.
  const groups = new Map<string, string[]>();
  for (const c of capabilities.data ?? []) {
    const module = c.split(".")[0] ?? c;
    groups.set(module, [...(groups.get(module) ?? []), c]);
  }

  async function submit(event: FormEvent) {
    event.preventDefault();
    const body: Schemas["RoleRequest"] = {
      name: name.trim(),
      description: description.trim(),
      capabilities: [...selected],
    };
    const outcome = row ? await updateRole(row.id, body) : await createRole(body);
    if (!outcome.ok) {
      flash.fail(t("admin.res.failed", { detail: problemText(outcome.problem) }));
      return;
    }
    await invalidate();
    done();
  }

  async function remove() {
    if (!row) {
      return;
    }
    const outcome = await deleteRole(row.id);
    if (!outcome.ok) {
      flash.fail(t("admin.res.failed", { detail: problemText(outcome.problem) }));
      return;
    }
    await invalidate();
    done();
  }

  return (
    <form onSubmit={submit} noValidate className="grid max-w-2xl gap-3">
      <Field
        label={t("admin.res.name")}
        value={name}
        onChange={(e) => setName(e.target.value)}
        required
      />
      <Field
        label={t("admin.res.description")}
        value={description}
        onChange={(e) => setDescription(e.target.value)}
      />
      <div className="grid gap-4 sm:grid-cols-2">
        {[...groups].map(([module, caps]) => (
          <CheckList
            key={module}
            legend={module}
            options={caps.map((c) => ({ id: c, label: c }))}
            selected={selected}
            onChange={(next) => {
              // Keep Capabilities of other groups: only this group's boxes changed.
              const kept = [...selected].filter((c) => !caps.includes(c));
              setSelected(new Set([...kept, ...caps.filter((c) => next.has(c))]));
            }}
          />
        ))}
      </div>
      <FormActions saveLabel={t("admin.save")} onCancel={done} message={flash.element} />
      {row ? (
        <Button
          type="button"
          size="sm"
          variant="destructive"
          className="justify-self-start"
          onClick={() => void remove()}
        >
          {t("admin.roles.delete")}
        </Button>
      ) : null}
    </form>
  );
}
