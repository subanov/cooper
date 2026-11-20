import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { Archive, Eye, EyeOff, Pencil, RotateCcw } from "lucide-react";
import { useState } from "react";
import { Badge } from "@/components/Badge";
import { Button } from "@/components/Button";
import { Card } from "@/components/Card";
import { FormGrid, SelectField, TextAreaField, TextField } from "@/components/Field";
import { Page, PageHeader } from "@/components/Page";
import { LoadingBlock, QueryState } from "@/components/QueryState";
import { analystsQuery, toOptions } from "@/features/reference/queries";
import { useCan } from "@/features/session/session";
import { useActorName } from "@/features/shared/History";
import { fieldError, useAction } from "@/features/shared/mutation";
import { parseTags, RecordForm } from "@/features/shared/RecordForm";
import { useFormat, useText } from "@/i18n/useText";
import type { Schemas } from "@/lib/api";
import { ArticleBody } from "./ArticleBody";
import {
  type Article,
  articleActions,
  articleQuery,
  folderOptions,
  foldersQuery,
  versionsQuery,
} from "./api";

/** One Article: read it, or edit, publish, archive and restore versions (FR-KNW-001…004). */
export function ArticlePage({ id }: { id?: string }) {
  const query = useQuery({ ...articleQuery(id ?? ""), enabled: Boolean(id) });
  if (!id) return <ArticleEditor />;
  return (
    <QueryState
      query={query}
      loading={
        <Page width="medium">
          <LoadingBlock lines={10} />
        </Page>
      }
    >
      {(article) => <ArticleView key={article.updated_at} article={article} />}
    </QueryState>
  );
}

function ArticleView({ article }: { article: Article }) {
  const t = useText();
  const f = useFormat();
  const can = useCan();
  const [editing, setEditing] = useState(false);
  const invalidate = [["knowledge"]] as const;
  const publish = useAction((on: boolean) => articleActions.publish(article.id, on), {
    invalidate,
    success: (_, on) => t(on ? "knowledge.publishedDone" : "knowledge.unpublishedDone"),
  });
  const archive = useAction(() => articleActions.archive(article.id), {
    invalidate,
    success: "knowledge.archivedDone",
  });
  const restore = useAction(() => articleActions.restore(article.id), {
    invalidate,
    success: "knowledge.restoredDone",
  });
  if (editing) return <ArticleEditor article={article} onDone={() => setEditing(false)} />;
  const writer = can("knowledge.write");
  return (
    <Page width="medium">
      <PageHeader
        back={{ to: "/knowledge", label: t("knowledge.back") }}
        title={article.title}
        meta={
          <>
            {article.is_archived ? (
              <Badge tone="muted">{t("knowledge.archived")}</Badge>
            ) : article.is_published ? (
              <Badge tone="success">{t("knowledge.published")}</Badge>
            ) : (
              <Badge tone="warning">{t("knowledge.draft")}</Badge>
            )}
            <span className="text-xs text-fg-muted">
              {t("knowledge.version", { n: article.version })} ·{" "}
              {t("knowledge.updated", { date: f.date(article.updated_at) })} ·{" "}
              {t("knowledge.views", { count: article.view_count })}
            </span>
          </>
        }
        actions={
          writer ? (
            <>
              {!article.is_archived ? (
                <Button
                  icon={<Pencil aria-hidden="true" className="size-4" />}
                  onClick={() => setEditing(true)}
                >
                  {t("knowledge.edit")}
                </Button>
              ) : null}
              {article.is_archived ? (
                <Button
                  icon={<RotateCcw aria-hidden="true" className="size-4" />}
                  loading={restore.isPending}
                  onClick={() => restore.mutate()}
                >
                  {t("knowledge.restore")}
                </Button>
              ) : (
                <Button
                  icon={<Archive aria-hidden="true" className="size-4" />}
                  loading={archive.isPending}
                  onClick={() => archive.mutate()}
                >
                  {t("knowledge.archive")}
                </Button>
              )}
              {!article.is_archived ? (
                <Button
                  variant={article.is_published ? "secondary" : "primary"}
                  icon={
                    article.is_published ? (
                      <EyeOff aria-hidden="true" className="size-4" />
                    ) : (
                      <Eye aria-hidden="true" className="size-4" />
                    )
                  }
                  loading={publish.isPending}
                  onClick={() => publish.mutate(!article.is_published)}
                >
                  {article.is_published ? t("knowledge.unpublish") : t("knowledge.publish")}
                </Button>
              ) : null}
            </>
          ) : undefined
        }
      />
      <Card>
        <ArticleBody html={article.body} />
        {article.tags.length > 0 ? (
          <div className="mt-4 flex flex-wrap gap-1 border-t border-line pt-3">
            {article.tags.map((tag) => (
              <span key={tag} className="rounded-sm bg-sunken px-1.5 py-0.5 text-xs text-fg-muted">
                {tag}
              </span>
            ))}
          </div>
        ) : null}
      </Card>
      {writer ? <Versions article={article} /> : null}
    </Page>
  );
}

