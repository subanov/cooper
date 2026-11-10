import { infiniteQueryOptions, queryOptions } from "@tanstack/react-query";
import { ok } from "@/features/okQuery";
import { result } from "@/features/result";
import { api, type Schemas } from "@/lib/api";

/** Which articles the list shows: `live` (published), `current` (with drafts), `archived`. */
export type ArticleState = "live" | "current" | "archived";

/** Articles, page by page. */
export function articleListQuery(filter: { q?: string; state: ArticleState; folderId?: string }) {
  return infiniteQueryOptions({
    queryKey: ["knowledge", "list", filter],
    queryFn: ({ pageParam }) =>
      ok(
        api.GET("/api/v1/knowledge/articles", {
          params: {
            query: {
              limit: 50,
              state: filter.state,
              ...(filter.q ? { q: filter.q } : {}),
              ...(filter.folderId ? { folder_id: filter.folderId } : {}),
              ...(pageParam ? { cursor: pageParam } : {}),
            },
          },
        }),
        "articles",
      ),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (page) => page.next_cursor ?? undefined,
  });
}

/** One Article. */
export const articleQuery = (id: string) =>
  queryOptions({
    queryKey: ["knowledge", "detail", id],
    queryFn: () =>
      ok(api.GET("/api/v1/knowledge/articles/{id}", { params: { path: { id } } }), "article"),
  });

/** Knowledge Folders. */
export const foldersQuery = queryOptions({
  queryKey: ["knowledge", "folders"],
  queryFn: () => ok(api.GET("/api/v1/knowledge/folders"), "folders"),
});

/** Creates an Article (a draft until published). */
export const createArticle = (body: Schemas["KnowledgeArticleRequest"]) =>
  result(api.POST("/api/v1/knowledge/articles", { body }));

/** Changes an Article; the server keeps the previous version. */
export const updateArticle = (id: string, body: Schemas["KnowledgeArticleRequest"]) =>
  result(api.PUT("/api/v1/knowledge/articles/{id}", { params: { path: { id } }, body }));

/** Publishes or unpublishes an Article. */
export const setArticlePublished = (id: string, is_published: boolean) =>
  result(
    api.PUT("/api/v1/knowledge/articles/{id}/published", {
      params: { path: { id } },
      body: { is_published },
    }),
  );

/** Archives an Article. */
export const archiveArticle = (id: string) =>
  result(api.POST("/api/v1/knowledge/articles/{id}/archive", { params: { path: { id } } }));

/** Restores an archived Article. */
export const restoreArticle = (id: string) =>
  result(api.POST("/api/v1/knowledge/articles/{id}/restore", { params: { path: { id } } }));
