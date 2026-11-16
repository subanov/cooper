import { queryOptions } from "@tanstack/react-query";
import { result } from "@/features/result";
import { api, type Schemas } from "@/lib/api";

/** A Mailbox as the API returns it and accepts it (secrets are write-only). */
export type Mailbox = Schemas["MailboxDto"];

/** Every Mailbox with its health (FR-EML-001, FR-EML-010; needs `tickets.configure`). */
export const mailboxesQuery = queryOptions({
  queryKey: ["administration", "mailboxes"],
  queryFn: async (): Promise<Mailbox[]> => {
    const { data, response } = await api.GET("/api/v1/mailboxes");
    if (!data) {
      throw new Error(`mailboxes request failed: ${response.status}`);
    }
    return data;
  },
});

/** Recent Rejected Emails of one Mailbox (FR-EML-005). */
export function rejectedEmailsQuery(id: string) {
  return queryOptions({
    queryKey: ["administration", "mailboxes", id, "rejected-emails"],
    queryFn: async () => {
      const { data, response } = await api.GET("/api/v1/mailboxes/{id}/rejected-emails", {
        params: { path: { id } },
      });
      if (!data) {
        throw new Error(`rejected emails request failed: ${response.status}`);
      }
      return data;
    },
  });
}

/** Creates a Mailbox; a `password` or `credentials` in the body is stored encrypted. */
export const createMailbox = (body: Mailbox) => result(api.POST("/api/v1/mailboxes", { body }));

/** Replaces a Mailbox's settings; the stored secret is kept unless the body sends one. */
export const updateMailbox = (id: string, body: Mailbox) =>
  result(api.PUT("/api/v1/mailboxes/{id}", { params: { path: { id } }, body }));

/** Starts Mailbox Consent: the provider sign-in page to send the browser to. */
export const startMailboxConsent = (id: string) =>
  result(api.POST("/api/v1/mailboxes/{id}/consent", { params: { path: { id } } }));

/** Whether the Mailbox is connected through Mailbox Consent (Microsoft delegated, Google). */
export function usesConsent(m: Pick<Mailbox, "provider" | "auth_mode">): boolean {
  return m.provider === "google" || (m.provider === "microsoft" && m.auth_mode === "delegated");
}
