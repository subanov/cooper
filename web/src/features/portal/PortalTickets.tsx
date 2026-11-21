import { useInfiniteQuery } from "@tanstack/react-query";
import { Link, useNavigate } from "@tanstack/react-router";
import { Inbox, Plus } from "lucide-react";
import { Badge } from "@/components/Badge";
import { buttonClass } from "@/components/Button";
import { EmptyState, LoadingBlock, LoadMore, Notice, ProblemNotice } from "@/components/Feedback";
import { PageHeader } from "@/components/Layout";
import { Segmented } from "@/components/Tabs";
import { useFormat, useText } from "@/i18n/useText";
import { portalTicketsQuery } from "./api";

type Show = "open" | "closed" | "managed";

/** The Requester's tickets, open or closed, and those they cover as a Manager (FR-PRT-003/004). */
export function PortalTicketsPage({ show }: { show: Show }) {
  const t = useText();
  const f = useFormat();
  const navigate = useNavigate();
  const list = useInfiniteQuery(portalTicketsQuery(show));
  const managed = useInfiniteQuery(portalTicketsQuery("managed"));
  const hasManaged = (managed.data?.pages[0]?.items.length ?? 0) > 0 || show === "managed";
  const rows = list.data?.pages.flatMap((p) => p.items) ?? [];
  return (
    <div className="grid gap-6">
      <PageHeader
        title={t("portalTickets.title")}
        description={t("portalTickets.intro")}
        actions={
          <Link
            to="/portal/tickets/new"
            className={buttonClass({ variant: "primary", size: "lg" })}
          >
            <Plus aria-hidden="true" />
            {t("portalTickets.new")}
          </Link>
        }
      />
      <Segmented
        label={t("portalTickets.show")}
        value={show}
        onChange={(v) =>
          void navigate({ to: "/portal/tickets", search: v === "open" ? {} : { show: v } })
        }
        options={[
          { value: "open" as const, label: t("portalTickets.open") },
          { value: "closed" as const, label: t("portalTickets.closed") },
          ...(hasManaged ? [{ value: "managed" as const, label: t("portalTickets.managed") }] : []),
        ]}
      />
      {show === "managed" ? (
        <Notice tone="info" live={false}>
          {t("portalTickets.managedHint")}
        </Notice>
      ) : null}
      <ProblemNotice error={list.error} />
      {list.isPending ? <LoadingBlock lines={5} /> : null}
      {list.isSuccess && rows.length === 0 ? (
        <EmptyState
          icon={<Inbox aria-hidden="true" />}
          title={
            show === "open"
              ? t("portalTickets.empty")
              : show === "closed"
                ? t("portalTickets.emptyClosed")
                : t("portalTickets.emptyManaged")
          }
        />
      ) : null}
      <ul aria-label={t("portalTickets.list")} className="grid gap-2">
        {rows.map((tk) => (
          <li
            key={tk.id}
            className="relative grid gap-1 rounded-lg border border-line bg-surface p-4 shadow-raised hover:border-control"
          >
            <div className="flex flex-wrap items-center gap-2">
              <span className="font-mono text-xs text-fg-muted">{tk.number}</span>
              <Badge tone={tk.is_open ? "accent" : "muted"}>{tk.status}</Badge>
            </div>
            <Link
              to="/portal/tickets/$id"
              params={{ id: tk.id }}
              className="text-[0.9375rem] font-semibold after:absolute after:inset-0 hover:underline"
            >
              {tk.subject}
            </Link>
            <p className="text-xs text-fg-muted">
              {t("portalTickets.updated", { date: f.relative(tk.updated_at) })}
            </p>
          </li>
        ))}
      </ul>
      <LoadMore query={list} />
    </div>
  );
}
