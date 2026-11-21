import { useInfiniteQuery, useQuery } from "@tanstack/react-query";
import { Link, useNavigate } from "@tanstack/react-router";
import { ChevronRight, CircleAlert, CircleCheck, Search } from "lucide-react";
import { type FormEvent, useState } from "react";
import { Badge } from "@/components/Badge";
import { Button } from "@/components/Button";
import { LoadingBlock } from "@/components/Feedback";
import { Input } from "@/components/Field";
import { Panel } from "@/components/Layout";
import { useFormat, useText } from "@/i18n/useText";
import { catalogueQuery, portalStatusQuery, portalTicketsQuery } from "./api";
import { usePortalSession } from "./PortalLayout";

/**
 * The Portal start page: search for an answer first, then raise a request from the
 * Portal Catalogue, and see open tickets and Service Status at a glance (FR-PRT-002).
 */
export function PortalHome() {
  const t = useText();
  const f = useFormat();
  const navigate = useNavigate();
  const session = usePortalSession();
  const [q, setQ] = useState("");
  const catalogue = useQuery(catalogueQuery);
  const open = useInfiniteQuery(portalTicketsQuery("open"));
  const status = useQuery(portalStatusQuery);
  const tickets = open.data?.pages[0]?.items.slice(0, 5) ?? [];
  const top = (catalogue.data ?? []).filter((c) => c.depth === 0);
  const issues = (status.data?.incidents ?? []).filter((i) => !i.is_resolved).length > 0;
  return (
    <div className="grid gap-8">
      <section
        aria-labelledby="portal-welcome"
        className="grid gap-4 rounded-xl bg-nav p-6 text-nav-fg shadow-overlay sm:p-10"
      >
        <h1
          id="portal-welcome"
          tabIndex={-1}
          data-page-heading
          className="text-2xl font-semibold tracking-tight"
        >
          {t("portalHome.welcome", { name: session.requester.full_name.split(" ")[0] ?? "" })}
        </h1>
        <search>
          <form
            className="flex flex-col gap-2 sm:flex-row"
            onSubmit={(e: FormEvent) => {
              e.preventDefault();
              void navigate({ to: "/portal/knowledge", search: q.trim() ? { q: q.trim() } : {} });
            }}
          >
            <div className="relative flex-1">
              <Search
                aria-hidden="true"
                className="pointer-events-none absolute left-3 top-1/2 size-5 -translate-y-1/2 text-fg-muted"
              />
              <Input
                type="search"
                aria-label={t("portalHome.searchLabel")}
                placeholder={t("portalHome.searchPlaceholder")}
                value={q}
                onChange={(e) => setQ(e.target.value)}
                className="h-12 border-transparent pl-10 text-base"
              />
            </div>
            <Button type="submit" variant="primary" size="lg" className="h-12">
              {t("portalHome.search")}
            </Button>
          </form>
        </search>
      </section>

      {status.data ? (
        <Link
          to="/portal/service-status"
          className="flex items-center gap-3 rounded-lg border border-line bg-surface px-4 py-3 text-sm hover:border-control"
        >
          {issues ? (
            <CircleAlert aria-hidden="true" className="size-5 shrink-0 text-warning-text" />
          ) : (
            <CircleCheck aria-hidden="true" className="size-5 shrink-0 text-success-text" />
          )}
          <span className="flex-1">
            {issues ? t("portalHome.statusIssues") : t("portalHome.statusOk")}
          </span>
          <span className="font-medium text-accent-text">{t("portalHome.statusLink")}</span>
        </Link>
      ) : null}

      <section aria-labelledby="portal-raise" className="grid gap-3">
        <div>
          <h2 id="portal-raise" className="text-lg font-semibold">
            {t("portalHome.raise")}
          </h2>
          <p className="text-sm text-fg-muted">{t("portalHome.raiseIntro")}</p>
        </div>
        {catalogue.isPending ? (
          <LoadingBlock />
        ) : (
          <ul className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
            {top.map((c) => (
              <li key={c.id}>
                <Link
                  to="/portal/tickets/new"
                  search={{ category: c.id }}
                  className="flex min-h-14 items-center justify-between gap-2 rounded-lg border border-line bg-surface px-4 py-3 font-medium shadow-raised hover:border-accent"
                >
                  {c.name}
                  <ChevronRight aria-hidden="true" className="size-4 text-fg-muted" />
                </Link>
              </li>
            ))}
            <li>
              <Link
                to="/portal/tickets/new"
                className="grid min-h-14 gap-0.5 rounded-lg border border-dashed border-control bg-surface px-4 py-3 hover:border-accent"
              >
                <span className="font-medium">{t("portalHome.other")}</span>
                <span className="text-xs text-fg-muted">{t("portalHome.otherHint")}</span>
              </Link>
            </li>
          </ul>
        )}
      </section>

      <Panel
        title={t("portalHome.open")}
        flush
        actions={
          <Link
            to="/portal/tickets"
            className="text-sm font-medium text-accent-text hover:underline"
          >
            {t("portalHome.seeAll")}
          </Link>
        }
      >
        {open.isPending ? (
          <LoadingBlock lines={2} className="p-4" />
        ) : tickets.length === 0 ? (
          <p className="px-4 py-6 text-sm text-fg-muted">{t("portalHome.noOpen")}</p>
        ) : (
          <ul className="divide-y divide-line">
            {tickets.map((tk) => (
              <li
                key={tk.id}
                className="relative flex flex-wrap items-center gap-x-3 gap-y-1 px-4 py-3 hover:bg-sunken/60"
              >
                <span className="font-mono text-xs text-fg-muted">{tk.number}</span>
                <Link
                  to="/portal/tickets/$id"
                  params={{ id: tk.id }}
                  className="min-w-0 flex-1 truncate font-medium after:absolute after:inset-0 hover:underline"
                >
                  {tk.subject}
                </Link>
                <Badge tone="accent">{tk.status}</Badge>
                <span className="text-xs text-fg-muted">{f.relative(tk.updated_at)}</span>
              </li>
            ))}
          </ul>
        )}
      </Panel>
    </div>
  );
}
