import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { Mail, UserCheck, UserX } from "lucide-react";
import { useState } from "react";
import { Badge } from "@/components/Badge";
import { Button } from "@/components/Button";
import { Card } from "@/components/Card";
import type { PickerOption } from "@/components/Combobox";
import { FormGrid, SelectField, TextField } from "@/components/Field";
import { ProblemNotice } from "@/components/Notice";
import { SectionHeader } from "@/components/Page";
import { LoadingBlock, QueryState } from "@/components/QueryState";
import { RequesterPicker } from "@/features/reference/pickers";
import {
  companiesQuery,
  costCentresQuery,
  requesterOption,
  requestersByIdQuery,
} from "@/features/reference/queries";
import { useCan } from "@/features/session/session";
import { fieldError, orNull, useAction } from "@/features/shared/mutation";
import { RecordForm } from "@/features/shared/RecordForm";
import { useText } from "@/i18n/useText";
import { api, type Schemas } from "@/lib/api";
import { call } from "@/lib/request";

type Requester = Schemas["RequesterDto"];

/** One Requester: details, Portal invitation and deactivation (FR-IAM-006, FR-PRT-001). */
export function RequesterPage({ id }: { id?: string }) {
  const query = useQuery({
    queryKey: ["users", "requester", id],
    queryFn: () => call(api.GET("/api/v1/requesters/{id}", { params: { path: { id: id ?? "" } } })),
    enabled: Boolean(id),
  });
  if (!id) return <RequesterView />;
  return (
    <QueryState query={query} loading={<LoadingBlock lines={8} />}>
      {(r) => <RequesterView key={r.updated_at} requester={r} />}
    </QueryState>
  );
}

function RequesterView({ requester }: { requester?: Requester }) {
  const t = useText();
  const can = useCan();
  const navigate = useNavigate();
  const companies = useQuery(companiesQuery);
  const costCentres = useQuery(costCentresQuery);
  const manager = useQuery(requestersByIdQuery([requester?.manager_id]));
  const [form, setForm] = useState({
    full_name: requester?.full_name ?? "",
    email: requester?.email ?? "",
    phone: requester?.phone ?? "",
    username: requester?.username ?? "",
    company_id: requester?.company_id ?? "",
    cost_centre_id: requester?.cost_centre_id ?? "",
  });
  const [managerPick, setManagerPick] = useState<PickerOption | null | undefined>(undefined);
  const [tried, setTried] = useState(false);
  const set = (key: keyof typeof form) => (e: { target: { value: string } }) =>
    setForm((x) => ({ ...x, [key]: e.target.value }));
  const currentManager = requester?.manager_id
    ? manager.data?.get(requester.manager_id)
    : undefined;
  const shownManager =
    managerPick === undefined
      ? currentManager
        ? requesterOption(currentManager)
        : null
      : managerPick;
  const editable = can("requesters.manage");
  const activeCompanies = (companies.data ?? []).filter((c) => c.is_active);
  const company =
    form.company_id ||
    activeCompanies.find((c) => c.is_default)?.id ||
    activeCompanies[0]?.id ||
    "";
  const body = (): Schemas["RequesterRequest"] => ({
    full_name: form.full_name.trim(),
    email: form.email.trim(),
    phone: orNull(form.phone),
    username: orNull(form.username),
    company_id: company || null,
    cost_centre_id: form.cost_centre_id || null,
    manager_id:
      managerPick === undefined ? (requester?.manager_id ?? null) : (managerPick?.id ?? null),
  });
  const save = useAction(
    () =>
      requester
        ? call(
            api.PUT("/api/v1/requesters/{id}", {
              params: { path: { id: requester.id } },
              body: body(),
            }),
          )
        : call(api.POST("/api/v1/requesters", { body: body() })),
    {
      invalidate: [["users"], ["reference", "requesters"]],
      success: (r) => (requester ? t("users.saved") : t("users.created", { name: r.full_name })),
      onSuccess: (r) => {
        if (!requester) void navigate({ to: "/users/requesters/$id", params: { id: r.id } });
      },
    },
  );
  const err = (field: string) => fieldError(save.error, field);
  const required = (v: string) => (tried && !v.trim() ? t("contracts.required") : undefined);
  return (
    <div className="grid gap-4">
      <SectionHeader
        back={{ to: "/users/requesters", label: t("users.backRequesters") }}
        title={requester ? requester.full_name : t("users.newRequesterTitle")}
        description={
          requester ? (
            requester.is_active ? (
              <Badge tone="success">{t("users.active")}</Badge>
            ) : (
              <Badge tone="muted">{t("users.deactivated")}</Badge>
            )
          ) : undefined
        }
      />
      <div className="grid gap-4 lg:grid-cols-[minmax(0,1fr)_22rem]">
        <Card title={t("users.profile")}>
          <fieldset disabled={!editable} className="contents">
            <RecordForm
              busy={save.isPending}
              error={save.error}
              dirty={false}
              submitLabel={requester ? t("common.saveChanges") : t("common.create")}
              onSubmit={() => {
                setTried(true);
                if (form.full_name.trim() && form.email.trim()) save.mutate();
              }}
            >
              <FormGrid>
                <TextField
                  label={t("users.fullName")}
                  required
                  autoComplete="off"
                  value={form.full_name}
                  error={required(form.full_name) ?? err("full_name")}
                  onChange={set("full_name")}
                />
                <TextField
                  label={t("users.email")}
                  type="email"
                  required
                  autoComplete="off"
                  value={form.email}
                  error={required(form.email) ?? err("email")}
                  onChange={set("email")}
                />
                <TextField
                  label={t("users.phone")}
                  type="tel"
                  value={form.phone}
                  onChange={set("phone")}
                />
                <TextField
                  label={t("users.username")}
                  value={form.username}
                  onChange={set("username")}
                />
                {activeCompanies.length > 1 ? (
                  <SelectField
                    label={t("users.company")}
                    value={company}
                    onChange={set("company_id")}
                    options={activeCompanies.map((c) => ({ value: c.id, label: c.name }))}
                  />
                ) : null}
                <SelectField
                  label={t("users.costCentre")}
                  value={form.cost_centre_id}
                  placeholder={t("common.none")}
                  onChange={set("cost_centre_id")}
                  options={(costCentres.data ?? [])
                    .filter(
                      (c) =>
                        (c.is_active && c.company_id === company) ||
                        c.id === requester?.cost_centre_id,
                    )
                    .map((c) => ({ value: c.id, label: `${c.code} · ${c.name}` }))}
                />
              </FormGrid>
              <RequesterPicker
                label={t("users.manager")}
                hint={t("users.managerHint")}
                companyId={company || undefined}
                value={shownManager}
                onChange={setManagerPick}
              />
            </RecordForm>
          </fieldset>
        </Card>
        {requester && editable ? <RequesterActions requester={requester} /> : null}
      </div>
    </div>
  );
}

