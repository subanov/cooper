import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { type FormEvent, useState } from "react";
import { Alert } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Field } from "@/components/ui/field";
import { problemText } from "@/features/result";
import {
  createAnalyst,
  resetAnalystTotp,
  setAnalystActive,
  setAnalystCompanyAccess,
  setAnalystPassword,
  setAnalystRoles,
  updateAnalyst,
} from "@/features/users/actions";
import { analystQuery, companiesQuery, rolesQuery } from "@/features/users/queries";
import { useText } from "@/i18n/useText";
import type { Schemas } from "@/lib/api";
import { useFlash } from "./Flash";

type Analyst = Schemas["AnalystDto"];

/** Creates an Analyst (`id` absent) or edits one. */
export function AnalystEditor({ id }: { id?: string | undefined }) {
  const t = useText();
  const existing = useQuery({ ...analystQuery(id ?? ""), enabled: Boolean(id) });
  if (id && existing.isPending) {
    return <p>{t("common.loading")}</p>;
  }
  if (id && !existing.data) {
    return <Alert variant="error">{t("common.error")}</Alert>;
  }
  return (
    <div className="grid max-w-2xl gap-6">
      <DetailsForm key={existing.data?.updated_at ?? "new"} analyst={existing.data} />
      {existing.data ? (
        <>
          <RolesForm key={`roles-${existing.data.updated_at}`} analyst={existing.data} />
          <CompanyAccessForm key={`access-${existing.data.updated_at}`} analyst={existing.data} />
          <SecuritySection analyst={existing.data} />
        </>
      ) : (
        <p className="text-sm text-muted-foreground">{t("analyst.saveFirst")}</p>
      )}
    </div>
  );
}

function useRefresh(id: string | undefined) {
  const queryClient = useQueryClient();
  return async () => {
    await queryClient.invalidateQueries({ queryKey: ["users", "analysts"] });
    if (id) {
      await queryClient.invalidateQueries({ queryKey: analystQuery(id).queryKey });
    }
  };
}

function DetailsForm({ analyst }: { analyst: Analyst | undefined }) {
  const t = useText();
  const navigate = useNavigate();
  const refresh = useRefresh(analyst?.id);
  const flash = useFlash();
  const [username, setUsername] = useState(analyst?.username ?? "");
  const [email, setEmail] = useState(analyst?.email ?? "");
  const [fullName, setFullName] = useState(analyst?.full_name ?? "");
  const [isAdmin, setIsAdmin] = useState(analyst?.is_admin ?? false);
  const [password, setPassword] = useState("");

  async function submit(event: FormEvent) {
    event.preventDefault();
    const body: Schemas["AnalystRequest"] = {
      username: username.trim(),
      email: email.trim(),
      full_name: fullName.trim(),
      is_admin: isAdmin,
      ...(!analyst && password ? { password } : {}),
    };
    const outcome = analyst ? await updateAnalyst(analyst.id, body) : await createAnalyst(body);
    if (!outcome.ok) {
      flash.fail(t("users.failed", { detail: problemText(outcome.problem) }));
      return;
    }
    await refresh();
    if (analyst) {
      flash.ok(t("users.saved"));
    } else {
      await navigate({ to: "/users/analysts/$id", params: { id: outcome.data.id } });
    }
  }

  return (
    <form onSubmit={submit} noValidate className="grid gap-3" aria-labelledby="analyst-details">
      <h2 id="analyst-details" className="text-lg font-semibold">
        {analyst ? analyst.full_name : t("users.newAnalyst")}
        {analyst && !analyst.is_active ? (
          <Badge tone="muted" className="ml-2">
            {t("users.deactivated")}
          </Badge>
        ) : null}
      </h2>
      <Field
        label={t("users.fullName")}
        value={fullName}
        onChange={(e) => setFullName(e.target.value)}
        required
      />
      <Field
        label={t("users.username")}
        value={username}
        onChange={(e) => setUsername(e.target.value)}
        required
      />
      <Field
        label={t("users.email")}
        type="email"
        value={email}
        onChange={(e) => setEmail(e.target.value)}
        required
      />
      {analyst ? null : (
        <Field
          label={t("users.password")}
          type="password"
          autoComplete="new-password"
          hint={t("users.passwordHint")}
          value={password}
          onChange={(e) => setPassword(e.target.value)}
        />
      )}
      <label className="flex items-center gap-2 text-sm">
        <input type="checkbox" checked={isAdmin} onChange={(e) => setIsAdmin(e.target.checked)} />
        {t("users.administrator")}
      </label>
      <div className="flex items-center gap-3">
        <Button type="submit">{analyst ? t("users.save") : t("users.create")}</Button>
        {flash.element}
      </div>
    </form>
  );
}

