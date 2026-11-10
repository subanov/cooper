import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Link, useNavigate } from "@tanstack/react-router";
import { type FormEvent, useState } from "react";
import { Alert } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Field } from "@/components/ui/field";
import { Label } from "@/components/ui/label";
import { SelectField } from "@/components/ui/select-field";
import { Textarea } from "@/components/ui/textarea";
import {
  archiveArticle,
  articleQuery,
  createArticle,
  foldersQuery,
  restoreArticle,
  setArticlePublished,
  updateArticle,
} from "@/features/knowledge/api";
import { can } from "@/features/navigation/navigation";
import { problemText } from "@/features/result";
import { analystSessionQuery } from "@/features/session/queries";
import { useText } from "@/i18n/useText";
import type { Schemas } from "@/lib/api";
import { useFlash } from "../users/Flash";

type Article = Schemas["KnowledgeArticleDto"];

/** Reads an Article, or edits it (`knowledge.write`); `id` absent creates one. */
export function ArticlePage({ id }: { id?: string | undefined }) {
  const t = useText();
  const session = useQuery(analystSessionQuery).data;
  const canWrite = session ? can(session, "knowledge.write") : false;
  const article = useQuery({ ...articleQuery(id ?? ""), enabled: Boolean(id) });
  const [editing, setEditing] = useState(false);
  if (id && article.isPending) {
    return <p>{t("common.loading")}</p>;
  }
  if (id && !article.data) {
    return <Alert variant="error">{t("common.error")}</Alert>;
  }
  return (
    <div className="grid max-w-3xl gap-4">
      <Link to="/knowledge" className="text-sm underline-offset-2 hover:underline">
        {t("knowledge.back")}
      </Link>
      {!article.data || editing ? (
        <ArticleForm
          key={article.data?.updated_at ?? "new"}
          article={article.data}
          onDone={() => setEditing(false)}
        />
      ) : (
        <ArticleView article={article.data} canWrite={canWrite} onEdit={() => setEditing(true)} />
      )}
    </div>
  );
}

function ArticleView({
  article,
  canWrite,
  onEdit,
}: {
  article: Article;
  canWrite: boolean;
  onEdit: () => void;
}) {
  const t = useText();
  const queryClient = useQueryClient();
  const flash = useFlash();

  async function run(action: () => Promise<{ ok: boolean }>, done: string) {
    const outcome = (await action()) as Awaited<ReturnType<typeof archiveArticle>>;
    if (outcome.ok) {
      await queryClient.invalidateQueries({ queryKey: ["knowledge"] });
      flash.ok(done);
    } else {
      flash.fail(t("tasks.failed", { detail: problemText(outcome.problem) }));
    }
  }

  return (
    <article className="grid gap-3">
      <h1 tabIndex={-1} className="text-2xl font-semibold outline-none">
        {article.title}
      </h1>
      <div className="flex flex-wrap items-center gap-2">
        {!article.is_published ? <Badge tone="warning">{t("knowledge.draft")}</Badge> : null}
        {article.is_archived ? <Badge tone="muted">{t("knowledge.archivedBadge")}</Badge> : null}
        <Badge tone="muted">
          {t(`knowledge.audience.${article.audience}` as "knowledge.audience.internal")}
        </Badge>
        <span className="text-sm text-muted-foreground">
          {t("knowledge.version", { n: article.version })}
        </span>
        {article.tags.map((tag) => (
          <Badge key={tag}>{tag}</Badge>
        ))}
      </div>
      {canWrite ? (
        <div className="flex flex-wrap gap-2">
          <Button size="sm" variant="outline" onClick={onEdit}>
            {t("admin.res.edit")}
          </Button>
          {article.is_archived ? (
            <Button
              size="sm"
              variant="outline"
              onClick={() => void run(() => restoreArticle(article.id), t("knowledge.restored"))}
            >
              {t("knowledge.restore")}
            </Button>
          ) : (
            <>
              <Button
                size="sm"
                variant="outline"
                onClick={() =>
                  void run(
                    () => setArticlePublished(article.id, !article.is_published),
                    t(article.is_published ? "knowledge.unpublished" : "knowledge.published"),
                  )
                }
              >
                {t(article.is_published ? "knowledge.unpublish" : "knowledge.publish")}
              </Button>
              <Button
                size="sm"
                variant="destructive"
                onClick={() =>
                  void run(() => archiveArticle(article.id), t("knowledge.archivedDone"))
                }
              >
                {t("knowledge.archive")}
              </Button>
            </>
          )}
        </div>
      ) : null}
      {flash.element}
      {/* The plain text is shown, not the stored HTML: no raw HTML is ever injected into the page. */}
      <div className="whitespace-pre-wrap rounded-lg border border-border p-4 text-sm">
        {article.body_text}
      </div>
    </article>
  );
}

