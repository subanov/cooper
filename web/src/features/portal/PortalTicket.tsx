import { useQuery } from "@tanstack/react-query";
import { Download, Paperclip } from "lucide-react";
import { useRef, useState } from "react";
import { Badge } from "@/components/Badge";
import { Button } from "@/components/Button";
import { LoadingBlock, ProblemNotice, QueryState } from "@/components/Feedback";
import { TextAreaField } from "@/components/Field";
import { BackLink, PageHeader, Panel } from "@/components/Layout";
import { useAction } from "@/features/shared/mutation";
import { useFormat, useText } from "@/i18n/useText";
import { cn } from "@/lib/cn";
import {
  type PortalTicket,
  portalActions,
  portalAttachmentsQuery,
  portalTicketQuery,
  portalTimelineQuery,
} from "./api";

/**
 * One of the Requester's tickets (FR-PRT-003): the public conversation, a reply box and
 * files. Internal Notes and time are never shown; the API leaves them out.
 */
export function PortalTicketPage({ id }: { id: string }) {
  const query = useQuery(portalTicketQuery(id));
  return (
    <QueryState query={query} loading={<LoadingBlock lines={8} />}>
      {(ticket) => <TicketView ticket={ticket} />}
    </QueryState>
  );
}

function TicketView({ ticket }: { ticket: PortalTicket }) {
  const t = useText();
  const f = useFormat();
  const timeline = useQuery(portalTimelineQuery(ticket.id));
  const [body, setBody] = useState("");
  const reply = useAction(() => portalActions.reply(ticket.id, body), {
    invalidate: [["portal"]],
    success: "portalTicket.sent",
    onSuccess: () => setBody(""),
  });
  return (
    <div className="grid gap-6">
      <PageHeader
        back={<BackLink to="/portal/tickets">{t("portalTicket.back")}</BackLink>}
        title={ticket.subject}
        eyebrow={
          <span className="flex flex-wrap items-center gap-2">
            <span className="font-mono font-medium text-fg">{ticket.number}</span>
            <Badge tone={ticket.is_open ? "accent" : "muted"}>{ticket.status}</Badge>
            <span className="text-xs text-fg-muted">
              {t("portalTicket.opened", { date: f.dateTime(ticket.created_at) })}
              {ticket.closed_at
                ? ` · ${t("portalTicket.closedOn", { date: f.dateTime(ticket.closed_at) })}`
                : ""}
            </span>
          </span>
        }
      />
      <div className="grid gap-6 lg:grid-cols-[minmax(0,1fr)_18rem]">
        <div className="grid content-start gap-6">
          <Panel title={t("portalTicket.description")}>
            <p className="whitespace-pre-wrap text-[0.9375rem] leading-relaxed [overflow-wrap:anywhere]">
              {ticket.description}
            </p>
          </Panel>
          <section aria-labelledby="conversation" className="grid gap-3">
            <h2 id="conversation" className="text-[0.9375rem] font-semibold">
              {t("portalTicket.conversation")}
            </h2>
            <QueryState query={timeline}>
              {(entries) =>
                entries.length === 0 ? (
                  <p className="text-sm text-fg-muted">{t("portalTicket.empty")}</p>
                ) : (
                  <ol className="grid gap-3">
                    {entries.map((e) => (
                      <li
                        key={`${e.at}-${e.kind}`}
                        className={cn("flex", e.from_requester ? "justify-end" : "justify-start")}
                      >
                        <article
                          className={cn(
                            "grid max-w-[85%] gap-1 rounded-lg border p-3",
                            e.from_requester
                              ? "border-accent/30 bg-accent-soft"
                              : "border-line bg-surface",
                          )}
                        >
                          <header className="flex flex-wrap gap-2 text-xs text-fg-muted">
                            <span className="font-semibold text-fg">
                              {e.from_requester
                                ? t("portalTicket.you")
                                : t("portalTicket.serviceDesk")}
                            </span>
                            <time dateTime={e.at}>{f.dateTime(e.at)}</time>
                          </header>
                          <p className="whitespace-pre-wrap text-sm [overflow-wrap:anywhere]">
                            {e.body}
                          </p>
                        </article>
                      </li>
                    ))}
                  </ol>
                )
              }
            </QueryState>
          </section>
          <Panel title={t("portalTicket.reply")}>
            <form
              className="grid gap-3"
              onSubmit={(e) => {
                e.preventDefault();
                if (body.trim()) reply.mutate();
              }}
            >
              <TextAreaField
                label={t("portalTicket.reply")}
                labelHidden
                hint={ticket.is_open ? undefined : t("portalTicket.replyHint")}
                rows={5}
                value={body}
                onChange={(e) => setBody(e.target.value)}
              />
              <ProblemNotice error={reply.error} />
              <div>
                <Button
                  type="submit"
                  variant="primary"
                  size="lg"
                  loading={reply.isPending}
                  disabled={!body.trim()}
                >
                  {t("portalTicket.send")}
                </Button>
              </div>
            </form>
          </Panel>
        </div>
        <Attachments ticketId={ticket.id} />
      </div>
    </div>
  );
}

function Attachments({ ticketId }: { ticketId: string }) {
  const t = useText();
  const input = useRef<HTMLInputElement>(null);
  const files = useQuery(portalAttachmentsQuery(ticketId));
  const add = useAction((file: File) => portalActions.upload(ticketId, file), {
    invalidate: [["portal", "ticket", ticketId]],
    success: (a) => t("portalTicket.attached", { name: a.file_name }),
  });
  return (
    <Panel
      title={t("portalTicket.attachments")}
      className="self-start"
      actions={
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
            icon={<Paperclip aria-hidden="true" />}
            loading={add.isPending}
            onClick={() => input.current?.click()}
          >
            {t("portalTicket.attach")}
          </Button>
        </>
      }
    >
      <ProblemNotice error={add.error} className="mb-3" />
      {(files.data ?? []).length === 0 ? (
        <p className="text-sm text-fg-muted">{t("portalTicket.noAttachments")}</p>
      ) : (
        <ul className="grid gap-1">
          {(files.data ?? []).map((a) => (
            <li key={a.id}>
              <a
                href={`/api/v1/portal/tickets/${ticketId}/attachments/${a.id}`}
                download={a.file_name}
                aria-label={t("portalTicket.download", { name: a.file_name })}
                className="flex min-h-11 items-center gap-2 rounded-md px-2 text-sm hover:bg-sunken"
              >
                <Download aria-hidden="true" className="size-4 shrink-0 text-fg-muted" />
                <span className="truncate">{a.file_name}</span>
              </a>
            </li>
          ))}
        </ul>
      )}
    </Panel>
  );
}
