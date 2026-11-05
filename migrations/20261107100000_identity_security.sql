-- Identity security (ADR-0013): two-factor sign-in, trusted devices, IP bans and
-- API Keys. Secrets are encrypted by the application (`v<key>.<nonce>.<ciphertext>`).

ALTER TABLE analysts
    ADD COLUMN totp_secret_encrypted  text,
    ADD COLUMN totp_pending_encrypted text,
    ADD COLUMN totp_enabled_at        timestamptz,
    ADD COLUMN totp_last_step         bigint,
    ADD CONSTRAINT analysts_totp_enabled_matches_secret
        CHECK ((totp_secret_encrypted IS NULL) = (totp_enabled_at IS NULL));

-- Browsers that skip the two-factor code for 30 days. Only token hashes are stored.
CREATE TABLE analyst_trusted_devices (
    id          uuid PRIMARY KEY,
    analyst_id  uuid        NOT NULL REFERENCES analysts (id) ON DELETE CASCADE,
    token_hash  bytea       NOT NULL,
    created_at  timestamptz NOT NULL,
    expires_at  timestamptz NOT NULL,
    user_agent  text
);
CREATE UNIQUE INDEX analyst_trusted_devices_token_hash_key ON analyst_trusted_devices (token_hash);
CREATE INDEX analyst_trusted_devices_analyst_id_idx ON analyst_trusted_devices (analyst_id);

-- Failed sign-ins per client IP and temporary bans (FR-IAM-005).
CREATE TABLE sign_in_failures (
    id          uuid PRIMARY KEY,
    ip_address  inet        NOT NULL,
    login       text        NOT NULL,
    occurred_at timestamptz NOT NULL
);
CREATE INDEX sign_in_failures_ip_idx ON sign_in_failures (ip_address, occurred_at);

CREATE TABLE ip_bans (
    ip_address   inet PRIMARY KEY,
    banned_until timestamptz NOT NULL,
    created_at   timestamptz NOT NULL
);

-- API Keys (FR-API-002). Only the SHA-256 of the full key is stored.
CREATE TABLE api_keys (
    id                    uuid PRIMARY KEY,
    name                  text        NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 200),
    prefix                text        NOT NULL CHECK (prefix ~ '^[0-9a-f]{8}$'),
    key_hash              bytea       NOT NULL,
    capabilities          text[]      NOT NULL DEFAULT '{}',
    all_companies         boolean     NOT NULL DEFAULT false,
    rate_limit_per_minute integer     NOT NULL CHECK (rate_limit_per_minute BETWEEN 1 AND 10000),
    expires_at            timestamptz,
    last_used_at          timestamptz,
    revoked_at            timestamptz,
    created_by            uuid REFERENCES analysts (id) ON DELETE SET NULL,
    created_at            timestamptz NOT NULL,
    updated_at            timestamptz NOT NULL
);
COMMENT ON TABLE api_keys IS 'API Keys for integrations; replaces FreeITSM apikeys (plaintext) and api_keys.';
CREATE UNIQUE INDEX api_keys_prefix_key ON api_keys (prefix);
CREATE INDEX api_keys_name_idx ON api_keys (lower(name), id);

CREATE TABLE api_key_companies (
    api_key_id uuid NOT NULL REFERENCES api_keys (id) ON DELETE CASCADE,
    tenant_id  uuid NOT NULL REFERENCES tenants (id) ON DELETE CASCADE,
    PRIMARY KEY (api_key_id, tenant_id)
);

-- Requests per key per minute, for rate limiting across server instances.
CREATE TABLE api_key_usage (
    api_key_id   uuid        NOT NULL REFERENCES api_keys (id) ON DELETE CASCADE,
    window_start timestamptz NOT NULL,
    requests     integer     NOT NULL,
    PRIMARY KEY (api_key_id, window_start)
);
