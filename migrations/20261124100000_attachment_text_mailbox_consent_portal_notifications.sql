-- Attachment text extraction (FR-EML-007), Mailbox consent (FR-EML-001, FR-EML-012) and
-- Portal Notifications with Notification Preferences (FR-PRT-007). ADR-0033.

-- Text extracted from attachments for ticket search. The extraction job fills it; the
-- status column already exists (ADR-0007).
ALTER TABLE attachments
    ADD COLUMN extracted_text      text,
    ADD COLUMN extraction_error    text,
    ADD COLUMN extraction_attempts integer     NOT NULL DEFAULT 0 CHECK (extraction_attempts >= 0),
    ADD COLUMN extraction_retry_at timestamptz,
    ADD COLUMN extracted_at        timestamptz,
    ADD COLUMN search_vector       tsvector GENERATED ALWAYS AS (
        to_tsvector('simple', cooper_unaccent(left(coalesce(extracted_text, ''), 100000)))
    ) STORED;
COMMENT ON COLUMN attachments.extracted_text IS 'Searchable text (normalised, at most 100000 characters); NULL until extracted. FreeITSM: attachment_text.extracted_text.';
CREATE INDEX attachments_search_idx ON attachments USING gin (search_vector);
CREATE INDEX attachments_extraction_pending_idx ON attachments (created_at, id) WHERE extraction_status = 'pending';

-- Mailbox consent requests in progress: the state (stored hashed), the PKCE verifier and
-- the Administrator who started it. Single use, short-lived. Mailboxes are instance-wide,
-- so this table is too.
CREATE TABLE mailbox_consents (
    state_hash    bytea PRIMARY KEY,
    mailbox_id    uuid        NOT NULL REFERENCES mailboxes (id) ON DELETE CASCADE,
    analyst_id    uuid        NOT NULL REFERENCES analysts (id) ON DELETE CASCADE,
    pkce_verifier text        NOT NULL,
    created_at    timestamptz NOT NULL,
    expires_at    timestamptz NOT NULL
);
CREATE INDEX mailbox_consents_expires_idx ON mailbox_consents (expires_at);
CREATE INDEX mailbox_consents_mailbox_idx ON mailbox_consents (mailbox_id);

-- Portal Notifications: a Requester's in-app notifications on the Portal.
CREATE TABLE portal_notifications (
    id              uuid PRIMARY KEY,
    tenant_id       uuid        NOT NULL REFERENCES tenants (id),
    requester_id    uuid        NOT NULL REFERENCES requesters (id) ON DELETE CASCADE,
    kind            text        NOT NULL CHECK (kind IN ('ticket_created', 'ticket_replied', 'ticket_closed')),
    title           text        NOT NULL CHECK (length(title) BETWEEN 1 AND 300),
    body            text        NOT NULL DEFAULT '',
    ticket_id       uuid,
    -- At-least-once delivery: one Notification per source event and Requester.
    source_event_id uuid        NOT NULL,
    created_at      timestamptz NOT NULL,
    read_at         timestamptz
);
CREATE UNIQUE INDEX portal_notifications_source_key ON portal_notifications (source_event_id, requester_id);
CREATE INDEX portal_notifications_requester_idx ON portal_notifications (requester_id, created_at DESC, id DESC);
CREATE INDEX portal_notifications_unread_idx ON portal_notifications (requester_id) WHERE read_at IS NULL;
CREATE INDEX portal_notifications_tenant_id_idx ON portal_notifications (tenant_id);
ALTER TABLE portal_notifications ENABLE ROW LEVEL SECURITY;
ALTER TABLE portal_notifications FORCE ROW LEVEL SECURITY;
CREATE POLICY portal_notifications_isolation ON portal_notifications
    USING (cooper_tenant_visible(tenant_id)) WITH CHECK (cooper_tenant_visible(tenant_id));

-- Notification Preferences of a Requester: per kind, in-app and/or email. No row means
-- both on.
CREATE TABLE portal_notification_preferences (
    tenant_id    uuid        NOT NULL REFERENCES tenants (id),
    requester_id uuid        NOT NULL REFERENCES requesters (id) ON DELETE CASCADE,
    kind         text        NOT NULL CHECK (kind IN ('ticket_created', 'ticket_replied', 'ticket_closed')),
    in_app       boolean     NOT NULL,
    email        boolean     NOT NULL,
    updated_at   timestamptz NOT NULL,
    PRIMARY KEY (requester_id, kind)
);
CREATE INDEX portal_notification_preferences_tenant_id_idx ON portal_notification_preferences (tenant_id);
ALTER TABLE portal_notification_preferences ENABLE ROW LEVEL SECURITY;
ALTER TABLE portal_notification_preferences FORCE ROW LEVEL SECURITY;
CREATE POLICY portal_notification_preferences_isolation ON portal_notification_preferences
    USING (cooper_tenant_visible(tenant_id)) WITH CHECK (cooper_tenant_visible(tenant_id));
