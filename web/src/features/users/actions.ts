import { result } from "@/features/result";
import type { Result } from "@/features/session/actions";
import { api, type Schemas } from "@/lib/api";

type Analyst = Schemas["AnalystDto"];
type Requester = Schemas["RequesterDto"];

/** Creates an Analyst. */
export function createAnalyst(body: Schemas["AnalystRequest"]): Promise<Result<Analyst>> {
  return result(api.POST("/api/v1/analysts", { body }));
}

/** Changes an Analyst's details. */
export function updateAnalyst(
  id: string,
  body: Schemas["AnalystRequest"],
): Promise<Result<Analyst>> {
  return result(api.PUT("/api/v1/analysts/{id}", { params: { path: { id } }, body }));
}

/** Deactivates (`false`) or reactivates (`true`) an Analyst. */
export function setAnalystActive(id: string, active: boolean): Promise<Result<Analyst>> {
  const path = active ? "/api/v1/analysts/{id}/reactivate" : "/api/v1/analysts/{id}/deactivate";
  return result(api.POST(path, { params: { path: { id } } }));
}

/** Sets an Analyst's password (at least 12 characters). */
export function setAnalystPassword(id: string, password: string): Promise<Result<undefined>> {
  return result(
    api.PUT("/api/v1/analysts/{id}/password", { params: { path: { id } }, body: { password } }),
  );
}

/** Turns off an Analyst's two-factor sign-in so they can enrol again. */
export function resetAnalystTotp(id: string): Promise<Result<Analyst>> {
  return result(api.POST("/api/v1/analysts/{id}/totp/reset", { params: { path: { id } } }));
}

/** Replaces the Roles assigned directly to an Analyst. */
export function setAnalystRoles(id: string, role_ids: string[]): Promise<Result<Analyst>> {
  return result(
    api.PUT("/api/v1/analysts/{id}/roles", { params: { path: { id } }, body: { role_ids } }),
  );
}

/** Replaces the Company access granted directly to an Analyst. */
export function setAnalystCompanyAccess(
  id: string,
  body: Schemas["CompanyAccessDto"],
): Promise<Result<Analyst>> {
  return result(
    api.PUT("/api/v1/analysts/{id}/company-access", { params: { path: { id } }, body }),
  );
}

/** Creates a Requester. */
export function createRequester(body: Schemas["RequesterRequest"]): Promise<Result<Requester>> {
  return result(api.POST("/api/v1/requesters", { body }));
}

/** Changes a Requester's details. */
export function updateRequester(
  id: string,
  body: Schemas["RequesterRequest"],
): Promise<Result<Requester>> {
  return result(api.PUT("/api/v1/requesters/{id}", { params: { path: { id } }, body }));
}

/** Deactivates (`false`) or reactivates (`true`) a Requester. */
export function setRequesterActive(id: string, active: boolean): Promise<Result<Requester>> {
  const path = active ? "/api/v1/requesters/{id}/reactivate" : "/api/v1/requesters/{id}/deactivate";
  return result(api.POST(path, { params: { path: { id } } }));
}

/** Emails the Requester a link to set a Portal password. */
export function sendPortalInvitation(id: string): Promise<Result<undefined>> {
  return result(
    api.POST("/api/v1/requesters/{id}/portal-invitation", { params: { path: { id } } }),
  );
}
