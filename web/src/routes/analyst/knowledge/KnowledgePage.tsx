import { useInfiniteQuery, useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { useState } from "react";
import { useIntl } from "react-intl";
import { Alert } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Field } from "@/components/ui/field";
import { SelectField } from "@/components/ui/select-field";
import { type ArticleState, articleListQuery, foldersQuery } from "@/features/knowledge/api";
import { can } from "@/features/navigation/navigation";
import { analystSessionQuery } from "@/features/session/queries";
import { useText } from "@/i18n/useText";

/** Knowledge Articles: search, filter by Folder and state. */
export function KnowledgePage() {
  const t = useText();
  const intl = useIntl();
  const session = useQuery(analystSessionQuery).data;
  const canWrite = session ? can(session, "knowledge.write") : false;
  const [q, setQ] = useState("");
  const [state, setState] = useState<ArticleState>("live");
  const [folderId, setFolderId] = useState("");
  const folders = useQuery(foldersQuery);
  const list = useInfiniteQuery(
    articleListQuery({
      state,
      ...(q.trim() ? { q: q.trim() } : {}),
      ...(folderId ? { folderId } : {}),
    }),
  );
  const rows = list.data?.pages.flatMap((p) => p.items) ?? [];
  return (
    <div className="grid gap-4">
      <div className="flex items-center gap-3">
        <h1 tabIndex={-1} className="text-2xl font-semibold outline-none">
          {t("nav.knowledge")}
        </h1>
        {canWrite ? (
          <Button asChild size="sm" className="ml-auto">
            <Link to="/knowledge/new">{t("knowledge.new")}</Link>
          </Button>
        ) : null}
      </div>
      <div className="flex flex-wrap items-end gap-3">
        <Field
          label={t("knowledge.search")}
          type="search"
          value={q}
          onChange={(e) => setQ(e.target.value)}
        />
        <SelectField
          label={t("knowledge.folder")}
          value={folderId}
          onChange={(e) => setFolderId(e.target.value)}
        >
          <option value="">{t("knowledge.allFolders")}</option>
          {(folders.data ?? []).map((f) => (
            <option key={f.id} value={f.id}>
              {f.name}
            </option>
          ))}
        </SelectField>
        {canWrite ? (
          <SelectField
            label={t("users.status")}
            value={state}
            onChange={(e) => setState(e.target.value as ArticleState)}
          >
            <option value="live">{t("knowledge.live")}</option>
            <option value="current">{t("knowledge.current")}</option>
            <option value="archived">{t("knowledge.archived")}</option>
          </SelectField>
        ) : null}
      </div>
      {list.isError ? <Alert variant="error">{t("common.error")}</Alert> : null}
      {list.isPending ? <p>{t("common.loading")}</p> : null}
      {list.isSuccess && rows.length === 0 ? (
        <p className="text-muted-foreground">{t("knowledge.empty")}</p>
      ) : null}
      <ul className="grid gap-2">
        {rows.map((a) => (
          <li key={a.id} className="grid gap-1 rounded-lg border border-border p-3">
            <div className="flex flex-wrap items-center gap-2">
              <Link
                to="/knowledge/$id"
                params={{ id: a.id }}
                className="font-medium underline-offset-2 hover:underline"
              >
                {a.title}
              </Link>
              {!a.is_published ? <Badge tone="warning">{t("knowledge.draft")}</Badge> : null}
              {a.is_archived ? <Badge tone="muted">{t("knowledge.archivedBadge")}</Badge> : null}
              <Badge tone="muted">
                {t(`knowledge.audience.${a.audience}` as "knowledge.audience.internal")}
              </Badge>
            </div>
            <p className="line-clamp-2 text-sm text-muted-foreground">{a.body_text}</p>
            <p className="text-xs text-muted-foreground">
              {t("knowledge.updated", {
                date: intl.formatDate(a.updated_at, { dateStyle: "medium" }),
              })}
            </p>
          </li>
        ))}
      </ul>
      {list.hasNextPage ? (
        <Button
          variant="outline"
          className="justify-self-start"
          onClick={() => void list.fetchNextPage()}
        >
          {t("users.more")}
        </Button>
      ) : null}
    </div>
  );
}
