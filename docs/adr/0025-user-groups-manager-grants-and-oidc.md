# 0025. User Groups, Manager Grants for the Portal, and OIDC single sign-on verified with ring

- **Status:** Accepted
- **Date:** 2026-11-07
- **Issue:** #94, #95, #96
- **Requirements:** FR-IAM-012, FR-IAM-015, FR-IAM-016, FR-PRT-004
- **Related:** [ADR-0011](0011-identity-context-sessions-and-rbac.md),
  [ADR-0013](0013-two-factor-ip-bans-api-keys-and-encryption-at-rest.md),
  [ADR-0021](0021-portal-accounts.md)

## Context

FreeITSM 4.10 (`includes/managers.php`, `auth_providers`, `analyst_sso_identities`,
`knowledge_user_groups`) lets Portal users act as managers, keeps groups of people for
permissions, and signs people in through OIDC providers. Its manager notes are clear and
Cooper keeps their rules:

- the feature is off until switched on, and fails closed;
- an exclusion always wins, whatever granted access (including the directory);
- coverage stays inside the manager's Company;
- managers who have left see nothing;
- Confidential tickets are hidden unless configured.

## Decision

### User Groups

Instance-wide groups (`user_groups`, `user_group_members`) of Analysts and/or
Requesters, each membership with an optional expiry. Inactive groups and expired
memberships count for nothing. Managed with capability `groups.manage`; membership
changes are audited as a whole list. Knowledge permissions (P2 Knowledge) will use them.

### Manager Grants and the Portal manager view

- `manager_grants` (tenant-scoped, RLS): `everyone`, `requester`, `group`,
  `cost_centre` or `reports`, optionally as an **exclusion** (`requester`, `group`,
  `cost_centre` only). Targets must be in the manager's Company. Managed with
  `requesters.manage`.
- Who a manager covers is a pure function
  (`cooper_identity::domain::managed_requesters`): directory reports (Requesters whose
  `manager_id` points at them; `portal.managers_directory` = `off`, `direct` or `all`),
  plus grants, minus exclusions, minus the manager, minus deactivated Requesters unless
  `portal.managers_leavers`. `portal.managers_enabled` (default **off**) gates everything.
- The service desk asks identity through its `Directory` port (`managed_scope`) and
  never reads identity tables. `GET /portal/managed-tickets` lists covered tickets, and
  `GET /portal/tickets/{id}` and its timeline open them. Managers **read only**: replies
  and attachments stay with each Requester's own tickets (FreeITSM makes reply and close
  settings; Cooper starts with the safe floor). Confidential tickets appear only with
  `portal.managers_confidential`.
- Deviation from FreeITSM: Cooper Requesters have no free-text department, so the
  `department` grant becomes `cost_centre`.

### OIDC single sign-on

- **Providers** (`sso_providers`, Administrators only): issuer, client ID, client
  secret (encrypted with the Keyring, bound to `sso_providers.client_secret:<id>`,
  re-encrypted on key rotation, never returned), scopes, audience (`analysts`,
  `requesters`, `both`), and whether a first sign-in may link by verified email. Issuers
  must be `https://` (plain `http://` only for `localhost`).
- **Flow:** authorization code with **PKCE (S256)**, `state` (random, stored as SHA-256,
  single use, 10 minutes) and `nonce`. The redirect URI is
  `<general.base_url>/api/v1/sso/callback`, so a forged `Host` header cannot redirect the
  code. `return_to` must be a local path; anything else falls back to `/` or `/portal`,
  so sign-in cannot be used as an open redirect.
- **ID token verification** in `cooper_identity::infrastructure::oidc`:
  - Discovery (whose `issuer` must equal the configured one) and JWKS are cached for an
    hour and the keys re-fetched once on an unknown `kid`.
  - Only **RS256** and **ES256** are accepted, verified with `ring`. `none`, HMAC and
    embedded keys are refused.
  - Claims (`check_claims`, pure): issuer, audience, `azp`, expiry and issue time with
    60 s leeway, nonce, non-empty subject.
- **Who signs in:** `sso_identities` links (provider, audience, subject) to an Analyst or
  a Portal Account email. A first sign-in links by **verified** email only (unverified
  email never matches). SSO never creates Analysts. For Requesters, an active Requester
  with the address must exist; a Portal Account is created if missing, with a random
  password usable only after a reset.
- **Second factor:** the identity provider is responsible for it. SSO sessions skip
  Cooper's TOTP prompt and enrollment requirement.
- **Why `ring` and not `jsonwebtoken`:** `jsonwebtoken` 11 needs either `aws-lc-rs` (a C
  build the project avoids, ADR-0023) or the `rsa` crate, which carries an unfixed
  advisory (RUSTSEC-2023-0071) that `cargo deny` rejects. `ring` is already in the tree
  through rustls; verifying two algorithms with it takes about a hundred lines. Those
  lines are covered by unit tests (tampering, wrong key, `none`/HS256) and by an
  end-to-end test against an in-process identity provider.
- New dependencies of `cooper-identity`: `reqwest` (already used by the service desk),
  `ring`, `tokio-rustls`, `webpki-roots` and `base64`, all already in the workspace.

## Consequences

- Group-to-Team mapping from SSO claims (FR-IAM-014) and LDAP (FR-IAM-013) are P3 and
  can reuse `sso_identities`.
- Managers covering "everyone" in a large Company send one ID list per request. That is
  fine at ITSM sizes; revisit with a set-based query if Companies reach tens of
  thousands of Requesters.
- Provider logout (RP-initiated logout) is not implemented. Signing out ends the Cooper
  session only.
