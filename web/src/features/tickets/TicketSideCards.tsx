import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { Download, Paperclip, Unlink } from "lucide-react";
import { useRef, useState } from "react";
import { Badge } from "@/components/Badge";
import { Button, IconButton } from "@/components/Button";
import { Card } from "@/components/Card";
import { Combobox, type PickerOption } from "@/components/Combobox";
import { ProblemNotice } from "@/components/Notice";
import { LoadingBlock } from "@/components/QueryState";
import { useCan, useModuleOn } from "@/features/session/session";
import { useAction } from "@/features/shared/mutation";
import { taskActions, taskSearchQuery, ticketTasksQuery } from "@/features/tasks/api";
import { useFormat, useText } from "@/i18n/useText";
import { attachmentsQuery, relatedQuery, type Ticket, ticketActions } from "./api";

/** Files attached to the ticket, with upload (FR-GEN-007). Downloads never render inline. */
export function AttachmentsCard({ ticket, canUpdate }: { ticket: Ticket; canUpdate: boolean }) {
  const t = useText();
  const f = useFormat();
  const input = useRef<HTMLInputElement>(null);
  const list = useQuery(attachmentsQuery(ticket.id));
  const add = useAction((file: File) => ticketActions.upload(ticket.id, file), {
    invalidate: [["tickets", "detail", ticket.id]],
    success: (a) => t("attachments.uploaded", { name: a.file_name }),
  });
  return (
    <Card
      title={t("attachments.title")}
      headingLevel={2}
      actions={
        canUpdate && !ticket.deleted_at ? (
          <>
            <input
              ref={input}
              type="file"
              className="sr-only"
              tabIndex={-1}
              aria-hidden="true"
              onChange={(e) => {
                const file = e.target.files?.[0];
                if (file) add.mutate(file);
                e.target.value = "";
              }}
            />
            <Button
              size="sm"
              icon={<Paperclip aria-hidden="true" className="size-4" />}
              loading={add.isPending}
              onClick={() => input.current?.click()}
            >
              {t("attachments.upload")}
            </Button>
          </>
        ) : undefined
      }
    >
      <ProblemNotice error={add.error} className="mb-3" />
      {list.isPending ? (
        <LoadingBlock lines={2} />
      ) : (list.data ?? []).length === 0 ? (
        <p className="text-sm text-fg-muted">{t("attachments.empty")}</p>
      ) : (
        <ul className="grid gap-1">
          {(list.data ?? []).map((a) => (
            <li key={a.id}>
              <a
                href={`/api/v1/tickets/${ticket.id}/attachments/${a.id}/content`}
                download={a.file_name}
                aria-label={t("attachments.download", { name: a.file_name })}
                className="flex items-center gap-2 rounded-md px-1.5 py-1 text-sm hover:bg-canvas"
              >
                <Download aria-hidden="true" className="size-4 shrink-0 text-fg-muted" />
                <span className="min-w-0 flex-1 truncate">{a.file_name}</span>
                <span className="shrink-0 text-xs text-fg-muted">
                  {formatBytes(a.size_bytes)} · {f.date(a.created_at)}
                </span>
              </a>
            </li>
          ))}
        </ul>
      )}
    </Card>
  );
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

/** Tasks linked to the ticket, with linking (FR-TSK-004). Needs the Tasks Module. */
export function TicketTasksCard({ ticket }: { ticket: Ticket }) {
  const can = useCan();
  const moduleOn = useModuleOn();
  if (!moduleOn("tasks") || !can("tasks.read")) return null;
  return <TasksCardBody ticket={ticket} canLink={can("tasks.update") && !ticket.deleted_at} />;
}

function TasksCardBody({ ticket, canLink }: { ticket: Ticket; canLink: boolean }) {
  const t = useText();
  const linked = useQuery(ticketTasksQuery(ticket.id));
  const [q, setQ] = useState("");
  const [pick, setPick] = useState<PickerOption | null>(null);
  const found = useQuery(taskSearchQuery(q));
  const linkedIds = new Set((linked.data ?? []).map((k) => k.id));
  const options = (found.data ?? [])
    .filter((k) => k.company_id === ticket.company_id && !linkedIds.has(k.id) && !k.is_closed)
    .map((k) => ({ id: k.id, label: k.title }));
  const invalidate = [["tasks"]] as const;
  const link = useAction(
    (taskId: string) => taskActions.link(taskId, { kind: "ticket", target_id: ticket.id }),
    {
      invalidate,
      success: "common.linked",
      onSuccess: () => setPick(null),
    },
  );
  const unlink = useAction((taskId: string) => taskActions.unlink(taskId, "ticket", ticket.id), {
    invalidate,
    success: "common.unlinked",
  });
  return (
    <Card title={t("ticketTasks.title")}>
      <div className="grid gap-3">
        {linked.isPending ? <LoadingBlock lines={2} /> : null}
        {linked.isSuccess && linked.data.length === 0 ? (
          <p className="text-sm text-fg-muted">{t("ticketTasks.empty")}</p>
        ) : null}
        <ul className="grid gap-1">
          {(linked.data ?? []).map((task) => (
            <li key={task.id} className="flex items-center gap-2 text-sm">
              <Link
                to="/tasks/$id"
                params={{ id: task.id }}
                className="min-w-0 flex-1 truncate font-medium hover:underline"
              >
                {task.title}
              </Link>
              <Badge tone={task.is_closed ? "muted" : "accent"}>
                {task.is_closed ? t("ticketTasks.closed") : t("ticketTasks.open")}
              </Badge>
              {canLink ? (
                <IconButton
                  size="sm"
                  label={t("common.unlinkNamed", { name: task.title })}
                  icon={<Unlink aria-hidden="true" className="size-4" />}
                  tooltip="top"
                  onClick={() => unlink.mutate(task.id)}
                />
              ) : null}
            </li>
          ))}
        </ul>
        {canLink ? (
          <form
            className="flex items-end gap-2"
            onSubmit={(e) => {
              e.preventDefault();
              if (pick) link.mutate(pick.id);
            }}
          >
            <Combobox
              className="flex-1"
              label={t("ticketTasks.find")}
              hint={t("ticketTasks.findHint")}
              value={pick}
              onChange={setPick}
              options={options}
              loading={found.isFetching}
              onQueryChange={setQ}
            />
            <Button type="submit" disabled={!pick} loading={link.isPending} className="mb-5">
              {t("common.link")}
            </Button>
          </form>
        ) : null}
        <ProblemNotice error={link.error ?? unlink.error} />
      </div>
    </Card>
  );
}

/** Problems, Changes, Articles and Morning Check results linked to the ticket (FR-TKT-030). */
export function RelatedCard({ ticket }: { ticket: Ticket }) {
  const t = useText();
  const related = useQuery(relatedQuery(ticket.id));
  if (!related.data) return null;
  const groups = [
    { key: "problems", label: t("related.problems"), rows: related.data.problems, to: undefined },
    { key: "changes", label: t("related.changes"), rows: related.data.changes, to: undefined },
    {
      key: "articles",
      label: t("related.articles"),
      rows: related.data.articles,
      to: "/knowledge/$id" as const,
    },
    { key: "checks", label: t("related.checks"), rows: related.data.check_results, to: undefined },
  ].filter((g) => g.rows.length > 0);
  if (groups.length === 0) return null;
  return (
    <Card title={t("related.title")}>
      <div className="grid gap-3">
        {groups.map((g) => (
          <section key={g.key} className="grid gap-1">
            <h3 className="text-xs font-medium text-fg-muted">{g.label}</h3>
            <ul className="grid gap-1 text-sm">
              {g.rows.map((r) => (
                <li key={r.id} className="flex items-center gap-2">
                  {r.number ? (
                    <span className="font-mono text-xs text-fg-muted">{r.number}</span>
                  ) : null}
                  {g.to ? (
                    <Link
                      to={g.to}
                      params={{ id: r.id }}
                      className="min-w-0 flex-1 truncate hover:underline"
                    >
                      {r.title}
                    </Link>
                  ) : (
                    <span className="min-w-0 flex-1 truncate">{r.title}</span>
                  )}
                  {r.is_closed ? <Badge tone="muted">{t("related.closed")}</Badge> : null}
                </li>
              ))}
            </ul>
          </section>
        ))}
      </div>
    </Card>
  );
}
