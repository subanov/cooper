import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { Mail, UserCheck, UserX } from "lucide-react";
import { useState } from "react";
import { Badge } from "@/components/Badge";
import { Button } from "@/components/Button";
import type { PickerOption } from "@/components/Combobox";
import { LoadingBlock, ProblemNotice } from "@/components/Feedback";
import { FormGrid, SelectField, TextField } from "@/components/Field";
import { BackLink, Panel } from "@/components/Layout";
import { DetailHeader } from "@/components/MasterDetail";
import { RequesterPicker } from "@/features/reference/pickers";
import {
  companiesQuery,
  costCentresQuery,
  namesById,
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

/** One Requester in the detail pane: details, Portal invitation and deactivation (FR-IAM-006, FR-PRT-001). */
export function RequesterPage({ id }: { id?: string | undefined }) {
  const t = useText();
  const query = useQuery({
    queryKey: ["users", "requester", id],
    queryFn: () => call(api.GET("/api/v1/requesters/{id}", { params: { path: { id: id ?? "" } } })),
    enabled: Boolean(id),
  });
  if (!id) return <RequesterView />;
  if (!query.data) {
    return (
      <div className="mx-auto grid max-w-5xl gap-4 p-4 sm:p-6">
        <DetailHeader
          back={<BackToRequesters />}
          title={query.isError ? t("users.unavailable") : t("common.loading")}
        />
        {query.isError ? (
          <ProblemNotice error={query.error} onRetry={() => void query.refetch()} />
        ) : (
          <LoadingBlock lines={8} />
        )}
      </div>
    );
  }
  return <RequesterView key={query.data.updated_at} requester={query.data} />;
}

function BackToRequesters() {
  const t = useText();
  return (
    <BackLink to="/users/requesters" search={true}>
      {t("users.backRequesters")}
    </BackLink>
  );
}

function toForm(r?: Requester) {
  return {
    full_name: r?.full_name ?? "",
    email: r?.email ?? "",
    phone: r?.phone ?? "",
    username: r?.username ?? "",
    company_id: r?.company_id ?? "",
    cost_centre_id: r?.cost_centre_id ?? "",
  };
}

function RequesterView({ requester }: { requester?: Requester }) {
  const t = useText();
  const can = useCan();
  const navigate = useNavigate();
  const companies = useQuery(companiesQuery);
  const costCentres = useQuery(costCentresQuery);
  const manager = useQuery(requestersByIdQuery([requester?.manager_id]));
  const [form, setForm] = useState(() => toForm(requester));
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
        if (!requester)
          void navigate({ to: "/users/requesters/$id", params: { id: r.id }, search: true });
      },
    },
  );
  const err = (field: string) => fieldError(save.error, field);
  const required = (v: string) => (tried && !v.trim() ? t("contracts.required") : undefined);
  const dirty =
    Boolean(requester) &&
    (JSON.stringify(form) !== JSON.stringify(toForm(requester)) || managerPick !== undefined);
  return (
    <article className="mx-auto grid max-w-5xl gap-5 p-4 sm:p-6">
      <DetailHeader
        back={<BackToRequesters />}
        eyebrow={
          requester ? (
            <>
              <span>{requester.email}</span>
              <span aria-hidden="true">·</span>
              <span>{namesById(companies.data).get(requester.company_id) ?? ""}</span>
            </>
          ) : undefined
        }
        title={requester ? requester.full_name : t("users.newRequesterTitle")}
        badges={
          requester ? (
            requester.is_active ? (
              <Badge tone="success">{t("users.active")}</Badge>
            ) : (
              <Badge tone="muted">{t("users.deactivated")}</Badge>
            )
          ) : undefined
        }
      />
      <div
        className={
          requester && editable
            ? "grid gap-5 lg:grid-cols-[minmax(0,1fr)_20rem]"
            : "grid max-w-3xl gap-5"
        }
      >
        <Panel title={t("users.profile")}>
          <RecordForm
            busy={save.isPending}
            error={save.error}
            dirty={dirty}
            disabled={!editable}
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
        </Panel>
        {requester && editable ? <RequesterActions requester={requester} /> : null}
      </div>
    </article>
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
    undo: () => ({ label: t("common.undo"), run: (): void => reactivate.mutate() }),
  });
  return (
    <aside className="grid content-start gap-4" aria-label={t("users.portal")}>
      {requester.is_active ? (
        <Panel title={t("users.portal")} description={t("users.portalIntro")}>
          <Button
            icon={<Mail aria-hidden="true" />}
            loading={invite.isPending}
            onClick={() => invite.mutate()}
          >
            {t("users.sendInvitation")}
          </Button>
        </Panel>
      ) : null}
      <Panel title={t("users.status")}>
        {requester.is_active ? (
          <Button
            variant="danger"
            icon={<UserX aria-hidden="true" />}
            loading={deactivate.isPending}
            onClick={() => deactivate.mutate()}
          >
            {t("users.deactivate")}
          </Button>
        ) : (
          <Button
            icon={<UserCheck aria-hidden="true" />}
            loading={reactivate.isPending}
            onClick={() => reactivate.mutate()}
          >
            {t("users.reactivate")}
          </Button>
        )}
      </Panel>
      <ProblemNotice error={invite.error ?? deactivate.error ?? reactivate.error} />
    </aside>
  );
}
