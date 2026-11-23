# 0037. Suggest Replies through a provider port in the service desk, with Gemini configured by environment

- **Status:** Accepted
- **Date:** 2026-11-23
- **Issue:** —
- **Requirements:** FR-AI-003

## Context

FR-AI-003 asks Cooper to suggest replies. Analysts write every Reply by hand or from a
Reply Template. A notebook prototype (`scripts/notebooks/gemini-playground.ipynb`) showed
that the Google Gemini API drafts useful first replies from a ticket's text. FreeITSM has
no comparable feature, so there is no behaviour to keep.

Forces:

- The provider is a third party. Ticket text leaves Cooper when a Reply is suggested.
- Providers are slow and often overloaded (HTTP 429/503, requests that hang). The server's
  request timeout is 30 seconds.
- Ticket text is written by Requesters and may contain instructions aimed at the model
  (prompt injection).
- FR-AI says the Administrator configures the provider. A per-Company settings screen with
  an encrypted key is a larger piece of work than the feature itself.

## Options considered

1. **A provider port in the service desk, Gemini behind a Cargo feature, configured by
   environment.** Small and testable. One key and model for the whole instance.
2. **A separate AI context** owning providers, keys and prompts for every AI feature. This
   is premature with one feature, and the prompt needs service desk data anyway.
3. **Per-Company provider settings in System Settings**, with the key encrypted through
   the `Keyring` (ADR-0013). This matches FR-AI fully but needs admin UI, migrations and
   key rotation before anyone can try the feature.

## Decision

Option 1.

- Domain (`service-desk`, pure): `SuggestedReplyRequest::for_ticket` builds the system
  instruction and the content: the subject, the description, and emails and Notes oldest
  first, each marked by kind. Internal Notes are labelled as Analyst-only and must not be
  revealed. Ticket text sits inside `<ticket>` delimiters, with angle brackets replaced,
  and the instruction says it is data, never instructions. Quoted email lines are dropped.
  Each entry is clipped to 6,000 characters, and the thread to about 24,000, keeping the
  newest entries. Guidance is limited to 4,000 characters.
- Port: `ReplySuggester`, with errors `Unavailable` (503, try again), `Rejected` (422,
  for example a safety block) and `Misconfigured` (502, logged as an error so it reaches
  the System Log).
- Application: `ServiceDesk::suggest_reply` needs `tickets.update` and the normal
  ticket visibility. **Internal Notes and Confidential tickets are included**: the people
  who decided this chose better drafts over keeping that text inside Cooper. Nothing is
  stored and nothing is audited, because no state changes. The Reply the Analyst sends is
  audited as usual. Logs carry the ticket, the model and the duration, never content.
- Infrastructure: `GeminiSuggester` (feature `ai`, on by default in `cooper-server`)
  reuses the rustls `HttpClient` of the mailbox providers. It tries `GEMINI_MODEL`, then
  each of `GEMINI_FALLBACK_MODELS`, moving on when a model is overloaded, unknown or slow.
  All attempts share a 25-second deadline (15 seconds per model), so the answer arrives
  before the server's request timeout. A bad key stops at once. The key goes in the
  `x-goog-api-key` header, never in a URL or a log.
- Configuration: when `GEMINI_API_KEY` is set, the feature is on for every Company.
  `cooper-worker` never suggests Replies.
- API and UI: `POST /api/v1/tickets/{id}/suggested-reply` with optional `guidance`.
  `TicketDto.can_suggest_reply` (single-ticket responses) tells the Composer whether to
  show **Suggest reply** or **Improve with AI**. The suggestion only fills the textarea:
  the Analyst reviews it, can undo it, and sends it as a Reply. This human review is the
  main defence against prompt injection and wrong answers.

## Consequences

- Analysts get a first draft in a few seconds. Instances without a key see no change.
- With a key set, ticket text, internal Notes included, goes to Google. On the free tier,
  Google may use prompts to improve its models, so only test data should be used there.
  Production use needs a paid key and a data-processing agreement. `.env.example` says
  so.
- One provider and model for all Companies. Per-Company settings with an encrypted key
  (Option 3) supersede the environment variables when FR-AI-001 or a Company asks for
  them. The `ReplySuggester` port stays.
- Other providers (Anthropic, OpenAI, a local model) are a new `ReplySuggester`
  implementation behind its own feature.
- There is no rate limiting per Analyst yet. The provider's quota and the request timeout
  bound the cost. Add a limit if usage grows.
