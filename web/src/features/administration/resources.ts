import { queryOptions } from "@tanstack/react-query";
import { result } from "@/features/result";
import type { Result } from "@/features/session/actions";
import { api, type Schemas } from "@/lib/api";

async function ok<T>(call: Promise<{ data?: T; response: Response }>, what: string): Promise<T> {
  const { data, response } = await call;
  if (data === undefined) {
    throw new Error(`${what} request failed: ${response.status}`);
  }
  return data;
}

/** Reference data that shares the Lookup shape (FR-SYS-001). */
export type LookupKind =
  | "ticket-statuses"
  | "ticket-priorities"
  | "ticket-types"
  | "ticket-origins"
  | "resolution-codes";

// All five kinds have the same request and response shape; the path is typed once.
type LookupPath = "/api/v1/ticket-statuses";
const path = (kind: LookupKind) => `/api/v1/${kind}` as LookupPath;
const itemPath = (kind: LookupKind) => `/api/v1/${kind}/{id}` as "/api/v1/ticket-statuses/{id}";

/** Every row of one kind of reference data, including deactivated rows. */
export function lookupRowsQuery(kind: LookupKind) {
  return queryOptions({
    queryKey: ["administration", "lookups", kind],
    queryFn: () => ok(api.GET(path(kind)), kind),
  });
}

/** Creates a reference-data row. */
export function createLookup(kind: LookupKind, body: Schemas["LookupRequest"]) {
  return result(api.POST(path(kind), { body }));
}

/** Changes a reference-data row. */
export function updateLookup(kind: LookupKind, id: string, body: Schemas["LookupRequest"]) {
  return result(api.PUT(itemPath(kind), { params: { path: { id } }, body }));
}

/** Creates a Ticket Category. */
export function createCategory(body: Schemas["CategoryRequest"]) {
  return result(api.POST("/api/v1/ticket-categories", { body }));
}

/** Changes a Ticket Category. */
export function updateCategory(id: string, body: Schemas["CategoryRequest"]) {
  return result(api.PUT("/api/v1/ticket-categories/{id}", { params: { path: { id } }, body }));
}

const ALL = { limit: 200, include_inactive: true } as const;

/** Companies, including deactivated ones. */
export const companyRowsQuery = queryOptions({
  queryKey: ["administration", "companies"],
  queryFn: async () =>
    (await ok(api.GET("/api/v1/companies", { params: { query: ALL } }), "companies")).items,
});

/** Teams, including deactivated ones. */
export const teamRowsQuery = queryOptions({
  queryKey: ["administration", "teams"],
  queryFn: async () =>
    (await ok(api.GET("/api/v1/teams", { params: { query: ALL } }), "teams")).items,
});

/** Departments, including deactivated ones. */
export const departmentRowsQuery = queryOptions({
  queryKey: ["administration", "departments"],
  queryFn: async () =>
    (await ok(api.GET("/api/v1/departments", { params: { query: ALL } }), "departments")).items,
});

/** Cost Centres, including deactivated ones. */
export const costCentreRowsQuery = queryOptions({
  queryKey: ["administration", "cost-centres"],
  queryFn: async () =>
    (await ok(api.GET("/api/v1/cost-centres", { params: { query: ALL } }), "cost centres")).items,
});

/** Roles. */
export const roleRowsQuery = queryOptions({
  queryKey: ["administration", "roles"],
  queryFn: async () =>
    (await ok(api.GET("/api/v1/roles", { params: { query: { limit: 200 } } }), "roles")).items,
});

/** Every Capability a Role may contain. */
export const capabilitiesQuery = queryOptions({
  queryKey: ["administration", "capabilities"],
  queryFn: () => ok(api.GET("/api/v1/capabilities"), "capabilities"),
  staleTime: 5 * 60_000,
});

/** Creates a Company. */
export const createCompany = (body: Schemas["CompanyRequest"]) =>
  result(api.POST("/api/v1/companies", { body }));
/** Changes a Company. */
export const updateCompany = (id: string, body: Schemas["CompanyRequest"]) =>
  result(api.PUT("/api/v1/companies/{id}", { params: { path: { id } }, body }));
/** Deactivates (`false`) or reactivates (`true`) a Company. */
export const setCompanyActive = (id: string, active: boolean) =>
  result(
    api.POST(active ? "/api/v1/companies/{id}/reactivate" : "/api/v1/companies/{id}/deactivate", {
      params: { path: { id } },
    }),
  );

/** Creates a Team. */
export const createTeam = (body: Schemas["TeamRequest"]) =>
  result(api.POST("/api/v1/teams", { body }));
/** Changes a Team. */
export const updateTeam = (id: string, body: Schemas["TeamRequest"]) =>
  result(api.PUT("/api/v1/teams/{id}", { params: { path: { id } }, body }));
/** Replaces a Team's members. */
export const setTeamMembers = (id: string, analyst_ids: string[]) =>
  result(
    api.PUT("/api/v1/teams/{id}/members", { params: { path: { id } }, body: { analyst_ids } }),
  );
/** Replaces a Team's Roles. */
export const setTeamRoles = (id: string, role_ids: string[]) =>
  result(api.PUT("/api/v1/teams/{id}/roles", { params: { path: { id } }, body: { role_ids } }));
/** Replaces a Team's Company access. */
export const setTeamCompanyAccess = (id: string, body: Schemas["CompanyAccessDto"]) =>
  result(api.PUT("/api/v1/teams/{id}/company-access", { params: { path: { id } }, body }));

/** Creates a Role. */
export const createRole = (body: Schemas["RoleRequest"]) =>
  result(api.POST("/api/v1/roles", { body }));
/** Changes a Role. */
export const updateRole = (id: string, body: Schemas["RoleRequest"]) =>
  result(api.PUT("/api/v1/roles/{id}", { params: { path: { id } }, body }));
/** Deletes a Role. */
export const deleteRole = (id: string): Promise<Result<undefined>> =>
  result(api.DELETE("/api/v1/roles/{id}", { params: { path: { id } } }));

/** Creates a Department. */
export const createDepartment = (body: Schemas["DepartmentRequest"]) =>
  result(api.POST("/api/v1/departments", { body }));
/** Changes a Department. */
export const updateDepartment = (id: string, body: Schemas["DepartmentRequest"]) =>
  result(api.PUT("/api/v1/departments/{id}", { params: { path: { id } }, body }));

/** Creates a Cost Centre. */
export const createCostCentre = (body: Schemas["CostCentreRequest"]) =>
  result(api.POST("/api/v1/cost-centres", { body }));
/** Changes a Cost Centre. */
export const updateCostCentre = (id: string, body: Schemas["CostCentreRequest"]) =>
  result(api.PUT("/api/v1/cost-centres/{id}", { params: { path: { id } }, body }));
