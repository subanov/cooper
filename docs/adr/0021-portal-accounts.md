# 0021. Portal Accounts and Portal sessions

- **Status:** Accepted
- **Date:** 2026-11-05
- **Issue:** #72
- **Requirements:** FR-PRT-001, FR-IAM-004, FR-IAM-005, FR-IAM-011
- **Related:** [ADR-0011](0011-identity-context-sessions-and-rbac.md),
  [ADR-0013](0013-two-factor-ip-bans-api-keys-and-encryption-at-rest.md)

## Context

Requesters belong to one Company each, and their email address is unique only within
that Company: the same person can be a Requester in several Companies (a contractor of
an MSP, a shared service). The Portal still needs one sign-in per person, it must confirm
that the person owns the address, and a Requester must never reach Analyst endpoints.

FreeITSM keeps a password on each `users` row and lets anyone register with any address.

## Decision

- A **Portal Account** (`portal_accounts`) belongs to an email address (unique,
  case-insensitive), not to a Requester row. It holds an Argon2id password hash. It acts
  for every **active** Requester with that address in an active Company; the session
  stores the current one, and `PUT /api/v1/portal/session/requester` switches.
- **Passwords are only set through an emailed one-time link** (`portal_links`, stored as
  SHA-256 hashes, single use):
  - **Portal Invitation** — an Analyst with `requesters.manage` sends it
    (`POST /api/v1/requesters/{id}/portal-invitation`); valid 7 days.
  - **Reset** — `POST /api/v1/portal/password-reset` for any address that has an active
    Requester (so it doubles as "first sign-in" for known Requesters); valid 60 minutes.
    The answer is always `202`.
  - **Self-Registration** — off by default (`portal.self_registration`). A person enters
    name and email; the address must belong to a Company's Sender Domain and never to a
    Freemail Domain. Valid 24 hours. When the link is used, the Requester is created in
    that Company (or the existing one is used). If an active Requester already has the
    address, a reset link is sent instead, so the answer never reveals who is known.

  `POST /api/v1/portal/password` with the link's token sets the password, creating the
  Portal Account on first use, and ends the account's other Portal sessions. Because
  the password is chosen only after the link is opened, nobody can pre-register someone
  else's address with a password they know.
- **Portal sessions** live in `requester_sessions` (same policy as Analyst sessions:
  hashed token, CSRF token, idle and absolute expiry) and use their own cookie,
  `cooper_portal_session`, with `Path=/api/v1/portal`. The authentication middleware
  reads it **only** for `/api/v1/portal/*`, and reads Analyst sessions and API keys
  **only** outside it. An Analyst and a Requester can be signed in from one browser
  without one replacing the other, and a Portal cookie never authenticates an Analyst
  endpoint.
- A Portal principal is `Actor::Requester`, scoped to the current Requester's Company,
  with **no Capabilities**. Application services additionally refuse Requester
  principals for staff use cases (`require_staff`), as a second line of defence.
- Failed Portal sign-ins count towards the same IP ban as Analyst sign-ins (ADR-0013).

## Consequences

- SSO for Requesters (OIDC, FR-IAM-012) is P2 and will link a verified email to the same
  Portal Account.
- Deactivating every Requester with an address (or their Companies) makes its Portal
  Account unusable without deleting it; reactivating restores access.
- Portal endpoints of other contexts (Catalogue, my tickets) live under `/api/v1/portal`
  and take the Requester from the principal, never from the request body.
- The `Portal` Module switch turns every `/api/v1/portal` endpoint off.
