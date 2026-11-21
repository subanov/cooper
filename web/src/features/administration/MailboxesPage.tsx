import { useQuery } from "@tanstack/react-query";
import {
  CircleCheck,
  Inbox,
  Link2,
  Pencil,
  Plus,
  ShieldCheck,
  Trash2,
  TriangleAlert,
} from "lucide-react";
import { useState } from "react";
import { Badge } from "@/components/Badge";
import { Button, IconButton } from "@/components/Button";
import { type Column, DataTable } from "@/components/DataTable";
import { Dialog, FormDialog } from "@/components/Dialog";
import { EmptyState, Notice, ProblemNotice, QueryState } from "@/components/Feedback";
import {
  Checkbox,
  Fieldset,
  FormGrid,
  Input,
  Select,
  SelectField,
  TextField,
} from "@/components/Field";
import { Facts, PageHeader, Panel } from "@/components/Layout";
import {
  companiesQuery,
  departmentsQuery,
  teamsQuery,
  ticketLookupQuery,
  toOptions,
} from "@/features/reference/queries";
import { fieldError, orNull, useAction } from "@/features/shared/mutation";
import type { MessageId } from "@/i18n/messages";
import { useFormat, useText } from "@/i18n/useText";
import { api, type Schemas } from "@/lib/api";
import { call } from "@/lib/request";

type Mailbox = Schemas["MailboxDto"];
type Rejected = Schemas["RejectedEmailDto"];

const mailboxesQuery = {
  queryKey: ["administration", "mailboxes"],
  queryFn: () => call(api.GET("/api/v1/mailboxes")),
} as const;

/**
 * Mailboxes (FR-EML-001…013): the accounts Cooper reads, their health, Mailbox Consent
 * and the Connection Check. Secrets are write-only: the API never returns them.
 */
export function MailboxesPage({
  search,
}: {
  search: { mailbox_consent?: string; mailbox?: string };
}) {
  const t = useText();
  const mailboxes = useQuery(mailboxesQuery);
  const [editing, setEditing] = useState<Mailbox | "new" | null>(null);
  const [rejected, setRejected] = useState<Mailbox | null>(null);
  return (
    <>
      <PageHeader
        level={2}
        title={t("admin.mailboxes")}
        description={t("admin.mailboxes.intro")}
        actions={
          <Button
            variant="primary"
            icon={<Plus aria-hidden="true" />}
            onClick={() => setEditing("new")}
          >
            {t("admin.mailboxes.add")}
          </Button>
        }
      />
      {search.mailbox_consent === "connected" ? (
        <Notice tone="success">{t("admin.mailboxes.connected")}</Notice>
      ) : search.mailbox_consent ? (
        <Notice tone="danger">{t("admin.mailboxes.consentFailed")}</Notice>
      ) : null}
      <QueryState query={mailboxes}>
        {(list) =>
          list.length === 0 ? (
            <EmptyState icon={<Inbox aria-hidden="true" />} title={t("admin.mailboxes.empty")} />
          ) : (
            <div className="grid gap-4 xl:grid-cols-2">
              {list.map((m) => (
                <MailboxCard
                  key={m.id ?? m.address}
                  mailbox={m}
                  onEdit={() => setEditing(m)}
                  onRejected={() => setRejected(m)}
                />
              ))}
            </div>
          )
        }
      </QueryState>
      {editing ? (
        <MailboxDialog
          mailbox={editing === "new" ? undefined : editing}
          onClose={() => setEditing(null)}
        />
      ) : null}
      {rejected ? <RejectedDialog mailbox={rejected} onClose={() => setRejected(null)} /> : null}
    </>
  );
}

function Health({ mailbox }: { mailbox: Mailbox }) {
  const t = useText();
  const h = mailbox.health;
  if (!h?.last_polled_at) return <Badge tone="muted">{t("admin.mailboxes.notPolled")}</Badge>;
  if (h.consecutive_failures > 0) {
    return (
      <Badge tone="danger" icon={<TriangleAlert aria-hidden="true" />}>
        {t("admin.mailboxes.failing", { count: h.consecutive_failures })}
      </Badge>
    );
  }
  return (
    <Badge tone="success" icon={<CircleCheck aria-hidden="true" />}>
      {t("admin.mailboxes.healthy")}
    </Badge>
  );
}