function Versions({ article }: { article: Article }) {
  const t = useText();
  const f = useFormat();
  const actorName = useActorName();
  const versions = useQuery(versionsQuery(article.id));
  const restore = useAction((n: number) => articleActions.restoreVersion(article.id, n), {
    invalidate: [["knowledge"]],
    success: (_, n) => t("knowledge.versionRestored", { n }),
  });
  return (
    <Card title={t("knowledge.versions")}>
      <QueryState query={versions}>
        {(list) => (
          <ol className="grid gap-2 text-sm">
            {list.map((v) => (
              <li key={v.version_no} className="flex flex-wrap items-center gap-2">
                <span className="font-medium">
                  {t("knowledge.savedBy", { n: v.version_no, date: f.dateTime(v.saved_at) })}
                </span>
                <span className="text-fg-muted">{actorName(v.saved_by)}</span>
                {v.version_no === article.version ? (
                  <Badge tone="accent">{t("knowledge.current")}</Badge>
                ) : !article.is_archived ? (
                  <Button
                    size="sm"
                    variant="ghost"
                    loading={restore.isPending && restore.variables === v.version_no}
                    onClick={() => restore.mutate(v.version_no)}
                  >
                    {t("knowledge.restoreVersion", { n: v.version_no })}
                  </Button>
                ) : null}
              </li>
            ))}
          </ol>
        )}
      </QueryState>
    </Card>
  );
}

function ArticleEditor({ article, onDone }: { article?: Article; onDone?: () => void }) {
  const t = useText();
  const navigate = useNavigate();
  const folders = useQuery(foldersQuery);
  const analysts = useQuery(analystsQuery);
  const initial = {
    title: article?.title ?? "",
    body: article?.body ?? "",
    audience: article?.audience ?? "internal",
    folder_id: article?.folder_id ?? "",
    tags: article?.tags.join(", ") ?? "",
    next_review_date: article?.next_review_date ?? "",
    owner_id: article?.owner_id ?? "",
  };
  const [form, setForm] = useState(initial);
  const [tried, setTried] = useState(false);
  const set = (key: keyof typeof form) => (e: { target: { value: string } }) =>
    setForm((x) => ({ ...x, [key]: e.target.value }));
  const body = (): Schemas["KnowledgeArticleRequest"] => ({
    title: form.title.trim(),
    body: form.body,
    audience: form.audience,
    folder_id: form.folder_id || null,
    tags: parseTags(form.tags),
    next_review_date: form.next_review_date || null,
    owner_id: form.owner_id || null,
  });
  const save = useAction(
    () => (article ? articleActions.update(article.id, body()) : articleActions.create(body())),
    {
      invalidate: [["knowledge"]],
      success: article ? "knowledge.saved" : "knowledge.created",
      onSuccess: (saved) => {
        if (article) onDone?.();
        else void navigate({ to: "/knowledge/$id", params: { id: saved.id } });
      },
    },
  );
  return (
    <Page width="medium">
      <PageHeader
        back={{ to: "/knowledge", label: t("knowledge.back") }}
        title={article ? article.title : t("knowledge.newTitle")}
        actions={
          article ? <Button onClick={onDone}>{t("knowledge.cancelEdit")}</Button> : undefined
        }
      />
      <Card>
        <RecordForm
          busy={save.isPending}
          error={save.error}
          dirty={JSON.stringify(form) !== JSON.stringify(initial) && Boolean(article)}
          submitLabel={article ? t("common.saveChanges") : t("common.create")}
          onSubmit={() => {
            setTried(true);
            if (form.title.trim()) save.mutate();
          }}
        >
          <TextField
            label={t("knowledge.titleField")}
            required
            value={form.title}
            error={
              tried && !form.title.trim()
                ? t("knowledge.titleRequired")
                : fieldError(save.error, "title")
            }
            onChange={set("title")}
          />
          <TextAreaField
            label={t("knowledge.body")}
            hint={t("knowledge.bodyHint")}
            rows={16}
            className="font-mono"
            value={form.body}
            onChange={set("body")}
          />
          <FormGrid>
            <SelectField
              label={t("knowledge.audience")}
              value={form.audience}
              onChange={set("audience")}
              options={(["internal", "customer", "public"] as const).map((a) => ({
                value: a,
                label: t(`knowledge.audience.${a}`),
              }))}
            />
            <SelectField
              label={t("knowledge.folder")}
              value={form.folder_id}
              placeholder={t("knowledge.noFolder")}
              onChange={set("folder_id")}
              options={folderOptions(folders.data)}
            />
            <TextField
              label={t("knowledge.tags")}
              hint={t("knowledge.tagsHint")}
              value={form.tags}
              onChange={set("tags")}
            />
            <TextField
              label={t("knowledge.reviewDate")}
              type="date"
              value={form.next_review_date}
              onChange={set("next_review_date")}
            />
            <SelectField
              label={t("knowledge.owner")}
              value={form.owner_id}
              placeholder={t("common.none")}
              onChange={set("owner_id")}
              options={toOptions(analysts.data, (a) => a.full_name, article?.owner_id)}
            />
          </FormGrid>
        </RecordForm>
      </Card>
    </Page>
  );
}
