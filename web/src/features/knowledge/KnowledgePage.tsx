import { useInfiniteQuery, useQuery } from "@tanstack/react-query";
import { Link, useNavigate } from "@tanstack/react-router";
import { BookOpen, Plus, Search } from "lucide-react";
import { type FormEvent, useEffect, useState } from "react";
import { Badge } from "@/components/Badge";
import { buttonClass } from "@/components/Button";
import { EmptyState } from "@/components/EmptyState";
import { Input, SelectField } from "@/components/Field";
import { ProblemNotice } from "@/components/Notice";
import { FilterBar, Page, PageHeader } from "@/components/Page";
import { LoadingBlock, LoadMore } from "@/components/QueryState";
import { useCan } from "@/features/session/session";
import { useFormat, useText } from "@/i18n/useText";
import type { SearchPatch } from "@/routes/search";
import { articlesQuery, folderOptions, foldersQuery, type KnowledgeSearch } from "./api";

/** Knowledge Articles with full-text search (FR-KNW-006). */
export function KnowledgePage({ search }: { search: KnowledgeSearch }) {
  const t = useText();
  const f = useFormat();
  const can = useCan();
  const navigate = useNavigate();
  const list = useInfiniteQuery(articlesQuery(search));
  const folders = useQuery(foldersQuery);
  const rows = list.data?.pages.flatMap((p) => p.items) ?? [];
  const [q, setQ] = useState(search.q ?? "");
  useEffect(() => setQ(search.q ?? ""), [search.q]);
  const setSearch = (next: SearchPatch<KnowledgeSearch>) =>
    void navigate({ to: "/knowledge", search: { ...search, ...next } });
  const filtered = Boolean(search.q || search.folder);
  return (
    <Page width="medium">
      <PageHeader
        title={t("knowledge.title")}
        description={t("knowledge.intro")}
        actions={
          can("knowledge.write") ? (
            <Link to="/knowledge/new" className={buttonClass({ variant: "primary" })}>
              <Plus aria-hidden="true" className="size-4" />
              {t("knowledge.new")}
            </Link>
          ) : undefined
        }
      />
      <FilterBar label={t("common.filters")}>
        <form
          className="relative w-full max-w-sm flex-1"
          onSubmit={(e: FormEvent) => {
            e.preventDefault();
            setSearch({ q: q.trim() || undefined });
          }}
        >
          <Search
            aria-hidden="true"
            className="pointer-events-none absolute left-2.5 top-1/2 size-4 -translate-y-1/2 text-fg-muted"
          />
          <Input
            type="search"
            aria-label={t("knowledge.search")}
            placeholder={t("knowledge.searchPlaceholder")}
            value={q}
            onChange={(e) => setQ(e.target.value)}
            className="pl-8"
          />
        </form>
        <SelectField
          className="w-52"
          label={t("knowledge.folder")}
          value={search.folder ?? ""}
          placeholder={t("knowledge.allFolders")}
          options={folderOptions(folders.data)}
          onChange={(e) => setSearch({ folder: e.target.value || undefined })}
        />
        {can("knowledge.write") ? (
          <SelectField
            className="w-44"
            label={t("knowledge.state")}
            value={search.state ?? "live"}
            options={(["live", "current", "archived"] as const).map((s) => ({
              value: s,
              label: t(`knowledge.state.${s}`),
            }))}
            onChange={(e) =>
              setSearch({
                state:
                  e.target.value === "live"
                    ? undefined
                    : (e.target.value as KnowledgeSearch["state"]),
              })
            }
          />
        ) : null}
      </FilterBar>
      <ProblemNotice error={list.error} />
      {list.isPending ? <LoadingBlock lines={6} /> : null}
      {list.isSuccess && rows.length === 0 ? (
        <EmptyState
          icon={<BookOpen aria-hidden="true" />}
          title={filtered ? t("knowledge.empty") : t("knowledge.emptyStart")}
        />
      ) : null}
      <ul aria-label={t("knowledge.list")} className="grid gap-2">
        {rows.map((a) => (
          <li
            key={a.id}
            className="relative grid gap-1 rounded-lg border border-line bg-surface p-4 shadow-raised hover:border-control"
          >
            <div className="flex flex-wrap items-center gap-2">
              <Link
                to="/knowledge/$id"
                params={{ id: a.id }}
                className="font-semibold after:absolute after:inset-0 hover:underline"
              >
                {a.title}
              </Link>
              {a.is_archived ? (
                <Badge tone="muted">{t("knowledge.archived")}</Badge>
              ) : !a.is_published ? (
                <Badge tone="warning">{t("knowledge.draft")}</Badge>
              ) : null}
              <Badge tone="neutral">
                {t(
                  `knowledge.audience.${a.audience === "public" ? "public" : a.audience === "customer" ? "customer" : "internal"}`,
                )}
              </Badge>
            </div>
            <p className="line-clamp-2 text-sm text-fg-muted">{a.body_text}</p>
            <p className="text-xs text-fg-muted">
              {t("knowledge.updated", { date: f.date(a.updated_at) })} ·{" "}
              {t("knowledge.views", { count: a.view_count })}
            </p>
          </li>
        ))}
      </ul>
      <LoadMore query={list} />
    </Page>
  );
}
