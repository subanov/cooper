/** An RFC 9457 problem returned by the API, reduced to what screens need. */
export interface Problem {
  /** The last segment of the problem `type`, for example `totp-required`. */
  kind: string;
  /** HTTP status. */
  status: number;
  /** Human-readable detail from the server. */
  detail: string | undefined;
  /** Seconds from `Retry-After`, for rate limits and IP bans. */
  retryAfter: number | undefined;
}

/** Reads a problem from an openapi-fetch error body and its response. */
export function toProblem(error: unknown, response: Response): Problem {
  const body = (typeof error === "object" && error !== null ? error : {}) as {
    type?: unknown;
    detail?: unknown;
  };
  const type = typeof body.type === "string" ? body.type : "";
  const retry = Number(response.headers.get("retry-after"));
  return {
    kind: type.split("/").pop() || "unknown",
    status: response.status,
    detail: typeof body.detail === "string" ? body.detail : undefined,
    retryAfter: Number.isFinite(retry) && retry > 0 ? retry : undefined,
  };
}
