-- Attachments (ADR-0007), outbound email with an attempt log (FR-EML-008), ticket emails,
-- Reply Templates, Analyst Signatures and password reset tokens.

-- Attachment metadata; content lives in the blob store under its SHA-256.
CREATE TABLE attachments (
    id                uuid PRIMARY KEY,
    tenant_id         uuid        NOT NULL REFERENCES tenants (id),
    owner_type        text        NOT NULL CHECK (owner_type ~ '^[a-z_]+$'),
    owner_id          uuid        NOT NULL,
    file_name         text        NOT NULL CHECK (length(file_name) BETWEEN 1 AND 255),
    media_type        text        NOT NULL,
    size_bytes        bigint      NOT NULL CHECK (size_bytes >= 0),
    sha256            text        NOT NULL CHECK (sha256 ~ '^[0-9a-f]{64}$'),
    extraction_status text        NOT NULL DEFAULT 'pending'
        CHECK (extraction_status IN ('pending', 'extracted', 'truncated', 'too_large', 'unsupported', 'failed')),
    created_by_kind   text        NOT NULL,
    created_by_id     uuid,
    created_at        timestamptz NOT NULL
);
CREATE INDEX attachments_owner_idx ON attachments (owner_type, owner_id, created_at);
CREATE INDEX attachments_tenant_id_idx ON attachments (tenant_id);
CREATE INDEX attachments_sha256_idx ON attachments (sha256);
ALTER TABLE attachments ENABLE ROW LEVEL SECURITY;
ALTER TABLE attachments FORCE ROW LEVEL SECURITY;
CREATE POLICY attachments_isolation ON attachments
    USING (cooper_tenant_visible(tenant_id)) WITH CHECK (cooper_tenant_visible(tenant_id));

-- Outbound email queue and send log (FR-EML-008).
CREATE TABLE outbound_emails (
    id              uuid PRIMARY KEY,
    tenant_id       uuid REFERENCES tenants (id),
    kind            text        NOT NULL CHECK (kind ~ '^[a-z_]+$'),
    from_address    text        NOT NULL,
    to_addresses    text[]      NOT NULL CHECK (cardinality(to_addresses) BETWEEN 1 AND 50),
    cc_addresses    text[]      NOT NULL DEFAULT '{}',
    subject         text        NOT NULL,
    text_body       text        NOT NULL,
    message_id      text        NOT NULL,
    in_reply_to     text,
    references_ids  text[]      NOT NULL DEFAULT '{}',
    status          text        NOT NULL CHECK (status IN ('queued', 'sent', 'dead')),
    attempts        integer     NOT NULL DEFAULT 0,
    next_attempt_at timestamptz NOT NULL,
    last_error      text,
    sent_at         timestamptz,
    created_at      timestamptz NOT NULL
);
COMMENT ON TABLE outbound_emails IS 'Outbound email queue: sent by the email-sender job with retries (FR-EML-008).';
CREATE UNIQUE INDEX outbound_emails_message_id_key ON outbound_emails (message_id);
CREATE INDEX outbound_emails_due_idx ON outbound_emails (next_attempt_at) WHERE status = 'queued';
ALTER TABLE outbound_emails ENABLE ROW LEVEL SECURITY;
ALTER TABLE outbound_emails FORCE ROW LEVEL SECURITY;
CREATE POLICY outbound_emails_isolation ON outbound_emails
    USING (tenant_id IS NULL OR cooper_tenant_visible(tenant_id))
    WITH CHECK (tenant_id IS NULL OR cooper_tenant_visible(tenant_id));

CREATE TABLE outbound_email_attempts (
    id           uuid PRIMARY KEY,
    email_id     uuid        NOT NULL REFERENCES outbound_emails (id) ON DELETE CASCADE,
    attempted_at timestamptz NOT NULL,
    succeeded    boolean     NOT NULL,
    error        text
);
CREATE INDEX outbound_email_attempts_email_idx ON outbound_email_attempts (email_id, attempted_at);

-- Emails on a ticket's timeline (outbound Replies now; inbound with Mailboxes).
CREATE TABLE ticket_emails (
    id           uuid PRIMARY KEY,
    tenant_id    uuid        NOT NULL REFERENCES tenants (id),
    ticket_id    uuid        NOT NULL REFERENCES tickets (id),
    direction    text        NOT NULL CHECK (direction IN ('inbound', 'outbound')),
    from_address text        NOT NULL,
    to_addresses text[]      NOT NULL,
    cc_addresses text[]      NOT NULL DEFAULT '{}',
    subject      text        NOT NULL,
    text_body    text        NOT NULL,
    message_id   text        NOT NULL,
    author_id    uuid REFERENCES analysts (id),
    occurred_at  timestamptz NOT NULL,
    search_vector tsvector GENERATED ALWAYS AS (to_tsvector('simple', cooper_unaccent(subject || ' ' || left(text_body, 100000)))) STORED
);
CREATE INDEX ticket_emails_ticket_idx ON ticket_emails (ticket_id, occurred_at);
CREATE INDEX ticket_emails_message_id_idx ON ticket_emails (message_id);
CREATE INDEX ticket_emails_tenant_id_idx ON ticket_emails (tenant_id);
CREATE INDEX ticket_emails_search_idx ON ticket_emails USING gin (search_vector);
ALTER TABLE ticket_emails ENABLE ROW LEVEL SECURITY;
ALTER TABLE ticket_emails FORCE ROW LEVEL SECURITY;
CREATE POLICY ticket_emails_isolation ON ticket_emails
    USING (cooper_tenant_visible(tenant_id)) WITH CHECK (cooper_tenant_visible(tenant_id));

-- Reply Templates (FR-TKT-022).
CREATE TABLE reply_templates (
    id         uuid PRIMARY KEY,
    name       text        NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 100),
    body       text        NOT NULL CHECK (length(body) BETWEEN 1 AND 20000),
    is_active  boolean     NOT NULL DEFAULT true,
    created_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL
);
CREATE UNIQUE INDEX reply_templates_name_key ON reply_templates (lower(name));

-- Analyst Signatures, appended to Replies.
ALTER TABLE analysts ADD COLUMN signature text NOT NULL DEFAULT '' CHECK (length(signature) <= 2000);

-- Password reset tokens (FR-IAM-004). Only hashes are stored.
CREATE TABLE password_reset_tokens (
    token_hash bytea PRIMARY KEY,
    analyst_id uuid        NOT NULL REFERENCES analysts (id) ON DELETE CASCADE,
    created_at timestamptz NOT NULL,
    expires_at timestamptz NOT NULL,
    used_at    timestamptz
);
CREATE INDEX password_reset_tokens_analyst_idx ON password_reset_tokens (analyst_id);
