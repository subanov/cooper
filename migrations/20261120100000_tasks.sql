-- Tasks (FR-TSK-001 to -006; ADR-0029).

CREATE TABLE task_statuses (
    id         uuid PRIMARY KEY,
    key        text UNIQUE,
    name       text        NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 50),
    is_closed  boolean     NOT NULL DEFAULT false,
    -- A closed status that completes the work (Done), as opposed to abandoning it
    -- (Cancelled). Completing a Task is what repeats it on completion.
    is_done    boolean     NOT NULL DEFAULT false CHECK (NOT is_done OR is_closed),
    colour     text        CHECK (colour ~ '^#[0-9a-fA-F]{6}$'),
    is_default boolean     NOT NULL DEFAULT false CHECK (NOT (is_default AND is_closed)),
    sort_order integer     NOT NULL DEFAULT 0,
    is_active  boolean     NOT NULL DEFAULT true,
    created_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL
);
CREATE UNIQUE INDEX task_statuses_name_key ON task_statuses (lower(name));
CREATE UNIQUE INDEX task_statuses_one_default ON task_statuses (is_default) WHERE is_default;

CREATE TABLE task_priorities (
    id         uuid PRIMARY KEY,
    key        text UNIQUE,
    name       text        NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 50),
    colour     text        CHECK (colour ~ '^#[0-9a-fA-F]{6}$'),
    is_default boolean     NOT NULL DEFAULT false,
    sort_order integer     NOT NULL DEFAULT 0,
    is_active  boolean     NOT NULL DEFAULT true,
    created_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL
);
CREATE UNIQUE INDEX task_priorities_name_key ON task_priorities (lower(name));
CREATE UNIQUE INDEX task_priorities_one_default ON task_priorities (is_default) WHERE is_default;

INSERT INTO task_statuses (id, key, name, is_closed, is_done, colour, is_default, sort_order, created_at, updated_at) VALUES
    ('019a0000-0000-7000-8000-000000000b01', 'to_do',       'To Do',       false, false, '#64748b', true,  10, now(), now()),
    ('019a0000-0000-7000-8000-000000000b02', 'in_progress', 'In Progress', false, false, '#2563eb', false, 20, now(), now()),
    ('019a0000-0000-7000-8000-000000000b03', 'blocked',     'Blocked',     false, false, '#dc2626', false, 30, now(), now()),
    ('019a0000-0000-7000-8000-000000000b04', 'done',        'Done',        true,  true,  '#16a34a', false, 40, now(), now()),
    ('019a0000-0000-7000-8000-000000000b05', 'cancelled',   'Cancelled',   true,  false, '#94a3b8', false, 50, now(), now());
INSERT INTO task_priorities (id, key, name, colour, is_default, sort_order, created_at, updated_at) VALUES
    ('019a0000-0000-7000-8000-000000000b11', 'low',    'Low',    '#64748b', false, 10, now(), now()),
    ('019a0000-0000-7000-8000-000000000b12', 'medium', 'Medium', '#2563eb', true,  20, now(), now()),
    ('019a0000-0000-7000-8000-000000000b13', 'high',   'High',   '#ea580c', false, 30, now(), now()),
    ('019a0000-0000-7000-8000-000000000b14', 'urgent', 'Urgent', '#dc2626', false, 40, now(), now());

-- Repeat rules (FR-TSK-006). A series, not a template: every occurrence is a real Task
-- carrying recurrence_id.
CREATE TABLE task_recurrences (
    id                  uuid PRIMARY KEY,
    tenant_id           uuid        NOT NULL REFERENCES tenants (id),
    -- The rule as JSON, parsed by the domain (frequency, interval, weekdays, monthly and
    -- yearly variants, repeat mode, end condition).
    rule                jsonb       NOT NULL,
    occurrences_created integer     NOT NULL DEFAULT 1 CHECK (occurrences_created >= 1),
    -- Fixed-schedule series only: the date of the next occurrence to create.
    next_date           date,
    is_active           boolean     NOT NULL DEFAULT true,
    created_at          timestamptz NOT NULL,
    updated_at          timestamptz NOT NULL
);