function RequesterActions({ requester }: { requester: Requester }) {
  const t = useText();
  const path = { params: { path: { id: requester.id } } };
  const invite = useAction(
    () => call(api.POST("/api/v1/requesters/{id}/portal-invitation", path)),
    {
      success: () => t("users.invitationSent", { email: requester.email }),
    },
  );
  const reactivate = useAction(() => call(api.POST("/api/v1/requesters/{id}/reactivate", path)), {
    invalidate: [["users"]],
    success: () => t("users.reactivatedDone", { name: requester.full_name }),
  });
  const deactivate = useAction(() => call(api.POST("/api/v1/requesters/{id}/deactivate", path)), {
    invalidate: [["users"]],
    success: () => t("users.deactivatedDone", { name: requester.full_name }),
    undo: () => ({ label: t("common.undo"), run: () => reactivate.mutate() }),
  });
  return (
    <aside className="grid content-start gap-4">
      {requester.is_active ? (
        <Card title={t("users.portal")} description={t("users.portalIntro")}>
          <Button
            icon={<Mail aria-hidden="true" className="size-4" />}
            loading={invite.isPending}
            onClick={() => invite.mutate()}
          >
            {t("users.sendInvitation")}
          </Button>
        </Card>
      ) : null}
      <Card>
        {requester.is_active ? (
          <Button
            variant="danger"
            icon={<UserX aria-hidden="true" className="size-4" />}
            loading={deactivate.isPending}
            onClick={() => deactivate.mutate()}
          >
            {t("users.deactivate")}
          </Button>
        ) : (
          <Button
            icon={<UserCheck aria-hidden="true" className="size-4" />}
            loading={reactivate.isPending}
            onClick={() => reactivate.mutate()}
          >
            {t("users.reactivate")}
          </Button>
        )}
      </Card>
      <ProblemNotice error={invite.error ?? deactivate.error ?? reactivate.error} />
    </aside>
  );
}
