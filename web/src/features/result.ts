import type { Result } from "@/features/session/actions";
import { toProblem } from "@/lib/problem";

/** Turns an openapi-fetch call into a `Result`: data, or the problem the server reported. */
export async function result<T>(
  call: Promise<{ data?: T; error?: unknown; response: Response }>,
): Promise<Result<T>> {
  const { data, error, response } = await call;
  if (data !== undefined) {
    return { ok: true, data };
  }
  // 204 No Content has no body: success without data.
  return response.ok
    ? { ok: true, data: undefined as T }
    : { ok: false, problem: toProblem(error, response) };
}

/** A problem as one line for the user: the server's detail, else its kind. */
export function problemText(problem: { detail: string | undefined; kind: string }): string {
  return problem.detail ?? problem.kind;
}
