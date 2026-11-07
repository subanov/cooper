-- Change Management (FR-CHG-001 to -012, -014; ADR-0027).

-- Reference data (instance-wide). Seeds from FreeITSM 4.9.0 (Vocabulary §4), except the
-- Emergency type's route (FR-CHG-002).
CREATE TABLE change_types (
    id             uuid PRIMARY KEY,
    key            text UNIQUE CHECK (key ~ '^[a-z][a-z0-9_]*$'),
    name           text        NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 100),
    approval_route text        NOT NULL DEFAULT 'review' CHECK (approval_route IN ('review', 'emergency')),
    -- Pre-approved: may go to implementation without an approval round (FR-CHG-004).
    is_standard    boolean     NOT NULL DEFAULT false,
    is_default     boolean     NOT NULL DEFAULT false,
    sort_order     integer     NOT NULL DEFAULT 0,
    is_active      boolean     NOT NULL DEFAULT true,
    created_at     timestamptz NOT NULL,
    updated_at     timestamptz NOT NULL
);
CREATE TABLE change_statuses (
    id         uuid PRIMARY KEY,
    key        text UNIQUE CHECK (key ~ '^[a-z][a-z0-9_]*$'),
    name       text        NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 100),
    stage      text        NOT NULL CHECK (stage IN ('draft', 'awaiting', 'approved', 'implementing', 'done', 'stopped')),
    is_closed  boolean     NOT NULL DEFAULT false,
    is_default boolean     NOT NULL DEFAULT false,
    sort_order integer     NOT NULL DEFAULT 0,
    is_active  boolean     NOT NULL DEFAULT true,
    created_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL,
    CONSTRAINT change_statuses_default_is_draft CHECK (NOT is_default OR stage = 'draft')
);
CREATE TABLE change_priorities (
    id uuid PRIMARY KEY, key text UNIQUE CHECK (key ~ '^[a-z][a-z0-9_]*$'),
    name text NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 100),
    is_default boolean NOT NULL DEFAULT false, sort_order integer NOT NULL DEFAULT 0, is_active boolean NOT NULL DEFAULT true,
    created_at timestamptz NOT NULL, updated_at timestamptz NOT NULL
);
CREATE TABLE change_impacts (
    id uuid PRIMARY KEY, key text UNIQUE CHECK (key ~ '^[a-z][a-z0-9_]*$'),
    name text NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 100),
    is_default boolean NOT NULL DEFAULT false, sort_order integer NOT NULL DEFAULT 0, is_active boolean NOT NULL DEFAULT true,
    created_at timestamptz NOT NULL, updated_at timestamptz NOT NULL
);
CREATE TABLE change_categories (
    id uuid PRIMARY KEY, key text UNIQUE CHECK (key ~ '^[a-z][a-z0-9_]*$'),
    name text NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 100),
    is_default boolean NOT NULL DEFAULT false, sort_order integer NOT NULL DEFAULT 0, is_active boolean NOT NULL DEFAULT true,
    created_at timestamptz NOT NULL, updated_at timestamptz NOT NULL
);
DO $$
DECLARE t text;
BEGIN
    FOREACH t IN ARRAY ARRAY['change_types', 'change_statuses', 'change_priorities', 'change_impacts', 'change_categories'] LOOP
        EXECUTE format('CREATE UNIQUE INDEX %I ON %I (lower(name))', t || '_name_key', t);
        EXECUTE format('CREATE UNIQUE INDEX %I ON %I (is_default) WHERE is_default', t || '_one_default', t);
    END LOOP;
END $$;

-- Risk Level bands on the Risk Score (FR-CHG-005).
CREATE TABLE change_risk_levels (
    id         uuid PRIMARY KEY,
    name       text        NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 50),
    min_score  integer     NOT NULL CHECK (min_score BETWEEN 1 AND 25),
    max_score  integer     NOT NULL CHECK (max_score BETWEEN 1 AND 25),
    sort_order integer     NOT NULL DEFAULT 0,
    created_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL,
    CONSTRAINT change_risk_levels_range CHECK (min_score <= max_score)
);