function RolesForm({ analyst }: { analyst: Analyst }) {
  const t = useText();
  const refresh = useRefresh(analyst.id);
  const flash = useFlash();
  const roles = useQuery(rolesQuery);
  const [selected, setSelected] = useState(new Set(analyst.role_ids));

  async function save(event: FormEvent) {
    event.preventDefault();
    const outcome = await setAnalystRoles(analyst.id, [...selected]);
    if (outcome.ok) {
      await refresh();
      flash.ok(t("users.saved"));
    } else {
      flash.fail(t("users.failed", { detail: problemText(outcome.problem) }));
    }
  }

  return (
    <form onSubmit={save} className="grid gap-2" aria-labelledby="analyst-roles">
      <h2 id="analyst-roles" className="text-lg font-semibold">
        {t("users.roles")}
      </h2>
      {(roles.data ?? []).map((role) => (
        <label key={role.id} className="flex items-start gap-2 text-sm">
          <input
            type="checkbox"
            checked={selected.has(role.id)}
            onChange={(e) => {
              const next = new Set(selected);
              e.target.checked ? next.add(role.id) : next.delete(role.id);
              setSelected(next);
            }}
          />
          <span>
            {role.name}
            {role.description ? (
              <span className="block text-xs text-muted-foreground">{role.description}</span>
            ) : null}
          </span>
        </label>
      ))}
      <div className="flex items-center gap-3">
        <Button type="submit" size="sm">
          {t("users.saveRoles")}
        </Button>
        {flash.element}
      </div>
    </form>
  );
}

function CompanyAccessForm({ analyst }: { analyst: Analyst }) {
  const t = useText();
  const refresh = useRefresh(analyst.id);
  const flash = useFlash();
  const companies = useQuery(companiesQuery);
  const access = analyst.company_access as Schemas["CompanyAccessDto"];
  const [all, setAll] = useState(access.all);
  const [selected, setSelected] = useState(new Set(access.company_ids));

  async function save(event: FormEvent) {
    event.preventDefault();
    const outcome = await setAnalystCompanyAccess(analyst.id, {
      all,
      company_ids: all ? [] : [...selected],
    });
    if (outcome.ok) {
      await refresh();
      flash.ok(t("users.saved"));
    } else {
      flash.fail(t("users.failed", { detail: problemText(outcome.problem) }));
    }
  }

  return (
    <form onSubmit={save} className="grid gap-2" aria-labelledby="analyst-access">
      <h2 id="analyst-access" className="text-lg font-semibold">
        {t("users.companyAccess")}
      </h2>
      <label className="flex items-center gap-2 text-sm">
        <input type="checkbox" checked={all} onChange={(e) => setAll(e.target.checked)} />
        {t("users.allCompanies")}
      </label>
      {all
        ? null
        : (companies.data ?? []).map((company) => (
            <label key={company.id} className="flex items-center gap-2 text-sm">
              <input
                type="checkbox"
                checked={selected.has(company.id)}
                onChange={(e) => {
                  const next = new Set(selected);
                  e.target.checked ? next.add(company.id) : next.delete(company.id);
                  setSelected(next);
                }}
              />
              {company.name}
            </label>
          ))}
      <div className="flex items-center gap-3">
        <Button type="submit" size="sm">
          {t("users.saveAccess")}
        </Button>
        {flash.element}
      </div>
    </form>
  );
}

function SecuritySection({ analyst }: { analyst: Analyst }) {
  const t = useText();
  const refresh = useRefresh(analyst.id);
  const flash = useFlash();
  const [password, setPassword] = useState("");

  async function setPw(event: FormEvent) {
    event.preventDefault();
    const outcome = await setAnalystPassword(analyst.id, password);
    if (outcome.ok) {
      setPassword("");
      await refresh();
      flash.ok(t("users.passwordSet"));
    } else {
      flash.fail(t("users.failed", { detail: problemText(outcome.problem) }));
    }
  }
  async function run(action: () => Promise<{ ok: boolean }>, done: string) {
    const outcome = (await action()) as Awaited<ReturnType<typeof resetAnalystTotp>>;
    if (outcome.ok) {
      await refresh();
      flash.ok(done);
    } else {
      flash.fail(t("users.failed", { detail: problemText(outcome.problem) }));
    }
  }

  return (
    <section aria-labelledby="analyst-security" className="grid gap-3">
      <h2 id="analyst-security" className="text-lg font-semibold">
        {t("users.security")}
      </h2>
      <form onSubmit={setPw} noValidate className="flex flex-wrap items-end gap-2">
        <Field
          label={t("users.newPassword")}
          type="password"
          autoComplete="new-password"
          hint={t("users.passwordHint")}
          value={password}
          onChange={(e) => setPassword(e.target.value)}
        />
        <Button type="submit" size="sm" variant="outline" disabled={!password}>
          {t("users.setPassword")}
        </Button>
      </form>
      <div className="flex flex-wrap items-center gap-2">
        {analyst.totp_enabled ? (
          <Button
            size="sm"
            variant="outline"
            onClick={() => void run(() => resetAnalystTotp(analyst.id), t("users.totpReset"))}
          >
            {t("users.resetTotp")}
          </Button>
        ) : (
          <span className="text-sm text-muted-foreground">{t("users.totpOff")}</span>
        )}
        <Button
          size="sm"
          variant={analyst.is_active ? "destructive" : "outline"}
          onClick={() =>
            void run(
              () => setAnalystActive(analyst.id, !analyst.is_active),
              t(analyst.is_active ? "users.deactivatedDone" : "users.reactivatedDone"),
            )
          }
        >
          {t(analyst.is_active ? "users.deactivate" : "users.reactivate")}
        </Button>
      </div>
      {flash.element}
    </section>
  );
}
