import { infiniteQueryOptions, queryOptions } from "@tanstack/react-query";
import { api, type Schemas } from "@/lib/api";
import { call } from "@/lib/request";

export type Contract = Schemas["ContractDto"];
export type Supplier = Schemas["SupplierDto"];
export type ContractLookupKind =
  | "supplier-types"
  | "supplier-statuses"
  | "contract-statuses"
  | "payment-schedules";

export interface ContractSearch {
  q?: string;
  status?: string;
  supplier?: string;
  notice?: true;
}

export function contractLookupQuery(kind: ContractLookupKind) {
  return queryOptions({
    queryKey: ["reference", kind],
    queryFn: () => {
      switch (kind) {
        case "supplier-types":
          return call(api.GET("/api/v1/supplier-types"));
        case "supplier-statuses":
          return call(api.GET("/api/v1/supplier-statuses"));
        case "contract-statuses":
          return call(api.GET("/api/v1/contract-statuses"));
        case "payment-schedules":
          return call(api.GET("/api/v1/payment-schedules"));
      }
    },
    staleTime: 5 * 60_000,
  });
}

export function contractsQuery(search: ContractSearch) {
  return infiniteQueryOptions({
    queryKey: ["contracts", "list", search],
    queryFn: ({ pageParam }) =>
      call(
        api.GET("/api/v1/contracts", {
          params: {
            query: {
              limit: 50,
              ...(search.q ? { q: search.q } : {}),
              ...(search.status ? { status_id: search.status } : {}),
              ...(search.supplier ? { supplier_id: search.supplier } : {}),
              ...(search.notice ? { notice_due: true } : {}),
              ...(pageParam ? { cursor: pageParam } : {}),
            },
          },
        }),
      ),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (page) => page.next_cursor ?? undefined,
  });
}

export function suppliersQuery(q: string | undefined) {
  return infiniteQueryOptions({
    queryKey: ["contracts", "suppliers", q ?? ""],
    queryFn: ({ pageParam }) =>
      call(
        api.GET("/api/v1/suppliers", {
          params: {
            query: { limit: 50, ...(q ? { q } : {}), ...(pageParam ? { cursor: pageParam } : {}) },
          },
        }),
      ),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (page) => page.next_cursor ?? undefined,
  });
}

/** Every Supplier, for pickers (short lists). */
export const allSuppliersQuery = queryOptions({
  queryKey: ["contracts", "suppliers", "all"],
  queryFn: async () =>
    (await call(api.GET("/api/v1/suppliers", { params: { query: { limit: 200 } } }))).items,
  staleTime: 60_000,
});

export const contractQuery = (id: string) =>
  queryOptions({
    queryKey: ["contracts", "detail", id],
    queryFn: () => call(api.GET("/api/v1/contracts/{id}", { params: { path: { id } } })),
  });
export const contractHistoryQuery = (id: string) =>
  queryOptions({
    queryKey: ["contracts", "detail", id, "history"],
    queryFn: () => call(api.GET("/api/v1/contracts/{id}/history", { params: { path: { id } } })),
  });
export const supplierQuery = (id: string) =>
  queryOptions({
    queryKey: ["contracts", "supplier", id],
    queryFn: () => call(api.GET("/api/v1/suppliers/{id}", { params: { path: { id } } })),
  });
export const contactsQuery = (id: string) =>
  queryOptions({
    queryKey: ["contracts", "supplier", id, "contacts"],
    queryFn: () => call(api.GET("/api/v1/suppliers/{id}/contacts", { params: { path: { id } } })),
  });
export const supplierHistoryQuery = (id: string) =>
  queryOptions({
    queryKey: ["contracts", "supplier", id, "history"],
    queryFn: () => call(api.GET("/api/v1/suppliers/{id}/history", { params: { path: { id } } })),
  });

const path = (id: string) => ({ params: { path: { id } } });
export const contractActions = {
  create: (body: Schemas["ContractRequest"]) => call(api.POST("/api/v1/contracts", { body })),
  update: (id: string, body: Schemas["ContractRequest"]) =>
    call(api.PUT("/api/v1/contracts/{id}", { ...path(id), body })),
  remove: (id: string) => call(api.DELETE("/api/v1/contracts/{id}", path(id))),
  createSupplier: (body: Schemas["SupplierRequest"]) =>
    call(api.POST("/api/v1/suppliers", { body })),
  updateSupplier: (id: string, body: Schemas["SupplierRequest"]) =>
    call(api.PUT("/api/v1/suppliers/{id}", { ...path(id), body })),
  addContact: (supplierId: string, body: Schemas["ContactRequest"]) =>
    call(api.POST("/api/v1/suppliers/{id}/contacts", { ...path(supplierId), body })),
  updateContact: (id: string, body: Schemas["ContactRequest"]) =>
    call(api.PUT("/api/v1/contacts/{id}", { ...path(id), body })),
};