INSERT INTO change_types (id, key, name, approval_route, is_standard, is_default, sort_order, created_at, updated_at) VALUES
    ('019a0000-0000-7000-8000-000000000a01', 'standard',  'Standard',  'review',    true,  false, 10, now(), now()),
    ('019a0000-0000-7000-8000-000000000a02', 'normal',    'Normal',    'review',    false, true,  20, now(), now()),
    ('019a0000-0000-7000-8000-000000000a03', 'emergency', 'Emergency', 'emergency', false, false, 30, now(), now());
INSERT INTO change_statuses (id, key, name, stage, is_closed, is_default, sort_order, created_at, updated_at) VALUES
    ('019a0000-0000-7000-8000-000000000a11', 'draft',            'Draft',            'draft',        false, true,  10,  now(), now()),
    ('019a0000-0000-7000-8000-000000000a12', 'submitted',        'Submitted',        'draft',        false, false, 20,  now(), now()),
    ('019a0000-0000-7000-8000-000000000a13', 'pending_approval', 'Pending Approval', 'awaiting',     false, false, 30,  now(), now()),
    ('019a0000-0000-7000-8000-000000000a14', 'approved',         'Approved',         'approved',     false, false, 40,  now(), now()),
    ('019a0000-0000-7000-8000-000000000a15', 'rejected',         'Rejected',         'stopped',      true,  false, 50,  now(), now()),
    ('019a0000-0000-7000-8000-000000000a16', 'scheduled',        'Scheduled',        'implementing', false, false, 60,  now(), now()),
    ('019a0000-0000-7000-8000-000000000a17', 'in_progress',      'In Progress',      'implementing', false, false, 70,  now(), now()),
    ('019a0000-0000-7000-8000-000000000a18', 'completed',        'Completed',        'done',         true,  false, 80,  now(), now()),
    ('019a0000-0000-7000-8000-000000000a19', 'failed',           'Failed',           'done',         true,  false, 90,  now(), now()),
    ('019a0000-0000-7000-8000-000000000a1a', 'cancelled',        'Cancelled',        'stopped',      true,  false, 100, now(), now());
INSERT INTO change_priorities (id, key, name, is_default, sort_order, created_at, updated_at) VALUES
    ('019a0000-0000-7000-8000-000000000a21', 'low',      'Low',      false, 10, now(), now()),
    ('019a0000-0000-7000-8000-000000000a22', 'medium',   'Medium',   true,  20, now(), now()),
    ('019a0000-0000-7000-8000-000000000a23', 'high',     'High',     false, 30, now(), now()),
    ('019a0000-0000-7000-8000-000000000a24', 'critical', 'Critical', false, 40, now(), now());
INSERT INTO change_impacts (id, key, name, is_default, sort_order, created_at, updated_at) VALUES
    ('019a0000-0000-7000-8000-000000000a31', 'low',    'Low',    false, 10, now(), now()),
    ('019a0000-0000-7000-8000-000000000a32', 'medium', 'Medium', true,  20, now(), now()),
    ('019a0000-0000-7000-8000-000000000a33', 'high',   'High',   false, 30, now(), now());
INSERT INTO change_risk_levels (id, name, min_score, max_score, sort_order, created_at, updated_at) VALUES
    ('019a0000-0000-7000-8000-000000000a41', 'Low',       1,  4,  10, now(), now()),
    ('019a0000-0000-7000-8000-000000000a42', 'Medium',    5,  9,  20, now(), now()),
    ('019a0000-0000-7000-8000-000000000a43', 'High',      10, 15, 30, now(), now()),
    ('019a0000-0000-7000-8000-000000000a44', 'Very High', 16, 20, 40, now(), now()),
    ('019a0000-0000-7000-8000-000000000a45', 'Critical',  21, 25, 50, now(), now());

-- Change Templates (FR-CHG-010): instance-wide, versioned, optionally limited to Teams.
CREATE TABLE change_templates (
    id              uuid PRIMARY KEY,
    name            text        NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 150),
    description     text        NOT NULL DEFAULT '',
    team_ids        uuid[]      NOT NULL DEFAULT '{}',
    current_version integer     NOT NULL CHECK (current_version >= 1),
    is_active       boolean     NOT NULL DEFAULT true,
    created_at      timestamptz NOT NULL,
    updated_at      timestamptz NOT NULL
);
CREATE UNIQUE INDEX change_templates_name_key ON change_templates (lower(name));
CREATE TABLE change_template_versions (
    template_id     uuid        NOT NULL REFERENCES change_templates (id),
    version_no      integer     NOT NULL CHECK (version_no >= 1),
    content         jsonb       NOT NULL,
    created_by_kind text        NOT NULL,
    created_by_id   uuid,
    created_at      timestamptz NOT NULL,
    PRIMARY KEY (template_id, version_no)
);

