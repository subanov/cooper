import { useQuery } from "@tanstack/react-query";
import { useRouterState } from "@tanstack/react-router";
import { type FormEvent, useState } from "react";
import { useIntl } from "react-intl";
import { Alert } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Field } from "@/components/ui/field";
import { SelectField } from "@/components/ui/select-field";
import {
  createMailbox,
  type Mailbox,
  mailboxesQuery,
  rejectedEmailsQuery,
  startMailboxConsent,
  updateMailbox,
  usesConsent,
} from "@/features/administration/mailboxes";
import { departmentRowsQuery } from "@/features/administration/resources";
import { problemText } from "@/features/result";
import { originsQuery, teamsQuery } from "@/features/tickets/queries";
import { companiesQuery } from "@/features/users/queries";
import type { MessageId } from "@/i18n/messages";
import { useText } from "@/i18n/useText";
import type { Schemas } from "@/lib/api";
import { CheckField, FormActions, ResourceAdmin, useFlash, useInvalidateAdmin } from "./shared";

type Health = Schemas["MailboxHealthDto"];

const PROVIDERS = ["imap", "microsoft", "google"] as const;
const POST_IMPORT = ["mark_read", "move", "delete"] as const;
const UNKNOWN_SENDER = ["create_requester", "reject"] as const;
const REJECTED = ["delete", "move", "keep"] as const;

/**
 * Mailboxes (FR-EML-001): the email accounts Cooper polls for tickets, with their
 * defaults, Whitelist, after-import handling, health (FR-EML-010), Rejected Emails and
 * Mailbox Consent. Secrets are write-only: the form never shows a stored one.
 */
export function MailboxesPage() {
  const t = useText();
  const rows = useQuery(mailboxesQuery);
  // Mailbox Consent ends here with `?mailbox_consent=connected&mailbox=<id>`.
  const connected = useRouterState({
    select: (s) => (s.location.search as Record<string, unknown>).mailbox_consent === "connected",
  });
  return (
    <div className="grid gap-3">
      {connected ? <Alert variant="success">{t("admin.mailboxes.connected")}</Alert> : null}
      <ResourceAdmin
        title={t("admin.mailboxes")}
        intro={t("admin.mailboxes.intro")}
        rows={rows.data}
        loading={rows.isPending}
        failed={rows.isError}
        getId={(r: Mailbox) => r.id ?? r.address}
        getName={(r) => r.name}
        addLabel={t("admin.mailboxes.add")}
        columns={[
          {
            header: t("admin.res.name"),
            cell: (r) => <span className="font-medium">{r.name}</span>,
          },
          { header: t("admin.mailboxes.address"), cell: (r) => r.address },
          {
            header: t("admin.mailboxes.provider"),
            cell: (r) => t(`admin.mailboxes.provider.${r.provider}` as MessageId),
          },
          { header: t("admin.mailboxes.health"), cell: (r) => <HealthBadge health={r.health} /> },
          {
            header: t("admin.res.status"),
            cell: (r) => (
              <Badge tone={r.is_active ? "ok" : "muted"}>
                {t(r.is_active ? "admin.res.active" : "admin.res.inactive")}
              </Badge>
            ),
          },
        ]}
        renderForm={(row, done) => <MailboxEditor row={row} done={done} />}
      />
    </div>
  );
}

function HealthBadge({ health }: { health: Health | null | undefined }) {
  const t = useText();
  if (!health?.last_polled_at) {
    return <Badge tone="muted">{t("admin.mailboxes.notPolled")}</Badge>;
  }
  if (health.consecutive_failures > 0) {
    return (
      <Badge tone="breached">
        {t("admin.mailboxes.failing", { count: health.consecutive_failures })}
      </Badge>
    );
  }
  return <Badge tone="ok">{t("admin.mailboxes.healthy")}</Badge>;
}

function MailboxEditor({ row, done }: { row: Mailbox | null; done: () => void }) {
  return (
    <div className="grid max-w-2xl gap-6">
      <MailboxForm row={row} done={done} />
      {row?.id ? (
        <>
          <ConnectionSection mailbox={row} id={row.id} />
          <HealthSection health={row.health} />
          <RejectedSection id={row.id} />
        </>
      ) : null}
    </div>
  );
}