CREATE TABLE tasks (
    id                  uuid PRIMARY KEY,
    tenant_id           uuid        NOT NULL REFERENCES tenants (id),
    title               text        NOT NULL CHECK (length(btrim(title)) BETWEEN 1 AND 255),
    description         text        NOT NULL DEFAULT '',
    status_id           uuid        NOT NULL REFERENCES task_statuses (id),
    priority_id         uuid        NOT NULL REFERENCES task_priorities (id),
    start_date          date,
    due_date            date,
    estimate_minutes    integer     CHECK (estimate_minutes BETWEEN 0 AND 1000000),
    assigned_analyst_id uuid REFERENCES analysts (id),
    assigned_team_id    uuid REFERENCES teams (id),
    parent_task_id      uuid REFERENCES tasks (id),
    tags                text[]      NOT NULL DEFAULT '{}',
    board_position      bigint      NOT NULL DEFAULT 0,
    recurrence_id       uuid REFERENCES task_recurrences (id),
    -- The occurrence this one followed, when it was made by a Recurrence.
    previous_occurrence_id uuid REFERENCES tasks (id),
    created_by_kind     text        NOT NULL,
    created_by_id       uuid,
    created_at          timestamptz NOT NULL,
    updated_at          timestamptz NOT NULL,
    completed_at        timestamptz,
    CHECK (start_date IS NULL OR due_date IS NULL OR start_date <= due_date),
    CHECK (parent_task_id IS DISTINCT FROM id)
);
CREATE INDEX tasks_status_board_idx ON tasks (status_id, board_position);
CREATE INDEX tasks_parent_idx ON tasks (parent_task_id);
CREATE INDEX tasks_analyst_idx ON tasks (assigned_analyst_id);
CREATE INDEX tasks_recurrence_idx ON tasks (recurrence_id);
CREATE INDEX tasks_tags_idx ON tasks USING gin (tags);

CREATE TABLE task_collaborators (
    tenant_id  uuid NOT NULL REFERENCES tenants (id),
    task_id    uuid NOT NULL REFERENCES tasks (id),
    analyst_id uuid NOT NULL REFERENCES analysts (id),
    PRIMARY KEY (task_id, analyst_id)
);

CREATE TABLE task_comments (
    id         uuid PRIMARY KEY,
    tenant_id  uuid        NOT NULL REFERENCES tenants (id),
    task_id    uuid        NOT NULL REFERENCES tasks (id),
    author_id  uuid REFERENCES analysts (id),
    body       text        NOT NULL CHECK (length(btrim(body)) BETWEEN 1 AND 100000),
    created_at timestamptz NOT NULL
);
CREATE INDEX task_comments_task_idx ON task_comments (task_id, created_at);

CREATE TABLE task_time_entries (
    id         uuid PRIMARY KEY,
    tenant_id  uuid        NOT NULL REFERENCES tenants (id),
    task_id    uuid        NOT NULL REFERENCES tasks (id),
    analyst_id uuid        NOT NULL REFERENCES analysts (id),
    work_date  date        NOT NULL,
    minutes    integer     NOT NULL CHECK (minutes BETWEEN 1 AND 1440),
    note       text        NOT NULL DEFAULT '',
    created_at timestamptz NOT NULL
);
CREATE INDEX task_time_entries_task_idx ON task_time_entries (task_id, work_date);

-- Links to tickets and Changes (FR-TSK-004). Contracts and Projects follow their
-- contexts.
CREATE TABLE task_links (
    tenant_id      uuid        NOT NULL REFERENCES tenants (id),
    task_id        uuid        NOT NULL REFERENCES tasks (id),
    kind           text        NOT NULL CHECK (kind IN ('ticket', 'change')),
    target_id      uuid        NOT NULL,
    linked_by_kind text        NOT NULL,
    linked_by_id   uuid,
    linked_at      timestamptz NOT NULL,
    PRIMARY KEY (task_id, kind, target_id)
);
CREATE INDEX task_links_target_idx ON task_links (kind, target_id);

DO $$
DECLARE t text;
BEGIN
    FOREACH t IN ARRAY ARRAY['task_recurrences', 'tasks', 'task_collaborators', 'task_comments', 'task_time_entries',
                             'task_links'] LOOP
        EXECUTE format('CREATE INDEX %I ON %I (tenant_id)', t || '_tenant_idx', t);
        EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY', t);
        EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY', t);
        EXECUTE format('CREATE POLICY %I ON %I USING (cooper_tenant_visible(tenant_id)) WITH CHECK (cooper_tenant_visible(tenant_id))',
                       t || '_isolation', t);
    END LOOP;
END $$;

-- Assignment Notifications (FR-TSK-001).
ALTER TABLE notifications DROP CONSTRAINT notifications_kind_check;
ALTER TABLE notifications ADD CONSTRAINT notifications_kind_check
    CHECK (kind IN ('ticket_assigned', 'mentioned', 'sla_warning', 'sla_breached', 'reply_received', 'mailbox_failing',
                    'change_submitted', 'change_approved', 'change_rejected', 'change_scheduled', 'change_completed',
                    'article_review_due', 'task_assigned'));
ALTER TABLE notifications ADD COLUMN task_id uuid REFERENCES tasks (id);
