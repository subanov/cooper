-- Mailboxes and email intake (ADR-0018). Instance-wide configuration; credentials are
-- encrypted by the application (ADR-0013).

CREATE TABLE mailboxes (
    id                      uuid PRIMARY KEY,
    name                    text        NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 100),
    address                 text        NOT NULL CHECK (position('@' IN address) > 1),
    provider                text        NOT NULL CHECK (provider IN ('imap', 'microsoft', 'google')),
    -- Microsoft Graph only.
    auth_mode               text CHECK (auth_mode IN ('delegated', 'app_only')),
    -- IMAP connection.
    imap_host               text,
    imap_port               integer CHECK (imap_port BETWEEN 1 AND 65535),
    imap_security           text CHECK (imap_security IN ('tls', 'starttls', 'none')),
    imap_username           text,
    folder                  text        NOT NULL DEFAULT 'INBOX',
    -- Password or OAuth credentials (JSON), encrypted: v<key>.<nonce>.<ciphertext>.
    secret_encrypted        text,
    -- Defaults for new tickets.
    tenant_id               uuid        NOT NULL REFERENCES tenants (id),
    department_id           uuid REFERENCES departments (id),
    origin_id               uuid REFERENCES ticket_origins (id),
    team_id                 uuid REFERENCES teams (id),
    post_import_action      text        NOT NULL DEFAULT 'mark_read' CHECK (post_import_action IN ('delete', 'move', 'mark_read')),
    move_to_folder          text,
    unknown_sender_action   text        NOT NULL DEFAULT 'create_requester' CHECK (unknown_sender_action IN ('create_requester', 'reject')),
    rejected_action         text        NOT NULL DEFAULT 'delete' CHECK (rejected_action IN ('delete', 'move', 'keep')),
    is_active               boolean     NOT NULL DEFAULT true,
    -- Health (FR-EML-010).
    last_polled_at          timestamptz,
    last_success_at         timestamptz,
    last_error              text,
    last_error_at           timestamptz,
    consecutive_failures    integer     NOT NULL DEFAULT 0,
    created_at              timestamptz NOT NULL,
    updated_at              timestamptz NOT NULL,
    CONSTRAINT mailboxes_move_folder CHECK (post_import_action <> 'move' OR move_to_folder IS NOT NULL),
    CONSTRAINT mailboxes_imap_settings CHECK (provider <> 'imap' OR (imap_host IS NOT NULL AND imap_port IS NOT NULL AND imap_security IS NOT NULL AND imap_username IS NOT NULL))
);
COMMENT ON TABLE mailboxes IS 'Target Mailboxes polled for tickets (FR-EML-001). FreeITSM: target_mailboxes.';
CREATE UNIQUE INDEX mailboxes_address_key ON mailboxes (lower(address));
CREATE UNIQUE INDEX mailboxes_name_key ON mailboxes (lower(name));

-- Mailbox Whitelist (FR-EML-005): an address or a domain, allowed or blocked.
CREATE TABLE mailbox_whitelist (
    id         uuid PRIMARY KEY,
    mailbox_id uuid        NOT NULL REFERENCES mailboxes (id) ON DELETE CASCADE,
    pattern    text        NOT NULL CHECK (pattern = lower(pattern) AND length(pattern) BETWEEN 3 AND 254),
    kind       text        NOT NULL CHECK (kind IN ('allow', 'block')),
    created_at timestamptz NOT NULL
);
CREATE UNIQUE INDEX mailbox_whitelist_key ON mailbox_whitelist (mailbox_id, pattern);

-- Rejected Emails (FR-EML-005, FR-EML-011): logged, whatever happens to the message.
CREATE TABLE rejected_emails (
    id           uuid PRIMARY KEY,
    mailbox_id   uuid        NOT NULL REFERENCES mailboxes (id) ON DELETE CASCADE,
    from_address text        NOT NULL,
    subject      text        NOT NULL,
    message_id   text,
    reason       text        NOT NULL,
    received_at  timestamptz NOT NULL
);
CREATE INDEX rejected_emails_mailbox_idx ON rejected_emails (mailbox_id, received_at DESC);

-- Inbound emails: which Mailbox and which Requester.
ALTER TABLE ticket_emails
    ADD COLUMN mailbox_id   uuid REFERENCES mailboxes (id),
    ADD COLUMN requester_id uuid REFERENCES requesters (id),
    ADD COLUMN in_reply_to  text;
CREATE INDEX ticket_emails_inbound_message_idx ON ticket_emails (message_id) WHERE direction = 'inbound';
