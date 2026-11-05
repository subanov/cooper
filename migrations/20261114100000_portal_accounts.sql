-- Portal Accounts, Portal sessions and set-password links (FR-PRT-001, ADR-0021).
-- These rows are read before any Company is known (sign-in, cookie lookup), like
-- analyst_sessions, so they are not tenant-scoped; Requesters stay behind RLS.

CREATE TABLE portal_accounts (
    id                  uuid PRIMARY KEY,
    email               text        NOT NULL CHECK (position('@' IN email) > 1),
    password_hash       text        NOT NULL,
    password_changed_at timestamptz NOT NULL,
    created_at          timestamptz NOT NULL,
    updated_at          timestamptz NOT NULL
);
COMMENT ON TABLE portal_accounts IS 'Portal sign-in of one email address; acts for every active Requester with it (ADR-0021).';
CREATE UNIQUE INDEX portal_accounts_email_key ON portal_accounts (lower(email));

CREATE TABLE requester_sessions (
    id                uuid PRIMARY KEY,
    portal_account_id uuid        NOT NULL REFERENCES portal_accounts (id) ON DELETE CASCADE,
    requester_id      uuid        NOT NULL REFERENCES requesters (id) ON DELETE CASCADE,
    token_hash        bytea       NOT NULL,
    csrf_token        text        NOT NULL,
    created_at        timestamptz NOT NULL,
    last_seen_at      timestamptz NOT NULL,
    idle_expires_at   timestamptz NOT NULL,
    expires_at        timestamptz NOT NULL,
    revoked_at        timestamptz,
    ip_address        inet,
    user_agent        text
);
CREATE UNIQUE INDEX requester_sessions_token_hash_key ON requester_sessions (token_hash);
CREATE INDEX requester_sessions_account_idx ON requester_sessions (portal_account_id);
CREATE INDEX requester_sessions_requester_idx ON requester_sessions (requester_id);
CREATE INDEX requester_sessions_expires_at_idx ON requester_sessions (expires_at);

-- One-time links that set a Portal password. Only hashes are stored.
CREATE TABLE portal_links (
    token_hash bytea PRIMARY KEY,
    purpose    text        NOT NULL CHECK (purpose IN ('invitation', 'reset', 'registration')),
    email      text        NOT NULL CHECK (position('@' IN email) > 1),
    full_name  text        CHECK (full_name IS NULL OR length(btrim(full_name)) BETWEEN 1 AND 200),
    created_at timestamptz NOT NULL,
    expires_at timestamptz NOT NULL,
    used_at    timestamptz,
    CONSTRAINT portal_links_registration_has_name CHECK ((purpose = 'registration') = (full_name IS NOT NULL))
);
CREATE INDEX portal_links_email_idx ON portal_links (lower(email), purpose);
CREATE INDEX portal_links_expires_at_idx ON portal_links (expires_at);
