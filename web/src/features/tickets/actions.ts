import type { Result } from "@/features/session/actions";
import { api, type Schemas } from "@/lib/api";
import { toProblem } from "@/lib/problem";

type Ticket = Schemas["TicketDto"];

async function result<T>(
  call: Promise<{ data?: T; error?: unknown; response: Response }>,
): Promise<Result<T>> {
  const { data, error, response } = await call;
  return data !== undefined
    ? { ok: true, data }
    : { ok: false, problem: toProblem(error, response) };
}

/** Assigns the ticket to an Analyst and/or Queue; `null` clears. */
export function assignTicket(
  id: string,
  body: Schemas["AssignmentRequest"],
): Promise<Result<Ticket>> {
  return result(api.PUT("/api/v1/tickets/{id}/assignment", { params: { path: { id } }, body }));
}

/** Moves the ticket to another Status (closing needs a Resolution Code). */
export function changeStatus(id: string, body: Schemas["StatusRequest"]): Promise<Result<Ticket>> {
  return result(api.POST("/api/v1/tickets/{id}/status", { params: { path: { id } }, body }));
}

/** Adds an internal or public Note. */
export function addNote(id: string, body: Schemas["NoteRequest"]): Promise<Result<unknown>> {
  return result(api.POST("/api/v1/tickets/{id}/notes", { params: { path: { id } }, body }));
}

/** Emails a reply to the Requester. */
export function sendReply(id: string, body: Schemas["ReplyRequest"]): Promise<Result<unknown>> {
  return result(api.POST("/api/v1/tickets/{id}/replies", { params: { path: { id } }, body }));
}

/** Creates a ticket. Only the subject is required; the server applies defaults. */
export function createTicket(body: Schemas["TicketRequest"]): Promise<Result<Ticket>> {
  return result(api.POST("/api/v1/tickets", { body }));
}
