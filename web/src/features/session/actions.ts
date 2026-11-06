import { api, type Schemas, setCsrfToken } from "@/lib/api";
import { type Problem, toProblem } from "@/lib/problem";

/** The result of an API call: data, or the problem the server reported. */
export type Result<T> = { ok: true; data: T } | { ok: false; problem: Problem };

/** Signs an Analyst in. A `totp-required` problem means: ask for the code and retry. */
export async function signIn(
  body: Schemas["SignInRequest"],
): Promise<Result<Schemas["SessionDto"]>> {
  const { data, error, response } = await api.POST("/api/v1/session", { body });
  if (!data) {
    return { ok: false, problem: toProblem(error, response) };
  }
  setCsrfToken("analyst", data.csrf_token);
  return { ok: true, data };
}

/** Ends the Analyst session. */
export async function signOut(): Promise<void> {
  await api.DELETE("/api/v1/session");
  setCsrfToken("analyst", undefined);
}

/** Emails a password reset link. Always succeeds unless the server fails. */
export async function requestPasswordReset(login: string): Promise<Result<null>> {
  const { error, response } = await api.POST("/api/v1/password-reset", { body: { login } });
  return response.ok
    ? { ok: true, data: null }
    : { ok: false, problem: toProblem(error, response) };
}

/** Sets a new password with the token from the reset email. */
export async function confirmPasswordReset(token: string, password: string): Promise<Result<null>> {
  const { error, response } = await api.POST("/api/v1/password-reset/confirm", {
    body: { token, password },
  });
  return response.ok
    ? { ok: true, data: null }
    : { ok: false, problem: toProblem(error, response) };
}

/** Starts two-factor enrollment: a new secret for the authenticator app. */
export async function beginTotpEnrollment(): Promise<Result<Schemas["TotpEnrollmentDto"]>> {
  const { data, error, response } = await api.POST("/api/v1/session/totp/enrollment");
  return data ? { ok: true, data } : { ok: false, problem: toProblem(error, response) };
}

/** Confirms enrollment with a code from the app. */
export async function confirmTotpEnrollment(code: string): Promise<Result<null>> {
  const { error, response } = await api.POST("/api/v1/session/totp/enrollment/confirm", {
    body: { code },
  });
  return response.ok
    ? { ok: true, data: null }
    : { ok: false, problem: toProblem(error, response) };
}
