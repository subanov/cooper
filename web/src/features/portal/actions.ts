import type { Result } from "@/features/session/actions";
import { api, type Schemas, setCsrfToken } from "@/lib/api";
import { toProblem } from "@/lib/problem";

/** Signs a Requester in to the Portal with their Portal Account (FR-PRT-001). */
export async function portalSignIn(
  body: Schemas["PortalSignInRequest"],
): Promise<Result<Schemas["PortalSessionDto"]>> {
  const { data, error, response } = await api.POST("/api/v1/portal/session", { body });
  if (!data) {
    return { ok: false, problem: toProblem(error, response) };
  }
  setCsrfToken("portal", data.csrf_token);
  return { ok: true, data };
}

/** Ends the Portal session. */
export async function portalSignOut(): Promise<void> {
  await api.DELETE("/api/v1/portal/session");
  setCsrfToken("portal", undefined);
}

/** Acts as the Requester with the same email address in another Company. */
export async function switchRequester(
  requesterId: string,
): Promise<Result<Schemas["PortalSessionDto"]>> {
  const { data, error, response } = await api.PUT("/api/v1/portal/session/requester", {
    body: { requester_id: requesterId },
  });
  if (!data) {
    return { ok: false, problem: toProblem(error, response) };
  }
  setCsrfToken("portal", data.csrf_token);
  return { ok: true, data };
}

async function accepted(
  call: Promise<{ error?: unknown; response: Response }>,
): Promise<Result<null>> {
  const { error, response } = await call;
  return response.ok
    ? { ok: true, data: null }
    : { ok: false, problem: toProblem(error, response) };
}

/** Emails a link to set a Portal password. Succeeds for unknown addresses too. */
export function requestPortalReset(email: string): Promise<Result<null>> {
  return accepted(api.POST("/api/v1/portal/password-reset", { body: { email } }));
}

/** Sets the Portal password with the token from an invitation, reset or registration email. */
export function setPortalPassword(token: string, password: string): Promise<Result<null>> {
  return accepted(api.POST("/api/v1/portal/password", { body: { token, password } }));
}

/** Self-Registration: emails a link that confirms the address. */
export function registerForPortal(
  body: Schemas["PortalRegistrationRequest"],
): Promise<Result<null>> {
  return accepted(api.POST("/api/v1/portal/registration", { body }));
}
