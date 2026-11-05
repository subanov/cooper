# 0019. Email Templates sent from ticket events

- **Status:** Accepted
- **Date:** 2026-11-13
- **Issue:** #63
- **Requirements:** FR-EML-009
- **Related:** [ADR-0005](0005-domain-events-and-transactional-outbox.md),
  [ADR-0017](0017-outbound-email-and-attachments.md)

## Context

FreeITSM sends automated emails (acknowledgements, assignment and closure notices) from
templates that can differ per recipient address or domain. The intent: Requesters hear
that their ticket arrived and when it is closed, and Analysts hear about work assigned to
them, with wording that a Company can tailor.

## Decision

- **Email Templates** are service desk content, shared by every Company like Reply
  Templates: a name, an **event**, an optional **match pattern** (an address or a
  domain), a subject and body with the Reply Template placeholders (`{requester_name}`,
  `{ticket_number}`, `{ticket_subject}`, `{analyst_name}`), and an active flag. Unknown
  placeholders are rejected when the template is saved.
- The events are a fixed set: `ticket_created`, `ticket_closed` and `ticket_replied` (the
  Requester emailed an update; an acknowledgement) go to the **Requester**;
  `ticket_assigned` goes to the **new Assignee**. An Analyst who assigns a ticket to
  themselves gets nothing.
- **Matching:** the active template for the event with the most specific pattern for the
  recipient wins: the address, then the domain (subdomains match, as in the Mailbox
  Whitelist), then a template without a pattern. No match, no email.
- The `email-template-mailer` event handler reacts to `ticket.created`,
  `ticket.assigned`, `ticket.email_received` and `ticket.closed` through the service
  desk's application API. It records the email on the ticket's timeline (outbound, no
  author) and raises `ticket.automatic_email_sent`, which the notifications context
  queues exactly like a Reply. The subject carries the SDREF tag and the threading
  headers point at the ticket's earlier emails, so a Requester's answer threads back.
- The `Message-ID` is derived from the source event (`<auto-<event id>@…>`) and checked
  before sending, so a redelivered event never sends a second email.
- An automatic email is **not** an SLA response: only a Reply by an Analyst is
  (FR-SLA-004).

## Consequences

- Deviation from FreeITSM: one most-specific template per event and recipient instead of
  every matching template, and no email to an Analyst who assigned themselves.
- Loop protection still holds: Cooper's own address is rejected on intake (ADR-0018), so
  an acknowledgement bouncing back to a Mailbox never creates a ticket.
- Per-Company templates, HTML bodies and more events (status changed, SLA breach to
  the Requester) can be added later without changing the contract.
