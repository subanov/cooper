import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { KeyRound, ShieldOff, UserCheck, UserX } from "lucide-react";
import { useState } from "react";
import { Badge } from "@/components/Badge";
import { Button } from "@/components/Button";
import { CheckboxList } from "@/components/CheckboxList";
import { ConfirmDialog } from "@/components/Dialog";
import { LoadingBlock, Notice, ProblemNotice } from "@/components/Feedback";
import { Checkbox, TextField } from "@/components/Field";
import { BackLink, Panel } from "@/components/Layout";
import { DetailHeader } from "@/components/MasterDetail";
import { rolesQuery } from "@/features/reference/queries";
import { useSession } from "@/features/session/session";
import { fieldError, useAction } from "@/features/shared/mutation";
import { RecordForm } from "@/features/shared/RecordForm";
import { useText } from "@/i18n/useText";
import { api, type Schemas } from "@/lib/api";
import { call } from "@/lib/request";
import { CompanyAccessEditor } from "./CompanyAccessEditor";

type Analyst = Schemas["AnalystDto"];

/** One Analyst in the detail pane: profile, Roles, Company access and security (FR-IAM-001, FR-IAM-005). */
export function AnalystPage({ id }: { id?: string | undefined }) {
  const t = useText();
  const query = useQuery({
    queryKey: ["users", "analyst", id],
    queryFn: () => call(api.GET("/api/v1/analysts/{id}", { params: { path: { id: id ?? "" } } })),
    enabled: Boolean(id),
  });
  if (!id) return <AnalystView />;
  if (!query.data) {
    return (
      <div className="mx-auto grid max-w-5xl gap-4 p-4 sm:p-6">
        <DetailHeader
          back={<BackToAnalysts />}
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
  return <AnalystView key={query.data.updated_at} analyst={query.data} />;
}

function BackToAnalysts() {
  const t = useText();
  return (
    <BackLink to="/users/analysts" search={true}>
      {t("users.back")}
    </BackLink>
  );
}

function AnalystView({ analyst }: { analyst?: Analyst }) {
  const t = useText();
  const session = useSession();
  const navigate = useNavigate();
  const [form, setForm] = useState({
    full_name: analyst?.full_name ?? "",
    username: analyst?.username ?? "",
    email: analyst?.email ?? "",
    is_admin: analyst?.is_admin ?? false,
    password: "",
  });
  const [tried, setTried] = useState(false);
  const set =
    (key: "full_name" | "username" | "email" | "password") => (e: { target: { value: string } }) =>
      setForm((x) => ({ ...x, [key]: e.target.value }));
  const valid = form.full_name.trim() && form.username.trim() && form.email.trim();
  const body = (): Schemas["AnalystRequest"] => ({
    full_name: form.full_name.trim(),
    username: form.username.trim(),
    email: form.email.trim(),
    is_admin: form.is_admin,
    ...(analyst ? {} : { password: form.password || null }),
  });
  const save = useAction(
    () =>
      analyst
        ? call(
            api.PUT("/api/v1/analysts/{id}", {
              params: { path: { id: analyst.id } },
              body: body(),
            }),
          )
        : call(api.POST("/api/v1/analysts", { body: body() })),
    {
      invalidate: [["users"], ["reference", "analysts"]],
      success: (a) => (analyst ? t("users.saved") : t("users.created", { name: a.full_name })),
      onSuccess: (a) => {
        if (!analyst)
          void navigate({ to: "/users/analysts/$id", params: { id: a.id }, search: true });
      },
    },
  );
  const err = (field: string) => fieldError(save.error, field);
  const required = (v: string) => (tried && !v.trim() ? t("contracts.required") : undefined);
  const dirty =
    Boolean(analyst) &&
    (form.full_name !== analyst?.full_name ||
      form.username !== analyst?.username ||
      form.email !== analyst?.email ||
      form.is_admin !== analyst?.is_admin);
  return (
    <article className="mx-auto grid max-w-5xl gap-5 p-4 sm:p-6">
      <DetailHeader
        back={<BackToAnalysts />}
        eyebrow={analyst ? <span>{analyst.email}</span> : undefined}
        title={analyst ? analyst.full_name : t("users.newAnalystTitle")}
        badges={
          analyst ? (
            <>
              {analyst.is_active ? (
                <Badge tone="success">{t("users.active")}</Badge>
              ) : (
                <Badge tone="muted">{t("users.deactivated")}</Badge>
              )}
              {analyst.is_admin ? <Badge tone="accent">{t("users.administrator")}</Badge> : null}
              <Badge tone="neutral">
                {t("users.twoFactor")}: {analyst.totp_enabled ? t("common.on") : t("common.off")}
              </Badge>
            </>
          ) : undefined
        }
      />
      <div className="grid gap-5 lg:grid-cols-2">
        <Panel title={t("users.profile")}>
          <RecordForm
            busy={save.isPending}
            error={save.error}
            dirty={dirty}
            submitLabel={analyst ? t("common.saveChanges") : t("common.create")}
            onSubmit={() => {
              setTried(true);
              if (valid) save.mutate();
            }}
          >
            <TextField
              label={t("users.fullName")}
              required
              autoComplete="off"
              value={form.full_name}
              error={required(form.full_name) ?? err("full_name")}
              onChange={set("full_name")}
            />
            <TextField
              label={t("users.username")}
              required
              autoComplete="off"
              value={form.username}
              error={required(form.username) ?? err("username")}
              onChange={set("username")}
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
            {!analyst ? (
              <TextField
                label={t("users.password")}
                hint={t("users.passwordHint")}
                type="password"
                autoComplete="new-password"
                value={form.password}
                error={err("password")}
                onChange={set("password")}
              />
            ) : null}
            {session.is_admin ? (
              <Checkbox
                label={t("users.administrator")}
                hint={t("users.administratorHint")}
                checked={form.is_admin}
                onChange={(e) => setForm((x) => ({ ...x, is_admin: e.target.checked }))}
              />
            ) : null}
          </RecordForm>
        </Panel>
        {analyst ? (
          <>
            <RolesPanel analyst={analyst} />
            <Panel title={t("users.companyAccess")} description={t("users.companyAccessHint")}>
              <AccessForm analyst={analyst} />
            </Panel>
            <SecurityPanel analyst={analyst} />
          </>
        ) : (
          <Notice tone="info" live={false}>
            {t("users.saveFirst")}
          </Notice>
        )}
      </div>
    </article>
  );
}

function RolesPanel({ analyst }: { analyst: Analyst }) {
  const t = useText();
  const roles = useQuery(rolesQuery);
  const [ids, setIds] = useState(analyst.role_ids);
  const save = useAction(
    () =>
      call(
        api.PUT("/api/v1/analysts/{id}/roles", {
          params: { path: { id: analyst.id } },
          body: { role_ids: ids },
        }),
      ),
    {
      invalidate: [["users"]],
      success: "users.rolesSaved",
    },
  );
  return (
    <Panel title={t("users.roles")} description={t("users.rolesHint")}>
      <form
        className="grid gap-3"
        onSubmit={(e) => {
          e.preventDefault();
          save.mutate();
        }}
      >
        <CheckboxList
          legend={t("users.roles")}
          options={(roles.data ?? []).map((r) => ({
            value: r.id,
            label: r.name,
            description: r.description || undefined,
          }))}
          value={ids}
          onChange={setIds}
        />
        <ProblemNotice error={save.error} />
        <div>
          <Button
            type="submit"
            loading={save.isPending}
            disabled={
              JSON.stringify([...ids].sort()) === JSON.stringify([...analyst.role_ids].sort())
            }
          >
            {t("users.saveRoles")}
          </Button>
        </div>
      </form>
    </Panel>
  );
}

function AccessForm({ analyst }: { analyst: Analyst }) {
  const save = useAction(
    (access: Schemas["CompanyAccessDto"]) =>
      call(
        api.PUT("/api/v1/analysts/{id}/company-access", {
          params: { path: { id: analyst.id } },
          body: access,
        }),
      ),
    { invalidate: [["users"]], success: "users.accessSaved" },
  );
  return (
    <>
      <CompanyAccessEditor
        value={analyst.company_access}
        busy={save.isPending}
        onSave={(a) => save.mutate(a)}
      />
      <ProblemNotice error={save.error} className="mt-3" />
    </>
  );
}

function SecurityPanel({ analyst }: { analyst: Analyst }) {
  const t = useText();
  const session = useSession();
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState(false);
  const path = { params: { path: { id: analyst.id } } };
  const setPw = useAction(
    () => call(api.PUT("/api/v1/analysts/{id}/password", { ...path, body: { password } })),
    {
      success: "users.passwordSet",
      onSuccess: () => setPassword(""),
    },
  );
  const resetTotp = useAction(() => call(api.POST("/api/v1/analysts/{id}/totp/reset", path)), {
    invalidate: [["users"]],
    success: "users.totpReset",
    onSuccess: () => setConfirm(false),
  });
  const reactivate = useAction(() => call(api.POST("/api/v1/analysts/{id}/reactivate", path)), {
    invalidate: [["users"], ["reference", "analysts"]],
    success: () => t("users.reactivatedDone", { name: analyst.full_name }),
  });
  // Reversible, so no confirmation: the toast offers Undo (H3).
  const deactivate = useAction(() => call(api.POST("/api/v1/analysts/{id}/deactivate", path)), {
    invalidate: [["users"], ["reference", "analysts"]],
    success: () => t("users.deactivatedDone", { name: analyst.full_name }),
    undo: () => ({ label: t("common.undo"), run: (): void => reactivate.mutate() }),
  });
  const self = analyst.id === session.analyst.id;
  return (
    <Panel title={t("users.security")}>
      <div className="grid gap-5">
        <form
          className="flex flex-wrap items-start gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            if (password.length >= 12) setPw.mutate();
          }}
        >
          <TextField
            className="min-w-56 flex-1"
            label={t("users.newPassword")}
            hint={t("reset.passwordHint")}
            type="password"
            autoComplete="new-password"
            value={password}
            error={fieldError(setPw.error, "password")}
            onChange={(e) => setPassword(e.target.value)}
          />
          <Button
            type="submit"
            icon={<KeyRound aria-hidden="true" />}
            loading={setPw.isPending}
            disabled={password.length < 12}
            className="mt-6"
          >
            {t("users.setPassword")}
          </Button>
        </form>
        <div className="flex flex-wrap items-center gap-3">
          <p className="text-sm">{analyst.totp_enabled ? t("users.totpOn") : t("users.totpOff")}</p>
          {session.is_admin && analyst.totp_enabled ? (
            <Button icon={<ShieldOff aria-hidden="true" />} onClick={() => setConfirm(true)}>
              {t("users.resetTotp")}
            </Button>
          ) : null}
        </div>
        <ProblemNotice error={setPw.error ?? reactivate.error ?? deactivate.error} />
        {!self ? (
          <div className="border-t border-line pt-4">
            {analyst.is_active ? (
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
          </div>
        ) : null}
      </div>
      <ConfirmDialog
        open={confirm}
        onOpenChange={setConfirm}
        title={t("users.resetTotpTitle", { name: analyst.full_name })}
        confirmLabel={t("users.resetTotp")}
        busy={resetTotp.isPending}
        onConfirm={() => resetTotp.mutate()}
      >
        <p className="text-sm">{t("users.resetTotpBody")}</p>
        <ProblemNotice error={resetTotp.error} className="mt-3" />
      </ConfirmDialog>
    </Panel>
  );
}
