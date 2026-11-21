import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { Button } from "@/components/Button";
import { CheckboxList } from "@/components/CheckboxList";
import { Dialog } from "@/components/Dialog";
import { Checkbox, TextField } from "@/components/Field";
import { Notice, ProblemNotice } from "@/components/Notice";
import { analystsQuery, rolesQuery, teamsQuery } from "@/features/reference/queries";
import { useAction } from "@/features/shared/mutation";
import { CompanyAccessEditor } from "@/features/users/CompanyAccessEditor";
import { useText } from "@/i18n/useText";
import { api, type Schemas } from "@/lib/api";
import { call } from "@/lib/request";
import { OrganisationList } from "./OrganisationPages";

type Team = Schemas["TeamDto"];

/** Teams (Queues): members, Roles and Company access (FR-IAM-007). */
export function TeamsPage() {
  const t = useText();
  const teams = useQuery(teamsQuery);
  return (
    <OrganisationList
      title={t("admin.teams")}
      intro={t("admin.teams.intro")}
      rows={teams.data}
      loading={teams.isPending}
      error={teams.error}
      columns={[
        {
          id: "members",
          header: t("admin.teams.members"),
          cell: (q) => t("admin.teams.memberCount", { count: q.analyst_ids.length }),
        },
        {
          id: "description",
          header: t("common.description"),
          wide: true,
          cell: (q) => q.description || t("common.noneDash"),
        },
      ]}
      renderEditor={(row, close) => <TeamDialog teamId={row?.id} onClose={close} />}
    />
  );
}

function TeamDialog({ teamId, onClose }: { teamId?: string | undefined; onClose: () => void }) {
  const t = useText();
  const teams = useQuery(teamsQuery);
  const [createdId, setCreatedId] = useState<string | undefined>(undefined);
  const team = teams.data?.find((q) => q.id === (teamId ?? createdId));
  return (
    <Dialog
      open
      onOpenChange={(o) => !o && onClose()}
      title={team ? t("ref.edit", { name: team.name }) : t("ref.add")}
      size="lg"
    >
      <div className="grid gap-6">
        <TeamDetails team={team} onCreated={setCreatedId} />
        {team ? (
          <>
            <TeamMembers team={team} />
            <TeamRoles team={team} />
            <TeamAccess team={team} />
          </>
        ) : (
          <Notice tone="info" live={false}>
            {t("admin.teams.saveFirst")}
          </Notice>
        )}
      </div>
    </Dialog>
  );
}

function TeamDetails({
  team,
  onCreated,
}: {
  team?: Team | undefined;
  onCreated: (id: string) => void;
}) {
  const t = useText();
  const [form, setForm] = useState({
    name: team?.name ?? "",
    description: team?.description ?? "",
    is_active: team?.is_active ?? true,
  });
  const save = useAction(
    () =>
      team
        ? call(
            api.PUT("/api/v1/teams/{id}", {
              params: { path: { id: team.id } },
              body: { ...form, name: form.name.trim() },
            }),
          )
        : call(api.POST("/api/v1/teams", { body: { ...form, name: form.name.trim() } })),
    {
      invalidate: [["reference", "teams"]],
      success: () => t("ref.saved", { name: form.name.trim() }),
      onSuccess: (saved) => {
        if (!team) onCreated(saved.id);
      },
    },
  );
  return (
    <form
      className="grid gap-3"
      onSubmit={(e) => {
        e.preventDefault();
        if (form.name.trim()) save.mutate();
      }}
    >
      <TextField
        label={t("ref.name")}
        required
        value={form.name}
        onChange={(e) => setForm((x) => ({ ...x, name: e.target.value }))}
      />
      <TextField
        label={t("common.description")}
        value={form.description}
        onChange={(e) => setForm((x) => ({ ...x, description: e.target.value }))}
      />
      <Checkbox
        label={t("common.active")}
        checked={form.is_active}
        onChange={(e) => setForm((x) => ({ ...x, is_active: e.target.checked }))}
      />
      <ProblemNotice error={save.error} />
      <div>
        <Button
          type="submit"
          variant="primary"
          loading={save.isPending}
          disabled={!form.name.trim()}
        >
          {team ? t("common.saveChanges") : t("common.create")}
        </Button>
      </div>
    </form>
  );
}

function TeamMembers({ team }: { team: Team }) {
  const t = useText();
  const analysts = useQuery(analystsQuery);
  const [ids, setIds] = useState(team.analyst_ids);
  const save = useAction(
    () =>
      call(
        api.PUT("/api/v1/teams/{id}/members", {
          params: { path: { id: team.id } },
          body: { analyst_ids: ids },
        }),
      ),
    {
      invalidate: [["reference", "teams"]],
      success: "admin.teams.membersSaved",
    },
  );
  return (
    <section className="grid gap-3 border-t border-line pt-4">
      <CheckboxList
        legend={t("admin.teams.members")}
        columns={2}
        items={(analysts.data ?? [])
          .filter((a) => a.is_active || ids.includes(a.id))
          .map((a) => ({ id: a.id, label: a.full_name, description: a.email }))}
        selected={ids}
        onChange={setIds}
      />
      <ProblemNotice error={save.error} />
      <div>
        <Button loading={save.isPending} onClick={() => save.mutate()}>
          {t("admin.teams.saveMembers")}
        </Button>
      </div>
    </section>
  );
}

function TeamRoles({ team }: { team: Team }) {
  const t = useText();
  const roles = useQuery(rolesQuery);
  const [ids, setIds] = useState(team.role_ids);
  const save = useAction(
    () =>
      call(
        api.PUT("/api/v1/teams/{id}/roles", {
          params: { path: { id: team.id } },
          body: { role_ids: ids },
        }),
      ),
    {
      invalidate: [["reference", "teams"]],
      success: "admin.teams.rolesSaved",
    },
  );
  return (
    <section className="grid gap-3 border-t border-line pt-4">
      <CheckboxList
        legend={t("users.roles")}
        items={(roles.data ?? []).map((r) => ({
          id: r.id,
          label: r.name,
          description: r.description || undefined,
        }))}
        selected={ids}
        onChange={setIds}
      />
      <ProblemNotice error={save.error} />
      <div>
        <Button loading={save.isPending} onClick={() => save.mutate()}>
          {t("users.saveRoles")}
        </Button>
      </div>
    </section>
  );
}

function TeamAccess({ team }: { team: Team }) {
  const t = useText();
  const save = useAction(
    (access: Schemas["CompanyAccessDto"]) =>
      call(
        api.PUT("/api/v1/teams/{id}/company-access", {
          params: { path: { id: team.id } },
          body: access,
        }),
      ),
    { invalidate: [["reference", "teams"]], success: "users.accessSaved" },
  );
  return (
    <section className="grid gap-3 border-t border-line pt-4">
      <h3 className="text-sm font-semibold">{t("users.companyAccess")}</h3>
      <CompanyAccessEditor
        value={team.company_access}
        busy={save.isPending}
        onSave={(a) => save.mutate(a)}
      />
      <ProblemNotice error={save.error} />
    </section>
  );
}