type WhitelistRow = Schemas["WhitelistDto"] & { key: number };

function MailboxForm({ row, done }: { row: Mailbox | null; done: () => void }) {
  const t = useText();
  const flash = useFlash();
  const invalidate = useInvalidateAdmin();
  const companies = useQuery(companiesQuery);
  const departments = useQuery(departmentRowsQuery);
  const teams = useQuery(teamsQuery);
  const origins = useQuery(originsQuery);

  const [name, setName] = useState(row?.name ?? "");
  const [address, setAddress] = useState(row?.address ?? "");
  const [provider, setProvider] = useState(row?.provider ?? "imap");
  const [authMode, setAuthMode] = useState(row?.auth_mode ?? "delegated");
  const [imapHost, setImapHost] = useState(row?.imap_host ?? "");
  const [imapPort, setImapPort] = useState(String(row?.imap_port ?? 993));
  const [imapSecurity, setImapSecurity] = useState(row?.imap_security ?? "tls");
  const [imapUsername, setImapUsername] = useState(row?.imap_username ?? "");
  const [password, setPassword] = useState("");
  const [clientId, setClientId] = useState("");
  const [clientSecret, setClientSecret] = useState("");
  const [tenant, setTenant] = useState("");
  const [folder, setFolder] = useState(row?.folder || "INBOX");
  const [companyId, setCompanyId] = useState(row?.company_id ?? "");
  const [departmentId, setDepartmentId] = useState(row?.department_id ?? "");
  const [teamId, setTeamId] = useState(row?.team_id ?? "");
  const [originId, setOriginId] = useState(row?.origin_id ?? "");
  const [postImport, setPostImport] = useState(row?.post_import_action ?? "mark_read");
  const [moveTo, setMoveTo] = useState(row?.move_to_folder ?? "");
  const [unknownSender, setUnknownSender] = useState(
    row?.unknown_sender_action ?? "create_requester",
  );
  const [rejected, setRejected] = useState(row?.rejected_action ?? "delete");
  const [whitelist, setWhitelist] = useState<WhitelistRow[]>(
    (row?.whitelist ?? []).map((w, key) => ({ ...w, key })),
  );
  const [nextKey, setNextKey] = useState(whitelist.length);
  const [active, setActive] = useState(row?.is_active ?? true);

  const isImap = provider === "imap";
  const hasSecret = row?.has_secret ?? false;
  const needsFolder = postImport === "move" || rejected === "move";

  async function submit(event: FormEvent) {
    event.preventDefault();
    const credentials =
      !isImap && (clientId.trim() || clientSecret.trim())
        ? {
            client_id: clientId.trim(),
            client_secret: clientSecret,
            tenant: provider === "microsoft" && tenant.trim() ? tenant.trim() : null,
          }
        : null;
    const body: Mailbox = {
      name: name.trim(),
      address: address.trim(),
      provider,
      auth_mode: provider === "microsoft" ? authMode : null,
      imap_host: isImap ? imapHost.trim() || null : null,
      imap_port: isImap && imapPort ? Number(imapPort) : null,
      imap_security: isImap ? imapSecurity : null,
      imap_username: isImap ? imapUsername.trim() || null : null,
      password: isImap && password ? password : null,
      credentials,
      folder: folder.trim(),
      company_id: companyId,
      department_id: departmentId || null,
      team_id: teamId || null,
      origin_id: originId || null,
      post_import_action: postImport,
      move_to_folder: needsFolder ? moveTo.trim() || null : null,
      unknown_sender_action: unknownSender,
      rejected_action: rejected,
      whitelist: whitelist
        .filter((w) => w.pattern.trim())
        .map((w) => ({ pattern: w.pattern.trim(), allow: w.allow })),
      is_active: active,
    };
    const outcome = row?.id ? await updateMailbox(row.id, body) : await createMailbox(body);
    if (!outcome.ok) {
      flash.fail(t("admin.res.failed", { detail: problemText(outcome.problem) }));
      return;
    }
    setPassword("");
    setClientSecret("");
    await invalidate();
    if (!row) {
      done();
    } else {
      flash.ok(t("admin.saved"));
    }
  }

  const secretHint = hasSecret ? t("admin.mailboxes.secretKept") : undefined;
  const option = (prefix: string, value: string) => (
    <option key={value} value={value}>
      {t(`${prefix}.${value}` as MessageId)}
    </option>
  );

  return (
    <form onSubmit={submit} noValidate className="grid gap-5">
      <fieldset className="grid gap-3">
        <legend className="mb-1 font-semibold">{t("admin.mailboxes.account")}</legend>
        <Field
          label={t("admin.res.name")}
          value={name}
          onChange={(e) => setName(e.target.value)}
          required
        />
        <Field
          label={t("admin.mailboxes.address")}
          type="email"
          value={address}
          onChange={(e) => setAddress(e.target.value)}
          required
        />
        <SelectField
          label={t("admin.mailboxes.provider")}
          value={provider}
          onChange={(e) => setProvider(e.target.value)}
        >
          {PROVIDERS.map((p) => option("admin.mailboxes.provider", p))}
        </SelectField>
        {provider === "microsoft" ? (
          <SelectField
            label={t("admin.mailboxes.authMode")}
            value={authMode}
            onChange={(e) => setAuthMode(e.target.value)}
          >
            {option("admin.mailboxes.authMode", "delegated")}
            {option("admin.mailboxes.authMode", "app_only")}
          </SelectField>
        ) : null}
        {isImap ? (
          <>
            <Field
              label={t("admin.mailboxes.imapHost")}
              value={imapHost}
              onChange={(e) => setImapHost(e.target.value)}
              required
            />
            <div className="grid gap-3 sm:grid-cols-2">
              <Field
                label={t("admin.mailboxes.imapPort")}
                type="number"
                min={1}
                max={65535}
                value={imapPort}
                onChange={(e) => setImapPort(e.target.value)}
              />
              <SelectField
                label={t("admin.mailboxes.imapSecurity")}
                value={imapSecurity}
                onChange={(e) => setImapSecurity(e.target.value)}
              >
                {option("admin.mailboxes.imapSecurity", "tls")}
                {option("admin.mailboxes.imapSecurity", "none")}
              </SelectField>
            </div>
            <Field
              label={t("admin.mailboxes.imapUsername")}
              autoComplete="off"
              value={imapUsername}
              onChange={(e) => setImapUsername(e.target.value)}
            />
            <Field
              label={t("admin.mailboxes.password")}
              hint={secretHint}
              type="password"
              autoComplete="new-password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
            />
          </>
        ) : (
          <>
            <Field
              label={t("admin.mailboxes.clientId")}
              hint={secretHint ?? t("admin.mailboxes.clientHint")}
              autoComplete="off"
              value={clientId}
              onChange={(e) => setClientId(e.target.value)}
            />
            <Field
              label={t("admin.mailboxes.clientSecret")}
              type="password"
              autoComplete="new-password"
              value={clientSecret}
              onChange={(e) => setClientSecret(e.target.value)}
            />
            {provider === "microsoft" ? (
              <Field
                label={t("admin.mailboxes.tenant")}
                autoComplete="off"
                value={tenant}
                onChange={(e) => setTenant(e.target.value)}
              />
            ) : null}
          </>
        )}
        <Field
          label={t("admin.mailboxes.folder")}
          value={folder}
          onChange={(e) => setFolder(e.target.value)}
        />
      </fieldset>

      <fieldset className="grid gap-3">
        <legend className="mb-1 font-semibold">{t("admin.mailboxes.defaults")}</legend>
        <SelectField
          label={t("admin.mailboxes.company")}
          value={companyId}
          onChange={(e) => setCompanyId(e.target.value)}
          required
        >
          <option value="">{t("admin.mailboxes.choose")}</option>
          {(companies.data ?? []).map((c) => (
            <option key={c.id} value={c.id}>
              {c.name}
            </option>
          ))}
        </SelectField>
        <SelectField
          label={t("admin.mailboxes.department")}
          value={departmentId}
          onChange={(e) => setDepartmentId(e.target.value)}
        >
          <option value="">{t("admin.mailboxes.none")}</option>
          {(departments.data ?? []).map((d) => (
            <option key={d.id} value={d.id}>
              {d.name}
            </option>
          ))}
        </SelectField>
        <SelectField
          label={t("admin.mailboxes.team")}
          value={teamId}
          onChange={(e) => setTeamId(e.target.value)}
        >
          <option value="">{t("admin.mailboxes.none")}</option>
          {(teams.data ?? []).map((team) => (
            <option key={team.id} value={team.id}>
              {team.name}
            </option>
          ))}
        </SelectField>
        <SelectField
          label={t("admin.mailboxes.origin")}
          value={originId}
          onChange={(e) => setOriginId(e.target.value)}
        >
          <option value="">{t("admin.mailboxes.originDefault")}</option>
          {(origins.data ?? []).map((o) => (
            <option key={o.id} value={o.id}>
              {o.name}
            </option>
          ))}
        </SelectField>
      </fieldset>

      <fieldset className="grid gap-3">
        <legend className="mb-1 font-semibold">{t("admin.mailboxes.handling")}</legend>
        <SelectField
          label={t("admin.mailboxes.postImport")}
          value={postImport}
          onChange={(e) => setPostImport(e.target.value)}
        >
          {POST_IMPORT.map((a) => option("admin.mailboxes.postImport", a))}
        </SelectField>
        <SelectField
          label={t("admin.mailboxes.unknownSender")}
          value={unknownSender}
          onChange={(e) => setUnknownSender(e.target.value)}
        >
          {UNKNOWN_SENDER.map((a) => option("admin.mailboxes.unknownSender", a))}
        </SelectField>
        <SelectField
          label={t("admin.mailboxes.rejected")}
          value={rejected}
          onChange={(e) => setRejected(e.target.value)}
        >
          {REJECTED.map((a) => option("admin.mailboxes.rejected", a))}
        </SelectField>
        {needsFolder ? (
          <Field
            label={t("admin.mailboxes.moveTo")}
            value={moveTo}
            onChange={(e) => setMoveTo(e.target.value)}
            required
          />
        ) : null}
      </fieldset>

      <fieldset className="grid gap-2">
        <legend className="mb-1 font-semibold">{t("admin.mailboxes.whitelist")}</legend>
        <p className="text-sm text-muted-foreground">{t("admin.mailboxes.whitelistIntro")}</p>
        {whitelist.map((w, index) => (
          <div key={w.key} className="flex items-end gap-2">
            <Field
              className="flex-1"
              label={t("admin.mailboxes.pattern", { number: index + 1 })}
              value={w.pattern}
              onChange={(e) =>
                setWhitelist(
                  whitelist.map((x) => (x.key === w.key ? { ...x, pattern: e.target.value } : x)),
                )
              }
            />
            <SelectField
              label={t("admin.mailboxes.patternRule", { number: index + 1 })}
              value={w.allow ? "allow" : "block"}
              onChange={(e) =>
                setWhitelist(
                  whitelist.map((x) =>
                    x.key === w.key ? { ...x, allow: e.target.value === "allow" } : x,
                  ),
                )
              }
            >
              <option value="allow">{t("admin.mailboxes.allow")}</option>
              <option value="block">{t("admin.mailboxes.block")}</option>
            </SelectField>
            <Button
              type="button"
              size="sm"
              variant="ghost"
              aria-label={t("admin.mailboxes.removePattern", { number: index + 1 })}
              onClick={() => setWhitelist(whitelist.filter((x) => x.key !== w.key))}
            >
              {t("admin.mailboxes.remove")}
            </Button>
          </div>
        ))}
        <div>
          <Button
            type="button"
            size="sm"
            variant="outline"
            onClick={() => {
              setWhitelist([...whitelist, { key: nextKey, pattern: "", allow: true }]);
              setNextKey(nextKey + 1);
            }}
          >
            {t("admin.mailboxes.addPattern")}
          </Button>
        </div>
      </fieldset>

      <CheckField label={t("admin.mailboxes.active")} checked={active} onChange={setActive} />
      <FormActions saveLabel={t("admin.save")} onCancel={done} message={flash.element} />
    </form>
  );
}

