import type { MessageId } from "@/i18n/messages";

/** An RFC 9457 problem from the API, reduced to what screens need. */
export interface Problem {
  /** Last segment of the problem `type`, for example `validation` or `totp-required`. */
  kind: string;
  /** HTTP status (0 when the network failed). */
  status: number;
  /** The server's plain-language detail, if any. Never SQL or stack traces. */
  detail: string | undefined;
  /** The field a validation problem is about, when the API names one. */
  field: string | undefined;
  /** Keys of Ticket Fields that must be answered (`fields-required`). */
  fields: string[];
  /** Seconds from `Retry-After` (rate limits). */
  retryAfter: number | undefined;
}

/** Reads a problem from an error body and its response. */
export function toProblem(body: unknown, response: Response | undefined): Problem {
  const b = (typeof body === "object" && body !== null ? body : {}) as Record<string, unknown>;
  const type = typeof b.type === "string" ? b.type : "";
  const retry = Number(response?.headers.get("retry-after"));
  return {
    kind: type.split("/").pop() || (response ? "unknown" : "network"),
    status: response?.status ?? 0,
    detail: typeof b.detail === "string" && b.detail ? b.detail : undefined,
    field: typeof b.field === "string" ? b.field : undefined,
    fields: Array.isArray(b.fields) ? b.fields.filter((f) => typeof f === "string") : [],
    retryAfter: Number.isFinite(retry) && retry > 0 ? retry : undefined,
  };
}

/** A failed API call, thrown by `call()` so TanStack Query can hold it as `error`. */
export class ApiError extends Error {
  readonly problem: Problem;
  constructor(problem: Problem) {
    super(problem.detail ?? problem.kind);
    this.name = "ApiError";
    this.problem = problem;
  }
}

/** The problem behind any thrown value. */
export function problemOf(error: unknown): Problem {
  if (error instanceof ApiError) {
    return error.problem;
  }
  return toProblem({}, undefined);
}

/**
 * The catalogue headline for a problem: what happened, in plain language. Screens show
 * the server's detail under it when there is one (H9: what, why, what next). Raw status
 * codes are never shown.
 */
export function problemHeadline(problem: Problem): MessageId {
  switch (problem.kind) {
    case "network":
      return "problem.network";
    case "validation":
    case "fields-required":
      return "problem.validation";
    case "rule-violated":
      return "problem.rule";
    case "conflict":
      return "problem.conflict";
    case "forbidden":
      return "problem.forbidden";
    case "unauthenticated":
      return "problem.signedOut";
    case "not-found":
      return "problem.notFound";
    case "csrf-token-invalid":
      return "problem.csrf";
    default:
      break;
  }
  if (problem.status === 403) return "problem.forbidden";
  if (problem.status === 404) return "problem.notFound";
  if (problem.status === 409) return "problem.conflict";
  if (problem.status === 413) return "problem.tooLarge";
  if (problem.status === 422 || problem.status === 400) return "problem.validation";
  if (problem.status === 429) return "problem.tooMany";
  return "problem.server";
}
