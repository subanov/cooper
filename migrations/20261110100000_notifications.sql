-- In-app Notifications (FR-GEN-006, ADR-0016).
CREATE TABLE notifications (
    id              uuid PRIMARY KEY,
    recipient_id    uuid        NOT NULL REFERENCES analysts (id) ON DELETE CASCADE,
    -- Company of the record the Notification is about; NULL for instance-wide ones.
    tenant_id       uuid REFERENCES tenants (id),
    kind            text        NOT NULL CHECK (kind IN ('ticket_assigned', 'mentioned', 'sla_warning', 'sla_breached', 'reply_received')),
    title           text        NOT NULL CHECK (length(title) BETWEEN 1 AND 300),
    body            text        NOT NULL DEFAULT '',
    ticket_id       uuid,
    -- The domain event that caused it: handlers may run more than once (at-least-once
    -- delivery), the unique index makes that harmless.
    source_event_id uuid        NOT NULL,
    created_at      timestamptz NOT NULL,
    read_at         timestamptz
);
CREATE UNIQUE INDEX notifications_source_key ON notifications (source_event_id, recipient_id);
CREATE INDEX notifications_recipient_idx ON notifications (recipient_id, created_at DESC, id DESC);
CREATE INDEX notifications_unread_idx ON notifications (recipient_id) WHERE read_at IS NULL;
ALTER TABLE notifications ENABLE ROW LEVEL SECURITY;
ALTER TABLE notifications FORCE ROW LEVEL SECURITY;
CREATE POLICY notifications_isolation ON notifications
    USING (tenant_id IS NULL OR cooper_tenant_visible(tenant_id))
    WITH CHECK (tenant_id IS NULL OR cooper_tenant_visible(tenant_id));
