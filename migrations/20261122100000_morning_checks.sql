-- Morning Checks (FR-MCK-001 to -006; ADR-0031).

-- Check Statuses (FR-MCK-002): reference data shared by every Company. Behaviour lives
-- on the row (`requires_notes`); no status name is hard-coded. Statuses are deactivated,
-- never deleted, so recorded results always keep their status.
CREATE TABLE check_statuses (
    id             uuid PRIMARY KEY,
    key            text UNIQUE,
    name           text        NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 50),
    colour         text        NOT NULL CHECK (colour ~ '^#[0-9a-fA-F]{6}$'),
    requires_notes boolean     NOT NULL DEFAULT false,
    sort_order     integer     NOT NULL DEFAULT 0,
    is_active      boolean     NOT NULL DEFAULT true,
    created_at     timestamptz NOT NULL,
    updated_at     timestamptz NOT NULL
);
CREATE UNIQUE INDEX check_statuses_name_key ON check_statuses (lower(name));

INSERT INTO check_statuses (id, key, name, colour, requires_notes, sort_order, created_at, updated_at) VALUES
    ('019a0000-0000-7000-8000-000000000c01', 'green', 'Green', '#28a745', false, 10, now(), now()),
    ('019a0000-0000-7000-8000-000000000c02', 'amber', 'Amber', '#ffc107', true,  20, now(), now()),
    ('019a0000-0000-7000-8000-000000000c03', 'red',   'Red',   '#dc3545', true,  30, now(), now());

-- Check Groups (FR-MCK-001): routed to a Team or an Analyst (never both).
CREATE TABLE check_groups (
    id                  uuid PRIMARY KEY,
    tenant_id           uuid        NOT NULL REFERENCES tenants (id),
    name                text        NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 100),
    description         text        NOT NULL DEFAULT '' CHECK (length(description) <= 2000),
    assigned_team_id    uuid REFERENCES teams (id),
    assigned_analyst_id uuid REFERENCES analysts (id),
    sort_order          integer     NOT NULL DEFAULT 0,
    is_active           boolean     NOT NULL DEFAULT true,
    created_at          timestamptz NOT NULL,
    updated_at          timestamptz NOT NULL,
    CHECK (assigned_team_id IS NULL OR assigned_analyst_id IS NULL),
    UNIQUE (id, tenant_id)
);
CREATE UNIQUE INDEX check_groups_name_key ON check_groups (tenant_id, lower(name));

-- Checks (FR-MCK-001). A Check's group is of the same Company (composite key).
CREATE TABLE checks (
    id                  uuid PRIMARY KEY,
    tenant_id           uuid        NOT NULL REFERENCES tenants (id),
    check_group_id      uuid,
    name                text        NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 255),
    description         text        NOT NULL DEFAULT '' CHECK (length(description) <= 2000),
    assigned_analyst_id uuid REFERENCES analysts (id),
    sort_order          integer     NOT NULL DEFAULT 0,
    is_active           boolean     NOT NULL DEFAULT true,
    created_at          timestamptz NOT NULL,
    updated_at          timestamptz NOT NULL,
    UNIQUE (id, tenant_id),
    FOREIGN KEY (check_group_id, tenant_id) REFERENCES check_groups (id, tenant_id)
);
CREATE INDEX checks_group_idx ON checks (check_group_id);

-- Check Results (FR-MCK-003): one per Check per Check Date. Who first recorded it and
-- who last changed it are both kept.
CREATE TABLE check_results (
    id               uuid PRIMARY KEY,
    tenant_id        uuid        NOT NULL REFERENCES tenants (id),
    check_id         uuid        NOT NULL,
    check_date       date        NOT NULL,
    status_id        uuid        NOT NULL REFERENCES check_statuses (id),
    notes            text        NOT NULL DEFAULT '' CHECK (length(notes) <= 10000),
    recorded_by_kind text        NOT NULL,
    recorded_by_id   uuid,
    recorded_at      timestamptz NOT NULL,
    modified_by_kind text        NOT NULL,
    modified_by_id   uuid,
    updated_at       timestamptz NOT NULL,
    CONSTRAINT check_results_one_per_day UNIQUE (check_id, check_date),
    FOREIGN KEY (check_id, tenant_id) REFERENCES checks (id, tenant_id)
);
CREATE INDEX check_results_date_idx ON check_results (check_date, check_id);

-- Result Links (FR-MCK-004) to follow-up tickets and Tasks of the same Company. Clearing
-- a result removes its links, never the tickets or Tasks themselves.
CREATE TABLE check_result_links (
    tenant_id       uuid        NOT NULL REFERENCES tenants (id),
    check_result_id uuid        NOT NULL REFERENCES check_results (id) ON DELETE CASCADE,
    kind            text        NOT NULL CHECK (kind IN ('ticket', 'task')),
    target_id       uuid        NOT NULL,
    linked_by_kind  text        NOT NULL,
    linked_by_id    uuid,
    linked_at       timestamptz NOT NULL,
    PRIMARY KEY (check_result_id, kind, target_id)
);
CREATE INDEX check_result_links_target_idx ON check_result_links (kind, target_id);

-- Reminders sent for unrecorded Checks (FR-MCK-006): one per Check Date, Check Group
-- (none for ungrouped Checks) and Analyst, so a worker restart never repeats one.
CREATE TABLE check_reminders (
    id             uuid PRIMARY KEY,
    tenant_id      uuid        NOT NULL REFERENCES tenants (id),
    check_date     date        NOT NULL,
    check_group_id uuid REFERENCES check_groups (id),
    analyst_id     uuid        NOT NULL REFERENCES analysts (id),
    unrecorded     integer     NOT NULL CHECK (unrecorded > 0),
    sent_at        timestamptz NOT NULL,
    CONSTRAINT check_reminders_once UNIQUE NULLS NOT DISTINCT (tenant_id, check_date, check_group_id, analyst_id)
);

DO $$
DECLARE t text;
BEGIN
    FOREACH t IN ARRAY ARRAY['check_groups', 'checks', 'check_results', 'check_result_links', 'check_reminders'] LOOP
        EXECUTE format('CREATE INDEX %I ON %I (tenant_id)', t || '_tenant_idx', t);
        EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY', t);
        EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY', t);
        EXECUTE format('CREATE POLICY %I ON %I USING (cooper_tenant_visible(tenant_id)) WITH CHECK (cooper_tenant_visible(tenant_id))',
                       t || '_isolation', t);
    END LOOP;
END $$;

-- Reminder Notifications (FR-MCK-006).
ALTER TABLE notifications DROP CONSTRAINT notifications_kind_check;
ALTER TABLE notifications ADD CONSTRAINT notifications_kind_check
    CHECK (kind IN ('ticket_assigned', 'mentioned', 'sla_warning', 'sla_breached', 'reply_received', 'mailbox_failing',
                    'change_submitted', 'change_approved', 'change_rejected', 'change_scheduled', 'change_completed',
                    'article_review_due', 'task_assigned', 'checks_unrecorded'));
