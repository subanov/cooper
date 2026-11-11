import { infiniteQueryOptions, queryOptions } from "@tanstack/react-query";
import { ok } from "@/features/okQuery";
import { result } from "@/features/result";
import { api, type Schemas } from "@/lib/api";

/** Contracts as a list, page by page, with their Notice Date and where it stands. */
export function contractListQuery(filter: {
  q?: string;
  statusId?: string;
  supplierId?: string;
  noticeDue?: boolean;
  active?: boolean;
}) {
  return infiniteQueryOptions({
    queryKey: ["contracts", "list", filter],
    queryFn: ({ pageParam }) =>
      ok(
        api.GET("/api/v1/contracts", {
          params: {
            query: {
              limit: 50,
              ...(filter.q ? { q: filter.q } : {}),
              ...(filter.statusId ? { status_id: filter.statusId } : {}),
              ...(filter.supplierId ? { supplier_id: filter.supplierId } : {}),
              ...(filter.noticeDue ? { notice_due: true } : {}),
              ...(filter.active !== undefined ? { active: filter.active } : {}),
              ...(pageParam ? { cursor: pageParam } : {}),
            },
          },
        }),
        "contracts",
      ),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (page) => page.next_cursor ?? undefined,
  });
}

/** One Contract. */
export const contractQuery = (id: string) =>
  queryOptions({
    queryKey: ["contracts", "detail", id],
    queryFn: () => ok(api.GET("/api/v1/contracts/{id}", { params: { path: { id } } }), "contract"),
  });

/** Suppliers by legal name (the first 200, for lists and pickers). */
export function supplierListQuery(filter: { q?: string } = {}) {
  return queryOptions({
    queryKey: ["contracts", "suppliers", filter],
    queryFn: () =>
      ok(
        api.GET("/api/v1/suppliers", {
          params: { query: { limit: 200, ...(filter.q ? { q: filter.q } : {}) } },
        }),
        "suppliers",
      ),
  });
}

/** One Supplier. */
export const supplierQuery = (id: string) =>
  queryOptions({
    queryKey: ["contracts", "supplier", id],
    queryFn: () => ok(api.GET("/api/v1/suppliers/{id}", { params: { path: { id } } }), "supplier"),
  });

/** The Contacts of a Supplier. */
export const supplierContactsQuery = (id: string) =>
  queryOptions({
    queryKey: ["contracts", "contacts", id],
    queryFn: () =>
      ok(
        api.GET("/api/v1/suppliers/{id}/contacts", { params: { path: { id } } }),
        "supplier contacts",
      ),
  });

/** The configurable lookups of Contracts (Vocabulary: no seed values). */
export type ContractLookupKind =
  | "supplier-types"
  | "supplier-statuses"
  | "contract-statuses"
  | "payment-schedules";

// All four kinds share the request and response shape; the path is typed once.
type LookupPath = "/api/v1/contract-statuses";
const lookupPath = (kind: ContractLookupKind) => `/api/v1/${kind}` as LookupPath;
const lookupItemPath = (kind: ContractLookupKind) =>
  `/api/v1/${kind}/{id}` as "/api/v1/contract-statuses/{id}";

/** Every row of one Contracts lookup, inactive rows included. */
export const contractLookupQuery = (kind: ContractLookupKind) =>
  queryOptions({
    queryKey: ["lookups", kind],
    queryFn: () => ok(api.GET(lookupPath(kind)), kind),
    staleTime: 5 * 60_000,
  });

/** Creates a lookup row. */
export const createContractLookup = (
  kind: ContractLookupKind,
  body: Schemas["ContractLookupRequest"],
) => result(api.POST(lookupPath(kind), { body }));

/** Changes a lookup row. */
export const updateContractLookup = (
  kind: ContractLookupKind,
  id: string,
  body: Schemas["ContractLookupRequest"],
) => result(api.PUT(lookupItemPath(kind), { params: { path: { id } }, body }));

/** Creates a Contract. */
export const createContract = (body: Schemas["ContractRequest"]) =>
  result(api.POST("/api/v1/contracts", { body }));

/** Changes a Contract. */
export const updateContract = (id: string, body: Schemas["ContractRequest"]) =>
  result(api.PUT("/api/v1/contracts/{id}", { params: { path: { id } }, body }));

/** Deletes a Contract. */
export const deleteContract = (id: string) =>
  result(api.DELETE("/api/v1/contracts/{id}", { params: { path: { id } } }));

/** Creates a Supplier. */
export const createSupplier = (body: Schemas["SupplierRequest"]) =>
  result(api.POST("/api/v1/suppliers", { body }));

/** Changes a Supplier. */
export const updateSupplier = (id: string, body: Schemas["SupplierRequest"]) =>
  result(api.PUT("/api/v1/suppliers/{id}", { params: { path: { id } }, body }));

/** Adds a Contact to a Supplier. */
export const createContact = (supplierId: string, body: Schemas["ContactRequest"]) =>
  result(
    api.POST("/api/v1/suppliers/{id}/contacts", { params: { path: { id: supplierId } }, body }),
  );

/** Changes a Contact. */
export const updateContact = (id: string, body: Schemas["ContactRequest"]) =>
  result(api.PUT("/api/v1/contacts/{id}", { params: { path: { id } }, body }));
