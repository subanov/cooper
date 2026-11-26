-- Asset owners and warranty/lease reminders (FR-AST-012; ADR-0042).

-- The Analyst responsible for an Asset. Reminders go to them.
CREATE TABLE asset_owners (
    asset_id   uuid PRIMARY KEY REFERENCES assets (id),
    tenant_id  uuid        NOT NULL REFERENCES tenants (id),
    owner_id   uuid        NOT NULL REFERENCES analysts (id),
    updated_at timestamptz NOT NULL
);
CREATE INDEX asset_owners_tenant_idx ON asset_owners (tenant_id);

-- Each reminder is sent once per Asset, date kind and date.
CREATE TABLE asset_reminders (
    asset_id      uuid        NOT NULL REFERENCES assets (id),
    tenant_id     uuid        NOT NULL REFERENCES tenants (id),
    kind          text        NOT NULL CHECK (kind IN ('warranty', 'lease')),
    reminded_date date        NOT NULL,
    owner_id      uuid        NOT NULL REFERENCES analysts (id),
    sent_at       timestamptz NOT NULL,
    PRIMARY KEY (asset_id, kind, reminded_date)
);
CREATE INDEX asset_reminders_tenant_idx ON asset_reminders (tenant_id);

DO $$
DECLARE t text;
BEGIN
    FOREACH t IN ARRAY ARRAY['asset_owners', 'asset_reminders'] LOOP
        EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY', t);
        EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY', t);
        EXECUTE format('CREATE POLICY %I ON %I USING (cooper_tenant_visible(tenant_id)) WITH CHECK (cooper_tenant_visible(tenant_id))',
                       t || '_isolation', t);
    END LOOP;
END $$;

-- Owners are reminded of warranty and lease expiry.
ALTER TABLE notifications DROP CONSTRAINT notifications_kind_check;
ALTER TABLE notifications ADD CONSTRAINT notifications_kind_check
    CHECK (kind IN ('ticket_assigned', 'mentioned', 'sla_warning', 'sla_breached', 'reply_received', 'mailbox_failing',
                    'change_submitted', 'change_approved', 'change_rejected', 'change_scheduled', 'change_completed',
                    'article_review_due', 'task_assigned', 'checks_unrecorded',
                    'contract_notice_due', 'contract_end_due', 'asset_warranty_due', 'asset_lease_due'));
