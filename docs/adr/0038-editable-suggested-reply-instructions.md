# 0038. Editable Suggested Reply Instructions with fixed rules Cooper always adds

- **Status:** Accepted
- **Date:** 2026-11-23
- **Issue:** —
- **Requirements:** FR-AI-003
- **Amends:** [ADR-0037](0037-suggested-replies-with-an-ai-provider.md) (the instruction was a constant)

## Context

ADR-0037 built the instruction sent to the AI provider into the code. Service desks want
to change how Suggested Replies read: the language, tone, greeting, what to ask the
Requester. Part of the instruction is not a matter of taste, though: it keeps internal
Notes away from the Requester, tells the model that ticket text is data (prompt injection)
and defines how the Analyst's typed text (guidance) is used. An edit must not be able to
remove that.

## Decision

- The instruction has two parts. **Suggested Reply Instructions** are editable;
  `FIXED_RULES` (domain constant) are always appended after them, "take precedence" over
  them, and are shown read-only on the admin page.
- **Who:** `tickets.configure`, like Reply Templates and Mailboxes, not `system.manage`.
  Using the instructions still needs only `tickets.update`.
- **Scope:** instance-wide, like Reply Templates. A singleton table
  `suggested_reply_instructions` (`id boolean PRIMARY KEY CHECK (id)`); no row means
  the built-in `DEFAULT_INSTRUCTIONS` apply, so "reset" deletes the row and later
  improvements to the default reach instances that never customised it. Not a System
  Setting: those need `system.manage` and reject newlines.
- **Placeholders:** `{analyst_name}` and `{ticket_number}`, values Cooper or the
  Analyst control. Requester-written values (subject, Requester name) are deliberately not
  available: they would move untrusted text out of the `<ticket>` data block into the
  instructions. Unknown placeholders are rejected on save. At most 8,000 characters.
- **`{ticket_summary}`** (added on request): when the instructions use it, Cooper first
  sends the ticket and thread (no guidance) with fixed `SUMMARY_INSTRUCTIONS` through the
  same `ReplySuggester`, then inserts the answer as `<ticket_summary>…</ticket_summary>`,
  escaped like ticket text and clipped to 3,000 characters. This does put AI-processed
  Requester text into the instructions, so the risk is reduced, not removed: the summary
  prompt forbids repeating instructions, facts from internal Notes are marked
  `[internal]`, and a fixed rule says the summary is data, never instructions, and
  `[internal]` facts must not be revealed. The Analyst's review stays the main defence.
- **Time:** one suggestion, summary included, has 25 seconds (`SuggestedReplyRequest::
  with_time_limit`, honoured by `GeminiSuggester`): at most 10 for the summary, the rest
  for the Reply. A failed or slow summary is logged as a warning and replaced by
  "(no summary available)"; the Reply is still suggested. Using the placeholder costs a
  second provider call (latency and quota).
- Changes are audited (`suggested_reply_instructions`, nil aggregate ID; a reset is
  `deleted`).
- API: `GET/PUT/DELETE /api/v1/suggested-reply-instructions` in the Tickets Module. The
  response carries the default and the fixed rules so the UI shows what is always sent.

## Consequences

- Admins tune Suggested Replies without a release; the prompt-injection and privacy rules
  stay in force whatever they write.
- One text for every Company. Per-Company instructions would add a `tenant_id` row with
  the instance row as fallback, when a Company asks for it.
- The thread limits of ADR-0037 (about 24,000 characters, 6,000 per entry) are unchanged.