CREATE SEQUENCE change_number_seq;

CREATE TABLE changes (
    id                  uuid PRIMARY KEY,
    tenant_id           uuid        NOT NULL REFERENCES tenants (id),
    change_number       text        NOT NULL UNIQUE,
    title               text        NOT NULL CHECK (length(btrim(title)) BETWEEN 1 AND 255),
    type_id             uuid        NOT NULL REFERENCES change_types (id),
    status_id           uuid        NOT NULL REFERENCES change_statuses (id),
    priority_id         uuid        NOT NULL REFERENCES change_priorities (id),
    impact_id           uuid        NOT NULL REFERENCES change_impacts (id),
    category_id         uuid REFERENCES change_categories (id),
    requested_by_id     uuid REFERENCES analysts (id),
    assigned_to_id      uuid REFERENCES analysts (id),
    approver_id         uuid REFERENCES analysts (id),
    work_start          timestamptz,
    work_end            timestamptz,
    outage_start        timestamptz,
    outage_end          timestamptz,
    description         text        NOT NULL DEFAULT '',
    reason_for_change   text        NOT NULL DEFAULT '',
    risk_evaluation     text        NOT NULL DEFAULT '',
    risk_likelihood     smallint CHECK (risk_likelihood BETWEEN 1 AND 5),
    risk_impact_score   smallint CHECK (risk_impact_score BETWEEN 1 AND 5),
    test_plan           text        NOT NULL DEFAULT '',
    rollback_plan       text        NOT NULL DEFAULT '',
    cab_required        boolean     NOT NULL DEFAULT false,
    cab_approval_type   text        NOT NULL DEFAULT 'all' CHECK (cab_approval_type IN ('all', 'majority')),
    approval_round      integer     NOT NULL DEFAULT 0,
    approved_at         timestamptz,
    template_id         uuid,
    template_version    integer,
    pir_successful      boolean,
    pir_actual_start    timestamptz,
    pir_actual_end      timestamptz,
    pir_lessons_learned text,
    pir_follow_up       text,
    pir_recorded_at     timestamptz,
    created_by_kind     text        NOT NULL,
    created_by_id       uuid,
    created_at          timestamptz NOT NULL,
    updated_at          timestamptz NOT NULL,
    closed_at           timestamptz,
    CONSTRAINT changes_work_window CHECK (work_start IS NULL OR work_end IS NULL OR work_start < work_end),
    CONSTRAINT changes_outage_window CHECK (outage_start IS NULL OR outage_end IS NULL OR outage_start < outage_end),
    CONSTRAINT changes_template FOREIGN KEY (template_id, template_version) REFERENCES change_template_versions (template_id, version_no)
);
COMMENT ON TABLE changes IS 'Changes (FR-CHG-001). Requested By is an Analyst, as in FreeITSM.';
CREATE INDEX changes_tenant_id_idx ON changes (tenant_id);
CREATE INDEX changes_list_idx ON changes (updated_at DESC, id DESC);
CREATE INDEX changes_work_idx ON changes (work_start, work_end);
CREATE INDEX changes_outage_idx ON changes (outage_start, outage_end);

CREATE TABLE change_cab_members (
    tenant_id   uuid    NOT NULL REFERENCES tenants (id),
    change_id   uuid    NOT NULL REFERENCES changes (id),
    analyst_id  uuid    NOT NULL REFERENCES analysts (id),
    is_required boolean NOT NULL DEFAULT true,
    PRIMARY KEY (change_id, analyst_id)
);

