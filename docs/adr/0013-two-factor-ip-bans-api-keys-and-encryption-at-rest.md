# 0013. Two-factor sign-in, IP bans, API keys and encryption of secrets at rest

- **Status:** Accepted
- **Date:** 2026-11-07
- **Issue:** #32, #34, #79, #89
- **Requirements:** FR-IAM-003, FR-IAM-005, FR-API-002, FR-EML-012; Appendix A (ASVS L2)
- **Related:** [ADR-0011](0011-identity-context-sessions-and-rbac.md),
  [ADR-0012](0012-administration-context.md)

## Context

ADR-0011 left sign-in protection and integrations for later. FreeITSM supports TOTP with
remembered devices, bans IP addresses after failed logins, has two API key tables (one
storing keys in plain text) and encrypts mailbox credentials with a single key file and
no rotation.

## Decision

### Encryption at rest

`cooper_platform::crypto::Keyring` encrypts secrets with **AES-256-GCM**. Keys come from
`COOPER_ENCRYPTION_KEYS` (`<id>:<base64 32 bytes>`, active key first; required by the
server and the worker). Stored values are `v<key id>.<nonce>.<ciphertext>`, each bound to
a **purpose** (associated data such as `analyst.totp:<analyst id>`), so a value copied to
another row does not decrypt. Rotation: add a new key first, keep the old one; the
`identity-secrets-reencrypt` job (and later the mailbox equivalent) re-encrypts values
written with older keys; then remove the old key
([runbook](../runbooks/encryption-key-rotation.md)).

### Two-factor sign-in (TOTP)

- RFC 6238, six digits, 30-second steps, HMAC-SHA1 (what authenticator apps support),
  one step of skew. Implemented in the domain on `hmac` + `sha1` (a few dozen lines with
  the RFC test vectors) instead of a TOTP crate.
- **Codes are single-use**: the last accepted step is stored and earlier or equal steps
  are rejected.
- Enrollment: `POST /session/totp/enrollment` returns a secret and `otpauth://` URI; the
  first code confirms it. Secrets are encrypted at rest.
- Sign-in is **one request**: `POST /session` with `totp_code`. Without a code the answer
  is `401` `totp-required`. `remember_device` sets a 30-day `cooper_trusted_device` cookie
  (`HttpOnly`, `SameSite=Strict`, path `/api/v1/session`; only its hash is stored) that
  skips the code for that Analyst.
- `security.require_totp` (System Setting): Analysts without two-factor sign-in can only
  use the `/session` endpoints until they enroll; they cannot turn it off.
- Administrators reset an Analyst's two-factor sign-in (lost phone); this ends their
  sessions and forgets trusted devices.

### IP bans

Failed sign-ins (wrong password or wrong code) are recorded per client IP. Reaching
`security.max_failed_sign_ins` (default 10) within `security.sign_in_ban_minutes`
(default 15) bans the address for that long: every sign-in from it gets `429` with
`Retry-After`, even with the right password. The client IP is the TCP peer, or the
right-most untrusted `X-Forwarded-For` address when the peer is in
`COOPER_TRUSTED_PROXIES`.

### API keys

- Format `cpr_<8 hex prefix>_<64 hex secret>`. Only the SHA-256 of the whole key is
  stored (keys are random, so a slow hash is unnecessary); the key is shown once.
- Each key has Capabilities (from the catalogue), Company access, a rate limit per minute
  (counted in PostgreSQL per minute window, so it holds across server instances), an
  optional expiry and a revocation time; `last_used_at` is updated at most once a minute.
- `Authorization: Bearer` requests act as `Actor::ApiKey`; they never have Administrator
  rights and need no CSRF token. Only Administrators manage keys.

### Housekeeping

The hourly `identity-housekeeping` job deletes expired sessions and trusted devices, old
sign-in failures, expired bans and old usage counters.

## Consequences

- New dependencies: `aes-gcm`, `hmac`, `sha1`.
- `COOPER_ENCRYPTION_KEYS` is required in every environment; `.env.example` and
  `docker-compose.yml` carry a public development key.
- Mailbox credentials and OAuth tokens (P1 email work) use the same keyring.

### Deviations from FreeITSM

- One API key table with hashed keys only; FreeITSM's plaintext `apikeys` is not carried
  over.
- TOTP codes cannot be replayed within their validity window.
- Encryption keys are versioned and rotatable.