/** Mailbox Consent for Microsoft `delegated` and Google Mailboxes. */
function ConnectionSection({ mailbox, id }: { mailbox: Mailbox; id: string }) {
  const t = useText();
  const flash = useFlash();
  if (!usesConsent(mailbox)) {
    return null;
  }
  async function connect() {
    const outcome = await startMailboxConsent(id);
    if (!outcome.ok) {
      flash.fail(t("admin.res.failed", { detail: problemText(outcome.problem) }));
      return;
    }
    window.location.assign(outcome.data.authorization_url);
  }
  return (
    <section className="grid gap-2" aria-labelledby={`consent-${id}`}>
      <h3 id={`consent-${id}`} className="font-semibold">
        {t("admin.mailboxes.consent")}
      </h3>
      <p className="text-sm text-muted-foreground">{t("admin.mailboxes.consentIntro")}</p>
      <div className="flex items-center gap-3">
        <Button type="button" size="sm" variant="outline" onClick={connect}>
          {t("admin.mailboxes.connect")}
        </Button>
        {flash.element}
      </div>
    </section>
  );
}

function HealthSection({ health }: { health: Health | null | undefined }) {
  const t = useText();
  const intl = useIntl();
  const when = (at: string | null | undefined) =>
    at ? intl.formatDate(at, { dateStyle: "medium", timeStyle: "short" }) : t("admin.jobs.never");
  return (
    <section className="grid gap-2">
      <h3 className="font-semibold">{t("admin.mailboxes.health")}</h3>
      <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-sm">
        <dt className="text-muted-foreground">{t("admin.mailboxes.lastPolled")}</dt>
        <dd>{when(health?.last_polled_at)}</dd>
        <dt className="text-muted-foreground">{t("admin.mailboxes.lastSuccess")}</dt>
        <dd>{when(health?.last_success_at)}</dd>
        <dt className="text-muted-foreground">{t("admin.mailboxes.failures")}</dt>
        <dd>{health?.consecutive_failures ?? 0}</dd>
        {health?.last_error ? (
          <>
            <dt className="text-muted-foreground">{t("admin.mailboxes.lastError")}</dt>
            <dd>
              {health.last_error} ({when(health.last_error_at)})
            </dd>
          </>
        ) : null}
      </dl>
    </section>
  );
}

