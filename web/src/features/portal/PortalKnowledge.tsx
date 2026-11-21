import { useQuery } from "@tanstack/react-query";
import { Link, useNavigate } from "@tanstack/react-router";
import { BookOpen, Search } from "lucide-react";
import { type FormEvent, useEffect, useState } from "react";
import { Button, buttonClass } from "@/components/Button";
import { EmptyState, LoadingBlock, QueryState } from "@/components/Feedback";
import { Input } from "@/components/Field";
import { BackLink, PageHeader, Panel } from "@/components/Layout";
import { ArticleBody } from "@/features/knowledge/ArticleBody";
import { useFormat, useText } from "@/i18n/useText";
import { api } from "@/lib/api";
import { call } from "@/lib/request";
import { portalArticlesQuery } from "./api";

/** Help articles for Requesters (FR-PRT-005): customer and public Articles they may read. */
export function PortalKnowledgePage({ q }: { q?: string | undefined }) {
  const t = useText();
  const f = useFormat();
  const navigate = useNavigate();
  const articles = useQuery(portalArticlesQuery(q));
  const [text, setText] = useState(q ?? "");
  useEffect(() => setText(q ?? ""), [q]);
  return (
    <div className="grid gap-6">
      <PageHeader title={t("portalKnowledge.title")} description={t("portalKnowledge.intro")} />
      <search>
        <form
          className="flex flex-col gap-2 sm:flex-row"
          onSubmit={(e: FormEvent) => {
            e.preventDefault();
            void navigate({
              to: "/portal/knowledge",
              search: text.trim() ? { q: text.trim() } : {},
            });
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
              value={text}
              onChange={(e) => setText(e.target.value)}
              className="h-11 pl-10"
            />
          </div>
          <Button type="submit" variant="primary" size="lg">
            {t("portalHome.search")}
          </Button>
        </form>
      </search>
      <QueryState query={articles} loading={<LoadingBlock lines={6} />}>
        {(list) =>
          list.length === 0 ? (
            <EmptyState
              icon={<BookOpen aria-hidden="true" />}
              title={q ? t("portalKnowledge.empty") : t("portalKnowledge.emptyAll")}
              action={
                <Link
                  to="/portal/tickets/new"
                  className={buttonClass({ variant: "primary", size: "lg" })}
                >
                  {t("portalHome.raise")}
                </Link>
              }
            />
          ) : (
            <ul className="grid gap-2">
              {list.map((a) => (
                <li
                  key={a.id}
                  className="relative grid gap-1 rounded-lg border border-line bg-surface p-4 shadow-raised hover:border-control"
                >
                  <Link
                    to="/portal/knowledge/$id"
                    params={{ id: a.id }}
                    className="text-[0.9375rem] font-semibold after:absolute after:inset-0 hover:underline"
                  >
                    {a.title}
                  </Link>
                  <p className="text-xs text-fg-muted">
                    {t("portalKnowledge.updated", { date: f.date(a.updated_at) })}
                  </p>
                </li>
              ))}
            </ul>
          )
        }
      </QueryState>
    </div>
  );
}

/** One help article, with a way out when it does not help (H3). */
export function PortalArticlePage({ id }: { id: string }) {
  const t = useText();
  const f = useFormat();
  const article = useQuery({
    queryKey: ["portal", "knowledge", "article", id],
    queryFn: () => call(api.GET("/api/v1/portal/knowledge/{id}", { params: { path: { id } } })),
  });
  return (
    <QueryState query={article} loading={<LoadingBlock lines={10} />}>
      {(a) => (
        <div className="grid gap-6">
          <PageHeader
            back={<BackLink to="/portal/knowledge">{t("portalKnowledge.back")}</BackLink>}
            title={a.title}
            description={t("portalKnowledge.updated", { date: f.date(a.updated_at) })}
          />
          <Panel>
            <ArticleBody html={a.body} />
          </Panel>
          <div className="flex flex-wrap items-center gap-3 rounded-lg border border-line bg-surface p-4">
            <p className="flex-1 font-medium">{t("portalKnowledge.notSolved")}</p>
            <Link
              to="/portal/tickets/new"
              className={buttonClass({ variant: "primary", size: "lg" })}
            >
              {t("portalHome.raise")}
            </Link>
          </div>
        </div>
      )}
    </QueryState>
  );
}