-- Votes are append-only (FR-CHG-007): the app role may only insert.
CREATE TABLE change_approval_votes (
    id         uuid PRIMARY KEY,
    tenant_id  uuid        NOT NULL REFERENCES tenants (id),
    change_id  uuid        NOT NULL REFERENCES changes (id),
    round_no   integer     NOT NULL CHECK (round_no >= 1),
    analyst_id uuid        NOT NULL REFERENCES analysts (id),
    decision   text        NOT NULL CHECK (decision IN ('approve', 'reject')),
    comment    text        NOT NULL DEFAULT '',
    voted_at   timestamptz NOT NULL,
    CONSTRAINT change_approval_votes_once UNIQUE (change_id, round_no, analyst_id)
);
CREATE TRIGGER change_approval_votes_append_only BEFORE UPDATE OR DELETE ON change_approval_votes
    FOR EACH ROW EXECUTE FUNCTION cooper_reject_modification();

CREATE TABLE change_checklist_items (
    id         uuid PRIMARY KEY,
    tenant_id  uuid        NOT NULL REFERENCES tenants (id),
    change_id  uuid        NOT NULL REFERENCES changes (id),
    text       text        NOT NULL CHECK (length(btrim(text)) BETWEEN 1 AND 500),
    is_done    boolean     NOT NULL DEFAULT false,
    done_by_id uuid REFERENCES analysts (id),
    done_at    timestamptz,
    sort_order integer     NOT NULL DEFAULT 0,
    created_at timestamptz NOT NULL
);
CREATE TABLE change_comments (
    id         uuid PRIMARY KEY,
    tenant_id  uuid        NOT NULL REFERENCES tenants (id),
    change_id  uuid        NOT NULL REFERENCES changes (id),
    author_id  uuid REFERENCES analysts (id),
    body       text        NOT NULL CHECK (length(btrim(body)) BETWEEN 1 AND 100000),
    created_at timestamptz NOT NULL
);
-- Links to tickets, Problems and other Changes (FR-CHG-008).
CREATE TABLE change_links (
    tenant_id      uuid        NOT NULL REFERENCES tenants (id),
    change_id      uuid        NOT NULL REFERENCES changes (id),
    kind           text        NOT NULL CHECK (kind IN ('ticket', 'problem', 'change')),
    target_id      uuid        NOT NULL,
    linked_by_kind text        NOT NULL,
    linked_by_id   uuid,
    linked_at      timestamptz NOT NULL,
    PRIMARY KEY (change_id, kind, target_id)
);
CREATE INDEX change_links_target_idx ON change_links (kind, target_id);
-- Every saved version of a long-text field (FR-CHG-009).
CREATE TABLE change_text_revisions (
    id          uuid PRIMARY KEY,
    tenant_id   uuid        NOT NULL REFERENCES tenants (id),
    change_id   uuid        NOT NULL REFERENCES changes (id),
    field       text        NOT NULL,
    body        text        NOT NULL,
    author_kind text        NOT NULL,
    author_id   uuid,
    created_at  timestamptz NOT NULL
);
CREATE INDEX change_text_revisions_idx ON change_text_revisions (change_id, field, created_at);

DO $$
DECLARE t text;
BEGIN
    FOREACH t IN ARRAY ARRAY['changes', 'change_cab_members', 'change_approval_votes', 'change_checklist_items',
                             'change_comments', 'change_links', 'change_text_revisions'] LOOP
        EXECUTE format('CREATE INDEX %I ON %I (tenant_id)', t || '_tenant_idx', t);
        EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY', t);
        EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY', t);
        EXECUTE format('CREATE POLICY %I ON %I USING (cooper_tenant_visible(tenant_id)) WITH CHECK (cooper_tenant_visible(tenant_id))',
                       t || '_isolation', t);
    END LOOP;
END $$;

-- In-app Notifications for Changes (FR-CHG-014).
ALTER TABLE notifications DROP CONSTRAINT notifications_kind_check;
ALTER TABLE notifications ADD CONSTRAINT notifications_kind_check
    CHECK (kind IN ('ticket_assigned', 'mentioned', 'sla_warning', 'sla_breached', 'reply_received', 'mailbox_failing',
                    'change_submitted', 'change_approved', 'change_rejected', 'change_scheduled', 'change_completed'));
-- Notifications about a Change point at it (they had only tickets before).
ALTER TABLE notifications ADD COLUMN change_id uuid REFERENCES changes (id);