function ArticleForm({ article, onDone }: { article: Article | undefined; onDone: () => void }) {
  const t = useText();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const flash = useFlash();
  const folders = useQuery(foldersQuery);
  const [title, setTitle] = useState(article?.title ?? "");
  const [body, setBody] = useState(article?.body ?? "");
  const [tags, setTags] = useState((article?.tags ?? []).join(", "));
  const [folderId, setFolderId] = useState(article?.folder_id ?? "");
  const [audience, setAudience] = useState(article?.audience ?? "internal");
  const [titleError, setTitleError] = useState<string | undefined>();

  async function submit(event: FormEvent) {
    event.preventDefault();
    if (!title.trim()) {
      setTitleError(t("tasks.titleRequired"));
      return;
    }
    setTitleError(undefined);
    const request: Schemas["KnowledgeArticleRequest"] = {
      title: title.trim(),
      body,
      tags: tags
        .split(",")
        .map((x) => x.trim())
        .filter(Boolean),
      folder_id: folderId || null,
      audience,
    };
    const outcome = article
      ? await updateArticle(article.id, request)
      : await createArticle(request);
    if (!outcome.ok) {
      flash.fail(t("tasks.failed", { detail: problemText(outcome.problem) }));
      return;
    }
    await queryClient.invalidateQueries({ queryKey: ["knowledge"] });
    if (article) {
      onDone();
    } else {
      await navigate({ to: "/knowledge/$id", params: { id: outcome.data.id } });
    }
  }

  return (
    <form onSubmit={submit} noValidate className="grid gap-3" aria-labelledby="article-form-title">
      <h1 id="article-form-title" tabIndex={-1} className="text-2xl font-semibold outline-none">
        {article ? t("knowledge.edit") : t("knowledge.new")}
      </h1>
      <Field
        label={t("tasks.title")}
        value={title}
        onChange={(e) => setTitle(e.target.value)}
        error={titleError}
        required
      />
      <div className="grid gap-1.5">
        <Label htmlFor="article-body">{t("knowledge.body")}</Label>
        <Textarea
          id="article-body"
          rows={12}
          className="font-mono"
          value={body}
          onChange={(e) => setBody(e.target.value)}
        />
        <p className="text-xs text-muted-foreground">{t("knowledge.bodyHint")}</p>
      </div>
      <div className="grid gap-3 sm:grid-cols-2">
        <SelectField
          label={t("knowledge.folder")}
          value={folderId}
          onChange={(e) => setFolderId(e.target.value)}
        >
          <option value="">{t("knowledge.noFolder")}</option>
          {(folders.data ?? []).map((f) => (
            <option key={f.id} value={f.id}>
              {f.name}
            </option>
          ))}
        </SelectField>
        <SelectField
          label={t("knowledge.audienceLabel")}
          value={audience}
          onChange={(e) => setAudience(e.target.value)}
        >
          <option value="internal">{t("knowledge.audience.internal")}</option>
          <option value="customer">{t("knowledge.audience.customer")}</option>
          <option value="public">{t("knowledge.audience.public")}</option>
        </SelectField>
      </div>
      <Field
        label={t("tasks.tags")}
        hint={t("tasks.tagsHint")}
        value={tags}
        onChange={(e) => setTags(e.target.value)}
      />
      <div className="flex items-center gap-3">
        <Button type="submit">{article ? t("users.save") : t("users.create")}</Button>
        {article ? (
          <Button type="button" variant="ghost" onClick={onDone}>
            {t("admin.res.cancel")}
          </Button>
        ) : null}
        {flash.element}
      </div>
    </form>
  );
}
