import { useQuery } from "@tanstack/react-query";
import { type FormEvent, useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Field } from "@/components/ui/field";
import {
  createTeam,
  roleRowsQuery,
  setTeamCompanyAccess,
  setTeamMembers,
  setTeamRoles,
  teamRowsQuery,
  updateTeam,
} from "@/features/administration/resources";
import { problemText } from "@/features/result";
import { analystsQuery } from "@/features/tickets/queries";
import { companiesQuery } from "@/features/users/queries";
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

type Team = Schemas["TeamDto"];

/** Teams (Queues): members, Roles and Company access. */
export function TeamsPage() {
  const t = useText();
  const rows = useQuery(teamRowsQuery);
  return (
    <ResourceAdmin
      title={t("admin.teams")}
      intro={t("admin.teams.intro")}
      rows={rows.data}
      loading={rows.isPending}
      failed={rows.isError}
      getId={(r: Team) => r.id}
      getName={(r) => r.name}
      addLabel={t("admin.teams.add")}
      columns={[
        { header: t("admin.res.name"), cell: (r) => <span className="font-medium">{r.name}</span> },
        { header: t("admin.res.description"), cell: (r) => r.description },
        {
          header: t("admin.teams.members"),
          cell: (r) => t("admin.teams.memberCount", { count: r.analyst_ids.length }),
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
      renderForm={(row, done) => <TeamForm row={row} done={done} />}
    />
  );
}

function TeamForm({ row, done }: { row: Team | null; done: () => void }) {
  const t = useText();
  const flash = useFlash();
  const invalidate = useInvalidateAdmin();
  const [name, setName] = useState(row?.name ?? "");
  const [description, setDescription] = useState(row?.description ?? "");
  const [active, setActive] = useState(row?.is_active ?? true);

  async function submit(event: FormEvent) {
    event.preventDefault();
    const body: Schemas["TeamRequest"] = {
      name: name.trim(),
      description: description.trim(),
      is_active: active,
    };
    const outcome = row ? await updateTeam(row.id, body) : await createTeam(body);
    if (!outcome.ok) {
      flash.fail(t("admin.res.failed", { detail: problemText(outcome.problem) }));
      return;
    }
    await invalidate();
    if (!row) {
      done();
    } else {
      flash.ok(t("admin.saved"));
    }
  }

  return (
    <div className="grid max-w-2xl gap-5">
      <form onSubmit={submit} noValidate className="grid gap-3">
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
        <CheckField label={t("admin.res.active")} checked={active} onChange={setActive} />
        <FormActions saveLabel={t("admin.save")} onCancel={done} message={flash.element} />
      </form>
      {row ? (
        <>
          <MembersForm team={row} />
          <TeamRolesForm team={row} />
          <TeamAccessForm team={row} />
        </>
      ) : (
        <p className="text-sm text-muted-foreground">{t("admin.teams.saveFirst")}</p>
      )}
    </div>
  );
}

function SaveSection(props: {
  title: string;
  saveLabel: string;
  onSave: () => Promise<{ ok: true } | { ok: false; detail: string }>;
  children: React.ReactNode;
}) {
  const t = useText();
  const flash = useFlash();
  const invalidate = useInvalidateAdmin();
  async function submit(event: FormEvent) {
    event.preventDefault();
    const outcome = await props.onSave();
    if (outcome.ok) {
      await invalidate();
      flash.ok(t("admin.saved"));
    } else {
      flash.fail(t("admin.res.failed", { detail: outcome.detail }));
    }
  }
  return (
    <form onSubmit={submit} className="grid gap-2">
      <h3 className="font-semibold">{props.title}</h3>
      {props.children}
      <div className="flex items-center gap-3">
        <Button type="submit" size="sm" variant="outline">
          {props.saveLabel}
        </Button>
        {flash.element}
      </div>
    </form>
  );
}

const done = <T,>(
  r: { ok: true; data: T } | { ok: false; problem: { detail: string | undefined; kind: string } },
) => (r.ok ? ({ ok: true } as const) : ({ ok: false, detail: problemText(r.problem) } as const));

function MembersForm({ team }: { team: Team }) {
  const t = useText();
  const analysts = useQuery(analystsQuery);
  const [selected, setSelected] = useState(new Set(team.analyst_ids));
  return (
    <SaveSection
      title={t("admin.teams.members")}
      saveLabel={t("admin.teams.saveMembers")}
      onSave={async () => done(await setTeamMembers(team.id, [...selected]))}
    >
      <CheckList
        legend={t("admin.teams.members")}
        options={(analysts.data ?? []).map((a) => ({ id: a.id, label: a.full_name }))}
        selected={selected}
        onChange={setSelected}
      />
    </SaveSection>
  );
}

function TeamRolesForm({ team }: { team: Team }) {
  const t = useText();
  const roles = useQuery(roleRowsQuery);
  const [selected, setSelected] = useState(new Set(team.role_ids));
  return (
    <SaveSection
      title={t("users.roles")}
      saveLabel={t("users.saveRoles")}
      onSave={async () => done(await setTeamRoles(team.id, [...selected]))}
    >
      <CheckList
        legend={t("users.roles")}
        options={(roles.data ?? []).map((r) => ({ id: r.id, label: r.name }))}
        selected={selected}
        onChange={setSelected}
      />
    </SaveSection>
  );
}

function TeamAccessForm({ team }: { team: Team }) {
  const t = useText();
  const companies = useQuery(companiesQuery);
  const access = team.company_access as Schemas["CompanyAccessDto"];
  const [all, setAll] = useState(access.all);
  const [selected, setSelected] = useState(new Set(access.company_ids));
  return (
    <SaveSection
      title={t("users.companyAccess")}
      saveLabel={t("users.saveAccess")}
      onSave={async () =>
        done(await setTeamCompanyAccess(team.id, { all, company_ids: all ? [] : [...selected] }))
      }
    >
      <CheckField label={t("users.allCompanies")} checked={all} onChange={setAll} />
      {all ? null : (
        <CheckList
          legend={t("users.companyAccess")}
          options={(companies.data ?? []).map((c) => ({ id: c.id, label: c.name }))}
          selected={selected}
          onChange={setSelected}
        />
      )}
    </SaveSection>
  );
}
