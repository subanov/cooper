import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { Button } from "@/components/Button";
import { CheckboxList } from "@/components/CheckboxList";
import { ConfirmDialog, FormDialog } from "@/components/Dialog";
import { ProblemNotice } from "@/components/Feedback";
import { TextField } from "@/components/Field";
import { rolesQuery } from "@/features/reference/queries";
import { useAction } from "@/features/shared/mutation";
import { useText } from "@/i18n/useText";
import { api, type Schemas } from "@/lib/api";
import { call } from "@/lib/request";
import { AdminList } from "./OrganisationList";

type Role = Schemas["RoleDto"];

/** Roles: named sets of Capabilities (FR-IAM-005). */
export function RolesPage() {
  const t = useText();
  const roles = useQuery(rolesQuery);
  return (
    <AdminList
      title={t("admin.roles")}
      intro={t("admin.roles.intro")}
      rows={roles.data}
      loading={roles.isPending}
      error={roles.error}
      columns={[
        {
          key: "caps",
          header: t("admin.roles.capabilities"),
          cell: (r) => t("admin.roles.count", { count: r.capabilities.length }),
        },
        {
          key: "description",
          header: t("common.description"),
          cell: (r) => r.description || t("common.noneDash"),
        },
      ]}
      renderEditor={(row, close) => <RoleDialog role={row} onClose={close} />}
    />
  );
}

function RoleDialog({ role, onClose }: { role?: Role | undefined; onClose: () => void }) {
  const t = useText();
  const capabilities = useQuery({
    queryKey: ["reference", "capabilities"],
    queryFn: () => call(api.GET("/api/v1/capabilities")),
    staleTime: 5 * 60_000,
  });
  const [form, setForm] = useState({
    name: role?.name ?? "",
    description: role?.description ?? "",
    capabilities: role?.capabilities ?? [],
  });
  const [confirm, setConfirm] = useState(false);
  const body = (): Schemas["RoleRequest"] => ({ ...form, name: form.name.trim() });
  const save = useAction(
    () =>
      role
        ? call(api.PUT("/api/v1/roles/{id}", { params: { path: { id: role.id } }, body: body() }))
        : call(api.POST("/api/v1/roles", { body: body() })),
    {
      invalidate: [["reference", "roles"]],
      success: () => t("ref.saved", { name: form.name.trim() }),
      onSuccess: onClose,
    },
  );
  const remove = useAction(
    () => call(api.DELETE("/api/v1/roles/{id}", { params: { path: { id: role?.id ?? "" } } })),
    {
      invalidate: [["reference", "roles"]],
      success: "admin.roles.deleted",
      onSuccess: onClose,
    },
  );
  // Capabilities read "<module>.<action>", so sorting groups them by Module (Miller's Law).
  const options = [...(capabilities.data ?? [])].sort().map((c) => ({ value: c, label: c }));
  return (
    <>
      <FormDialog
        open={!confirm}
        onOpenChange={(o) => !o && onClose()}
        title={role ? t("ref.edit", { name: role.name }) : t("ref.add")}
        submitLabel={t("common.save")}
        busy={save.isPending}
        submitDisabled={!form.name.trim()}
        size="lg"
        onSubmit={() => save.mutate()}
      >
        <ProblemNotice error={save.error} />
        <TextField
          label={t("ref.name")}
          required
          autoFocus
          value={form.name}
          onChange={(e) => setForm((x) => ({ ...x, name: e.target.value }))}
        />
        <TextField
          label={t("common.description")}
          value={form.description}
          onChange={(e) => setForm((x) => ({ ...x, description: e.target.value }))}
        />
        <fieldset className="grid gap-2">
          <legend className="mb-1 text-[0.8125rem] font-medium">
            {t("admin.roles.capabilities")}
          </legend>
          <CheckboxList
            legend={t("admin.roles.capabilities")}
            columns={2}
            options={options}
            value={form.capabilities}
            onChange={(caps) => setForm((x) => ({ ...x, capabilities: caps }))}
          />
        </fieldset>
        {role ? (
          <div className="border-t border-line pt-3">
            <Button variant="ghost" className="text-danger-text" onClick={() => setConfirm(true)}>
              {t("admin.roles.delete")}
            </Button>
          </div>
        ) : null}
      </FormDialog>
      {role ? (
        <ConfirmDialog
          open={confirm}
          onOpenChange={setConfirm}
          title={t("admin.roles.deleteTitle", { name: role.name })}
          confirmLabel={t("admin.roles.delete")}
          busy={remove.isPending}
          onConfirm={() => remove.mutate()}
        >
          <p className="text-sm">{t("admin.roles.deleteBody")}</p>
          <ProblemNotice error={remove.error} className="mt-3" />
        </ConfirmDialog>
      ) : null}
    </>
  );
}
