import { infiniteQueryOptions, queryOptions } from "@tanstack/react-query";
import { api, type Schemas } from "@/lib/api";
import { call } from "@/lib/request";

export type Article = Schemas["KnowledgeArticleDto"];

export interface KnowledgeSearch {
  q?: string;
  folder?: string;
  state?: "live" | "current" | "archived";
}

export const foldersQuery = queryOptions({
  queryKey: ["knowledge", "folders"],
  queryFn: () => call(api.GET("/api/v1/knowledge/folders")),
  staleTime: 60_000,
});

export function articlesQuery(search: KnowledgeSearch) {
  return infiniteQueryOptions({
    queryKey: ["knowledge", "list", search],
    queryFn: ({ pageParam }) =>
      call(
        api.GET("/api/v1/knowledge/articles", {
          params: {
            query: {
              limit: 30,
              ...(search.q ? { q: search.q } : {}),
              ...(search.folder ? { folder_id: search.folder } : {}),
              ...(search.state ? { state: search.state } : {}),
              ...(pageParam ? { cursor: pageParam } : {}),
            },
          },
        }),
      ),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (page) => page.next_cursor ?? undefined,
  });
}

export const articleQuery = (id: string) =>
  queryOptions({
    queryKey: ["knowledge", "detail", id],
    queryFn: () => call(api.GET("/api/v1/knowledge/articles/{id}", { params: { path: { id } } })),
  });

export const versionsQuery = (id: string) =>
  queryOptions({
    queryKey: ["knowledge", "detail", id, "versions"],
    queryFn: () =>
      call(api.GET("/api/v1/knowledge/articles/{id}/versions", { params: { path: { id } } })),
  });

const path = (id: string) => ({ params: { path: { id } } });
export const articleActions = {
  create: (body: Schemas["KnowledgeArticleRequest"]) =>
    call(api.POST("/api/v1/knowledge/articles", { body })),
  update: (id: string, body: Schemas["KnowledgeArticleRequest"]) =>
    call(api.PUT("/api/v1/knowledge/articles/{id}", { ...path(id), body })),
  publish: (id: string, isPublished: boolean) =>
    call(
      api.PUT("/api/v1/knowledge/articles/{id}/published", {
        ...path(id),
        body: { is_published: isPublished },
      }),
    ),
  archive: (id: string) => call(api.POST("/api/v1/knowledge/articles/{id}/archive", path(id))),
  restore: (id: string) => call(api.POST("/api/v1/knowledge/articles/{id}/restore", path(id))),
  restoreVersion: (id: string, versionNo: number) =>
    call(
      api.POST("/api/v1/knowledge/articles/{id}/versions/{version_no}/restore", {
        params: { path: { id, version_no: versionNo } },
      }),
    ),
};

/** Folder options with their path ("IT › Printers"). */
export function folderOptions(rows: readonly Schemas["KnowledgeFolderDto"][] | undefined) {
  const byId = new Map((rows ?? []).map((r) => [r.id, r]));
  const pathOf = (r: Schemas["KnowledgeFolderDto"], depth = 0): string => {
    const parent = r.parent_id && depth < 8 ? byId.get(r.parent_id) : undefined;
    return parent ? `${pathOf(parent, depth + 1)} › ${r.name}` : r.name;
  };
  return (rows ?? [])
    .map((r) => ({ value: r.id, label: pathOf(r) }))
    .sort((a, b) => a.label.localeCompare(b.label));
}