function MailboxCard({
  mailbox,
  onEdit,
  onRejected,
}: {
  mailbox: Mailbox;
  onEdit: () => void;
  onRejected: () => void;
}) {
  const t = useText();
  const f = useFormat();
  const id = mailbox.id ?? "";
  const check = useAction(() =>
    call(api.POST("/api/v1/mailboxes/{id}/connection-check", { params: { path: { id } } })),
  );
  const consent = useAction(
    () => call(api.POST("/api/v1/mailboxes/{id}/consent", { params: { path: { id } } })),
    {
      onSuccess: (r) => window.location.assign(r.authorization_url),
    },
  );
  const needsConsent =
    mailbox.provider === "google" ||
    (mailbox.provider === "microsoft" && mailbox.auth_mode === "delegated");
  const h = mailbox.health;
  return (
    <Panel
      title={mailbox.name}
      description={`${mailbox.address} · ${t(`admin.mailboxes.provider.${mailbox.provider}` as MessageId)}`}
      actions={
        <>
          {!mailbox.is_active ? <Badge tone="muted">{t("common.inactive")}</Badge> : null}
          <Health mailbox={mailbox} />
          <IconButton
            size="sm"
            tooltip="top"
            label={t("admin.mailboxes.edit", { name: mailbox.name })}
            icon={<Pencil aria-hidden="true" />}
            onClick={onEdit}
          />
        </>
      }
    >
      <div className="grid gap-4">
        <Facts
          items={[
            {
              key: "polled",
              term: t("admin.mailboxes.lastPolled"),
              value: h?.last_polled_at ? f.relative(h.last_polled_at) : t("admin.jobs.never"),
            },
            {
              key: "success",
              term: t("admin.mailboxes.lastSuccess"),
              value: h?.last_success_at ? f.dateTime(h.last_success_at) : t("admin.jobs.never"),
            },
            h?.last_error
              ? {
                  key: "error",
                  term: t("admin.mailboxes.lastError"),
                  value: <span className="text-danger-text">{h.last_error}</span>,
                }
              : null,
          ]}
        />
        {check.data ? (
          check.data.ok ? (
            <Notice tone="success">{t("admin.mailboxes.checkOk")}</Notice>
          ) : (
            <Notice tone="danger">
              {t("admin.mailboxes.checkFailed", { error: check.data.error ?? "" })}
            </Notice>
          )
        ) : null}
        <ProblemNotice error={check.error ?? consent.error} />
        <div className="flex flex-wrap gap-2">
          <Button
            size="sm"
            icon={<ShieldCheck aria-hidden="true" />}
            loading={check.isPending}
            onClick={() => check.mutate()}
          >
            {t("admin.mailboxes.checkButton")}
          </Button>
          {needsConsent && mailbox.has_secret ? (
            <Button
              size="sm"
              icon={<Link2 aria-hidden="true" />}
              loading={consent.isPending}
              onClick={() => consent.mutate()}
            >
              {t("admin.mailboxes.connect")}
            </Button>
          ) : null}
          <Button size="sm" variant="ghost" onClick={onRejected}>
            {t("admin.mailboxes.rejectedEmails")}
          </Button>
        </div>
      </div>
    </Panel>
  );
}

