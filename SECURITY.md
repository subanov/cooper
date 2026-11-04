# Security policy

## Reporting a vulnerability

Please **do not open a public issue**. Report vulnerabilities privately through
[GitHub security advisories](https://github.com/subanov/cooper/security/advisories/new).
Include:

- what an attacker can do, and the affected component or endpoint;
- steps to reproduce or a proof of concept;
- the commit or version you tested.

We aim to acknowledge reports within 3 working days and to agree on a disclosure date
with you once a fix is available.

## Supported versions

Cooper has not had a release yet. Fixes go to `main`.

## Security baseline

The design rules are in [AGENTS.md §10](AGENTS.md#10-security-baseline). In short:
Argon2id password hashing, TOTP 2FA, API keys stored as prefix + hash, secrets encrypted
at rest, an append-only audit trail for every mutation, tenant isolation enforced in the
application and by PostgreSQL Row-Level Security, and sanitised HTML.