function RejectedSection({ id }: { id: string }) {
  const t = useText();
  const intl = useIntl();
  const rows = useQuery(rejectedEmailsQuery(id));
  return (
    <section className="grid gap-2">
      <h3 className="font-semibold">{t("admin.mailboxes.rejectedEmails")}</h3>
      {rows.isError ? <Alert variant="error">{t("common.error")}</Alert> : null}
      {rows.data && rows.data.length === 0 ? (
        <p className="text-sm text-muted-foreground">{t("admin.mailboxes.noRejected")}</p>
      ) : null}
      {rows.data && rows.data.length > 0 ? (
        <table className="w-full text-left text-sm">
          <caption className="sr-only">{t("admin.mailboxes.rejectedEmails")}</caption>
          <thead>
            <tr className="border-b border-border">
              <th scope="col" className="py-1 pr-3">
                {t("admin.mailboxes.received")}
              </th>
              <th scope="col" className="py-1 pr-3">
                {t("admin.mailboxes.from")}
              </th>
              <th scope="col" className="py-1 pr-3">
                {t("admin.mailboxes.subject")}
              </th>
              <th scope="col" className="py-1">
                {t("admin.mailboxes.reason")}
              </th>
            </tr>
          </thead>
          <tbody>
            {rows.data.map((r) => (
              <tr
                key={`${r.received_at}-${r.from}-${r.subject}`}
                className="border-b border-border"
              >
                <td className="py-1 pr-3">
                  {intl.formatDate(r.received_at, { dateStyle: "medium", timeStyle: "short" })}
                </td>
                <td className="py-1 pr-3">{r.from}</td>
                <td className="py-1 pr-3">{r.subject}</td>
                <td className="py-1">{r.reason}</td>
              </tr>
            ))}
          </tbody>
        </table>
      ) : null}
    </section>
  );
}