function RejectedDialog({ mailbox, onClose }: { mailbox: Mailbox; onClose: () => void }) {
  const t = useText();
  const f = useFormat();
  const list = useQuery({
    queryKey: ["administration", "mailboxes", mailbox.id, "rejected"],
    queryFn: () =>
      call(
        api.GET("/api/v1/mailboxes/{id}/rejected-emails", {
          params: { path: { id: mailbox.id ?? "" } },
        }),
      ),
  });
  const columns: Column<Rejected>[] = [
    {
      key: "received",
      header: t("admin.mailboxes.received"),
      className: "whitespace-nowrap",
      cell: (r) => f.dateTime(r.received_at),
    },
    { key: "from", header: t("admin.mailboxes.from"), cell: (r) => r.from },
    { key: "subject", header: t("admin.mailboxes.subject"), cell: (r) => r.subject },
    { key: "reason", header: t("admin.mailboxes.reason"), cell: (r) => r.reason },
  ];
  return (
    <Dialog
      open
      onOpenChange={(o) => !o && onClose()}
      title={`${t("admin.mailboxes.rejectedEmails")}: ${mailbox.name}`}
      size="lg"
    >
      <QueryState query={list}>
        {(rows) => (
          <DataTable
            caption={t("admin.mailboxes.rejectedEmails")}
            rows={rows}
            rowKey={(r) => `${r.received_at}-${r.from}`}
            empty={<p className="py-4 text-sm text-fg-muted">{t("admin.mailboxes.noRejected")}</p>}
            columns={columns}
          />
        )}
      </QueryState>
    </Dialog>
  );
}

