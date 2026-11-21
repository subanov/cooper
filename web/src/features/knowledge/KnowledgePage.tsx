import { useInfiniteQuery, useQuery } from "@tanstack/react-query";
import { Link, Outlet, useNavigate } from "@tanstack/react-router";
import { BookOpen, Plus } from "lucide-react";
import { Badge } from "@/components/Badge";
import { buttonClass } from "@/components/Button";
import { EmptyState, LoadingBlock, LoadMore, ProblemNotice } from "@/components/Feedback";
import { FilterSelect, SearchBox } from "@/components/ListSearch";
import {
  ListPane,
  PickOne,
  RecordRow,
  rowLinkClass,
  SplitView,
  useDetailOpen,
  useListKeys,
  useOpenId,
} from "@/components/MasterDetail";
import { useCan } from "@/features/session/session";
import { useFormat, useText } from "@/i18n/useText";
import { useHotkeys } from "@/lib/hooks";
import type { SearchPatch } from "@/routes/search";
import {
  type Article,
  articlesQuery,
  folderOptions,
  foldersQuery,
  type KnowledgeSearch,
} from "./api";

/** Knowledge (FR-KNW-001): search and Folders on the left, the open Article on the right. */
export function KnowledgePage({ search }: { search: KnowledgeSearch }) {
  const t = useText();
  const f = useFormat();
  const can = useCan();
  const navigate = useNavigate();
  const list = useInfiniteQuery(articlesQuery(search));
  const folders = useQuery(foldersQuery);
  const rows = list.data?.pages.flatMap((p) => p.items) ?? [];
  const openId = useOpenId();
  const detailOpen = useDetailOpen("/knowledge");
  const setSearch = (next: SearchPatch<KnowledgeSearch>) =>
    void navigate({ to: "/knowledge", search: { ...search, ...next } });
  const filtered = Boolean(search.q || search.folder);
  useListKeys(
    rows.map((a) => a.id),
    openId,
    (id) => void navigate({ to: "/knowledge/$id", params: { id }, search }),
  );
  useHotkeys({ "/": () => document.getElementById("knowledge-search")?.focus() });
  return (
    <SplitView
      detailOpen={detailOpen}
      placeholder={
        <PickOne icon={<BookOpen />} title={t("knowledge.pickOne")}>
          {t("knowledge.intro")}
        </PickOne>
      }
      list={
        <ListPane
          title={t("knowledge.title")}
          label={t("knowledge.title")}
          actions={
            can("knowledge.write") ? (
              <Link
                to="/knowledge/new"
                search={search}
                className={buttonClass({ variant: "primary", size: "sm" })}
              >
                <Plus aria-hidden="true" />
                {t("knowledge.new")}
              </Link>
            ) : undefined
          }
          filters={
            <>
              <SearchBox
                label={t("knowledge.search")}
                placeholder={t("knowledge.searchPlaceholder")}
                value={search.q}
                shortcut="/"
                id="knowledge-search"
                onSubmit={(q) => setSearch({ q })}
              />
              <div className="flex flex-wrap items-center gap-2">
                <FilterSelect
                  label={t("knowledge.folder")}
                  allLabel={t("knowledge.allFolders")}
                  value={search.folder}
                  options={folderOptions(folders.data)}
                  onChange={(folder) => setSearch({ folder })}
                />
                {can("knowledge.write") ? (
                  <FilterSelect
                    label={t("knowledge.state")}
                    allLabel={t("knowledge.state.live")}
                    value={search.state === "live" ? undefined : search.state}
                    options={(["current", "archived"] as const).map((s) => ({
                      value: s,
                      label: t(`knowledge.state.${s}`),
                    }))}
                    onChange={(state) =>
                      setSearch({ state: state as KnowledgeSearch["state"] | undefined })
                    }
                  />
                ) : null}
              </div>
            </>
          }
        >
          {list.isError ? (
            <ProblemNotice error={list.error} className="m-4" onRetry={() => void list.refetch()} />
          ) : null}
          {list.isPending ? <LoadingBlock className="p-4" lines={6} /> : null}
          {list.isSuccess && rows.length === 0 ? (
            <EmptyState
              icon={<BookOpen aria-hidden="true" />}
              title={filtered ? t("knowledge.empty") : t("knowledge.emptyStart")}
            />
          ) : null}
          {rows.length > 0 ? (
            <ul aria-label={t("knowledge.list")}>
              {rows.map((a) => (
                <RecordRow key={a.id} selected={a.id === openId}>
                  <Link
                    to="/knowledge/$id"
                    params={{ id: a.id }}
                    search={search}
                    aria-current={a.id === openId ? "page" : undefined}
                    className={rowLinkClass}
                  >
                    {a.title}
                  </Link>
                  <p className="line-clamp-2 text-xs text-fg-muted">{a.body_text}</p>
                  <div className="flex flex-wrap items-center gap-1.5 text-xs text-fg-muted">
                    <StateBadge article={a} />
                    <Badge tone="neutral">{t(audienceLabel(a.audience))}</Badge>
                    <span>
                      {t("knowledge.updated", { date: f.date(a.updated_at) })} ·{" "}
                      {t("knowledge.views", { count: a.view_count })}
                    </span>
                  </div>
                </RecordRow>
              ))}
            </ul>
          ) : null}
          <LoadMore query={list} />
        </ListPane>
      }
    >
      <Outlet />
    </SplitView>
  );
}

/** The catalogue name of an audience. */
export function audienceLabel(audience: string) {
  return audience === "public"
    ? ("knowledge.audience.public" as const)
    : audience === "customer"
      ? ("knowledge.audience.customer" as const)
      : ("knowledge.audience.internal" as const);
}

/** Draft, Published or Archived. */
export function StateBadge({
  article,
}: {
  article: Pick<Article, "is_archived" | "is_published">;
}) {
  const t = useText();
  if (article.is_archived) return <Badge tone="muted">{t("knowledge.archived")}</Badge>;
  if (!article.is_published) return <Badge tone="warning">{t("knowledge.draft")}</Badge>;
  return <Badge tone="success">{t("knowledge.published")}</Badge>;
}
