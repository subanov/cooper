import { ApiError, toProblem } from "./problem";

type Call<T> = Promise<{ data?: T; error?: unknown; response: Response }>;

/**
 * Awaits an openapi-fetch call and returns its data, or throws an `ApiError` with the
 * problem the server reported. A network failure becomes a `network` problem.
 */
export async function call<T>(request: Call<T>): Promise<T> {
  let result: Awaited<Call<T>>;
  try {
    result = await request;
  } catch {
    throw new ApiError(toProblem({}, undefined));
  }
  const { data, error, response } = result;
  if (!response.ok) {
    throw new ApiError(toProblem(error, response));
  }
  // 202 and 204 have no body.
  return data as T;
}

/** Reads an upload response (multipart requests do not go through openapi-fetch). */
export async function uploaded<T>(response: Promise<Response>): Promise<T> {
  let res: Response;
  try {
    res = await response;
  } catch {
    throw new ApiError(toProblem({}, undefined));
  }
  const body = await res.json().catch(() => ({}));
  if (!res.ok) {
    throw new ApiError(toProblem(body, res));
  }
  return body as T;
}