function MailboxDialog({
  mailbox,
  onClose,
}: {
  mailbox?: Mailbox | undefined;
  onClose: () => void;
}) {
  const t = useText();
  const companies = useQuery(companiesQuery);
  const departments = useQuery(departmentsQuery);
  const teams = useQuery(teamsQuery);
  const origins = useQuery(ticketLookupQuery("ticket-origins"));
  const activeCompanies = (companies.data ?? []).filter((c) => c.is_active);
  const [form, setForm] = useState({
    name: mailbox?.name ?? "",
    address: mailbox?.address ?? "",
    provider: mailbox?.provider ?? "imap",
    auth_mode: mailbox?.auth_mode ?? "delegated",
    imap_host: mailbox?.imap_host ?? "",
    imap_port: String(mailbox?.imap_port ?? 993),
    imap_security: mailbox?.imap_security ?? "tls",
    imap_username: mailbox?.imap_username ?? "",
    password: "",
    client_id: "",
    client_secret: "",
    tenant: "",
    folder: mailbox?.folder ?? "INBOX",
    company_id: mailbox?.company_id ?? activeCompanies.find((c) => c.is_default)?.id ?? "",
    department_id: mailbox?.department_id ?? "",
    team_id: mailbox?.team_id ?? "",
    origin_id: mailbox?.origin_id ?? "",
    post_import_action: mailbox?.post_import_action ?? "mark_read",
    move_to_folder: mailbox?.move_to_folder ?? "",
    unknown_sender_action: mailbox?.unknown_sender_action ?? "create_requester",
    rejected_action: mailbox?.rejected_action ?? "keep",
    is_active: mailbox?.is_active ?? true,
  });
  const [whitelist, setWhitelist] = useState<Schemas["WhitelistDto"][]>(mailbox?.whitelist ?? []);
  const set = (key: keyof typeof form) => (e: { target: { value: string } }) =>
    setForm((x) => ({ ...x, [key]: e.target.value }));
  const imap = form.provider === "imap";
  const body = (): Mailbox => ({
    ...(mailbox?.id ? { id: mailbox.id } : {}),
    name: form.name.trim(),
    address: form.address.trim(),
    provider: form.provider,
    auth_mode: form.provider === "microsoft" ? form.auth_mode : null,
    imap_host: imap ? orNull(form.imap_host) : null,
    imap_port: imap ? Number(form.imap_port) || null : null,
    imap_security: imap ? form.imap_security : null,
    imap_username: imap ? orNull(form.imap_username) : null,
    ...(imap && form.password ? { password: form.password } : {}),
    ...(!imap && form.client_id && form.client_secret
      ? {
          credentials: {
            client_id: form.client_id.trim(),
            client_secret: form.client_secret,
            tenant: orNull(form.tenant),
          },
        }
      : {}),
    folder: form.folder.trim() || "INBOX",
    company_id: form.company_id,
    department_id: form.department_id || null,
    team_id: form.team_id || null,
    origin_id: form.origin_id || null,
    post_import_action: form.post_import_action,
    move_to_folder: orNull(form.move_to_folder),
    unknown_sender_action: form.unknown_sender_action,
    rejected_action: form.rejected_action,
    is_active: form.is_active,
    whitelist: whitelist.filter((w) => w.pattern.trim()),
  });
  const save = useAction(
    () =>
      mailbox?.id
        ? call(
            api.PUT("/api/v1/mailboxes/{id}", {
              params: { path: { id: mailbox.id } },
              body: body(),
            }),
          )
        : call(api.POST("/api/v1/mailboxes", { body: body() })),
    {
      invalidate: [["administration", "mailboxes"]],
      success: "admin.mailboxes.saved",
      onSuccess: onClose,
    },
  );
  const err = (field: string) => fieldError(save.error, field);
  const opt = (prefix: string, values: readonly string[]) =>
    values.map((v) => ({ value: v, label: t(`${prefix}.${v}` as MessageId) }));
  return (
    <FormDialog
      open
      onOpenChange={(o) => !o && onClose()}
      title={mailbox ? t("admin.mailboxes.edit", { name: mailbox.name }) : t("admin.mailboxes.add")}
      submitLabel={t("common.save")}
      busy={save.isPending}
      size="lg"
      submitDisabled={!form.name.trim() || !form.address.trim() || !form.company_id}
      onSubmit={() => save.mutate()}
    >
      <ProblemNotice error={save.error} />
      <Fieldset legend={t("admin.mailboxes.account")}>
        <FormGrid>
          <TextField
            label={t("admin.mailboxes.name")}
            required
            value={form.name}
            error={err("name")}
            onChange={set("name")}
          />
          <TextField
            label={t("admin.mailboxes.address")}
            type="email"
            required
            value={form.address}
            error={err("address")}
            onChange={set("address")}
          />
          <SelectField
            label={t("admin.mailboxes.provider")}
            value={form.provider}
            onChange={set("provider")}
            options={opt("admin.mailboxes.provider", ["imap", "microsoft", "google"])}
          />
          {form.provider === "microsoft" ? (
            <SelectField
              label={t("admin.mailboxes.authMode")}
              value={form.auth_mode}
              onChange={set("auth_mode")}
              options={opt("admin.mailboxes.authMode", ["delegated", "app_only"])}
            />
          ) : null}
          {imap ? (
            <>
              <TextField
                label={t("admin.mailboxes.imapHost")}
                value={form.imap_host}
                error={err("imap_host")}
                onChange={set("imap_host")}
              />
              <TextField
                label={t("admin.mailboxes.imapPort")}
                type="number"
                value={form.imap_port}
                onChange={set("imap_port")}
              />
              <SelectField
                label={t("admin.mailboxes.imapSecurity")}
                value={form.imap_security}
                onChange={set("imap_security")}
                options={opt("admin.mailboxes.imapSecurity", ["tls", "none"])}
              />
              <TextField
                label={t("admin.mailboxes.imapUsername")}
                autoComplete="off"
                value={form.imap_username}
                onChange={set("imap_username")}
              />
              <TextField
                label={t("admin.mailboxes.password")}
                type="password"
                autoComplete="new-password"
                hint={mailbox?.has_secret ? t("admin.mailboxes.secretKept") : undefined}
                value={form.password}
                onChange={set("password")}
              />
            </>
          ) : (
            <>
              <TextField
                label={t("admin.mailboxes.clientId")}
                hint={
                  mailbox?.has_secret
                    ? t("admin.mailboxes.secretKept")
                    : t("admin.mailboxes.clientHint")
                }
                autoComplete="off"
                value={form.client_id}
                onChange={set("client_id")}
              />
              <TextField
                label={t("admin.mailboxes.clientSecret")}
                type="password"
                autoComplete="new-password"
                value={form.client_secret}
                onChange={set("client_secret")}
              />
              {form.provider === "microsoft" ? (
                <TextField
                  label={t("admin.mailboxes.tenant")}
                  value={form.tenant}
                  onChange={set("tenant")}
                />
              ) : null}
            </>
          )}
          <TextField
            label={t("admin.mailboxes.folder")}
            value={form.folder}
            onChange={set("folder")}
          />
        </FormGrid>
        {!imap ? (
          <p className="text-xs text-fg-muted">{t("admin.mailboxes.consentIntro")}</p>
        ) : null}
      </Fieldset>
      <Fieldset legend={t("admin.mailboxes.defaults")}>
        <FormGrid>
          <SelectField
            label={t("common.company")}
            required
            value={form.company_id}
            placeholder={t("common.choose")}
            onChange={set("company_id")}
            options={toOptions(companies.data, (c) => c.name, mailbox?.company_id)}
          />
          <SelectField
            label={t("ticket.department")}
            value={form.department_id}
            placeholder={t("common.none")}
            onChange={set("department_id")}
            options={toOptions(departments.data, (d) => d.name, mailbox?.department_id)}
          />
          <SelectField
            label={t("ticket.queue")}
            value={form.team_id}
            placeholder={t("common.none")}
            onChange={set("team_id")}
            options={toOptions(teams.data, (q) => q.name, mailbox?.team_id)}
          />
          <SelectField
            label={t("ticket.origin")}
            value={form.origin_id}
            placeholder={t("admin.mailboxes.originDefault")}
            onChange={set("origin_id")}
            options={toOptions(origins.data, (o) => o.name, mailbox?.origin_id)}
          />
        </FormGrid>
      </Fieldset>
      <Fieldset legend={t("admin.mailboxes.handling")}>
        <FormGrid>
          <SelectField
            label={t("admin.mailboxes.postImport")}
            value={form.post_import_action}
            onChange={set("post_import_action")}
            options={opt("admin.mailboxes.postImport", ["mark_read", "move", "delete"])}
          />
          {form.post_import_action === "move" || form.rejected_action === "move" ? (
            <TextField
              label={t("admin.mailboxes.moveTo")}
              value={form.move_to_folder}
              onChange={set("move_to_folder")}
            />
          ) : null}
          <SelectField
            label={t("admin.mailboxes.unknownSender")}
            value={form.unknown_sender_action}
            onChange={set("unknown_sender_action")}
            options={opt("admin.mailboxes.unknownSender", ["create_requester", "reject"])}
          />
          <SelectField
            label={t("admin.mailboxes.rejected")}
            value={form.rejected_action}
            onChange={set("rejected_action")}
            options={opt("admin.mailboxes.rejected", ["keep", "move", "delete"])}
          />
        </FormGrid>
      </Fieldset>
      <Fieldset
        legend={t("admin.mailboxes.whitelist")}
        description={t("admin.mailboxes.whitelistIntro")}
      >
        <ul className="grid gap-2">
          {whitelist.map((entry, index) => (
            // Entries have no ID; their position is their identity while editing.
            // biome-ignore lint/suspicious/noArrayIndexKey: rows are edited in place
            <li key={index} className="flex items-center gap-2">
              <Input
                aria-label={t("admin.mailboxes.pattern", { number: index + 1 })}
                value={entry.pattern}
                onChange={(e) =>
                  setWhitelist((w) =>
                    w.map((x, i) => (i === index ? { ...x, pattern: e.target.value } : x)),
                  )
                }
              />
              <Select
                aria-label={t("admin.mailboxes.patternRule", { number: index + 1 })}
                className="w-32"
                value={entry.allow ? "allow" : "block"}
                onChange={(e) =>
                  setWhitelist((w) =>
                    w.map((x, i) =>
                      i === index ? { ...x, allow: e.target.value === "allow" } : x,
                    ),
                  )
                }
              >
                <option value="allow">{t("admin.mailboxes.allow")}</option>
                <option value="block">{t("admin.mailboxes.block")}</option>
              </Select>
              <IconButton
                tooltip="top"
                label={t("admin.mailboxes.removePattern", { number: index + 1 })}
                icon={<Trash2 aria-hidden="true" />}
                onClick={() => setWhitelist((w) => w.filter((_, i) => i !== index))}
              />
            </li>
          ))}
        </ul>
        <div>
          <Button
            size="sm"
            icon={<Plus aria-hidden="true" />}
            onClick={() => setWhitelist((w) => [...w, { pattern: "", allow: true }])}
          >
            {t("admin.mailboxes.addPattern")}
          </Button>
        </div>
      </Fieldset>
      <Checkbox
        label={t("admin.mailboxes.active")}
        checked={form.is_active}
        onChange={(e) => setForm((x) => ({ ...x, is_active: e.target.checked }))}
      />
    </FormDialog>
  );
}
