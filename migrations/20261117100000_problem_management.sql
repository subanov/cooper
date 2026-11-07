-- Problem Management (FR-PRB-001 to -004, -006, -007; ADR-0026).

-- Reference data (instance-wide), seeded from FreeITSM 4.9.0 (Vocabulary §3).
CREATE TABLE problem_statuses (
    id         uuid PRIMARY KEY,
    key        text UNIQUE CHECK (key ~ '^[a-z][a-z0-9_]*$'),
    name       text        NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 100),
    is_closed  boolean     NOT NULL DEFAULT false,
    is_default boolean     NOT NULL DEFAULT false,
    sort_order integer     NOT NULL DEFAULT 0,
    is_active  boolean     NOT NULL DEFAULT true,
    created_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL
);
CREATE UNIQUE INDEX problem_statuses_name_key ON problem_statuses (lower(name));
CREATE UNIQUE INDEX problem_statuses_one_default ON problem_statuses (is_default) WHERE is_default;

CREATE TABLE problem_priorities (
    id         uuid PRIMARY KEY,
    key        text UNIQUE CHECK (key ~ '^[a-z][a-z0-9_]*$'),
    name       text        NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 100),
    is_default boolean     NOT NULL DEFAULT false,
    sort_order integer     NOT NULL DEFAULT 0,
    is_active  boolean     NOT NULL DEFAULT true,
    created_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL
);
CREATE UNIQUE INDEX problem_priorities_name_key ON problem_priorities (lower(name));
CREATE UNIQUE INDEX problem_priorities_one_default ON problem_priorities (is_default) WHERE is_default;

INSERT INTO problem_statuses (id, key, name, is_closed, is_default, sort_order, created_at, updated_at) VALUES
    ('019a0000-0000-7000-8000-000000000901', 'new',                   'New',                   false, true,  10, now(), now()),
    ('019a0000-0000-7000-8000-000000000902', 'investigating',         'Investigating',         false, false, 20, now(), now()),
    ('019a0000-0000-7000-8000-000000000903', 'root_cause_identified', 'Root Cause Identified', false, false, 30, now(), now()),
    ('019a0000-0000-7000-8000-000000000904', 'known_error',           'Known Error',           false, false, 40, now(), now()),
    ('019a0000-0000-7000-8000-000000000905', 'resolved',              'Resolved',              true,  false, 50, now(), now()),
    ('019a0000-0000-7000-8000-000000000906', 'closed',                'Closed',                true,  false, 60, now(), now());

INSERT INTO problem_priorities (id, key, name, is_default, sort_order, created_at, updated_at) VALUES
    ('019a0000-0000-7000-8000-000000000911', 'low',      'Low',      false, 10, now(), now()),
    ('019a0000-0000-7000-8000-000000000912', 'medium',   'Medium',   true,  20, now(), now()),
    ('019a0000-0000-7000-8000-000000000913', 'high',     'High',     false, 30, now(), now()),
    ('019a0000-0000-7000-8000-000000000914', 'critical', 'Critical', false, 40, now(), now());

-- Problem Numbers are PRB-00001, PRB-00002, … across the instance, as in FreeITSM.
CREATE SEQUENCE problem_number_seq;

CREATE TABLE problems (
    id                  uuid PRIMARY KEY,
    tenant_id           uuid        NOT NULL REFERENCES tenants (id),
    problem_number      text        NOT NULL UNIQUE,
    title               text        NOT NULL CHECK (length(btrim(title)) BETWEEN 1 AND 255),
    description         text        NOT NULL DEFAULT '',
    status_id           uuid        NOT NULL REFERENCES problem_statuses (id),
    priority_id         uuid        NOT NULL REFERENCES problem_priorities (id),
    assigned_analyst_id uuid REFERENCES analysts (id),
    root_cause          text        NOT NULL DEFAULT '',
    workaround          text        NOT NULL DEFAULT '',
    is_known_error      boolean     NOT NULL DEFAULT false,
    created_by_kind     text        NOT NULL,
    created_by_id       uuid,
    created_at          timestamptz NOT NULL,
    updated_at          timestamptz NOT NULL,
    closed_at           timestamptz,
    search_vector       tsvector GENERATED ALWAYS AS (
        setweight(to_tsvector('simple', cooper_unaccent(problem_number || ' ' || title)), 'A') ||
        setweight(to_tsvector('simple', cooper_unaccent(left(description || ' ' || root_cause || ' ' || workaround, 100000))), 'B')
    ) STORED
);
COMMENT ON TABLE problems IS 'Problems: the root cause behind one or more incidents (FR-PRB-001).';
CREATE INDEX problems_tenant_id_idx ON problems (tenant_id);
CREATE INDEX problems_list_idx ON problems (updated_at DESC, id DESC);
CREATE INDEX problems_search_idx ON problems USING gin (search_vector);
ALTER TABLE problems ENABLE ROW LEVEL SECURITY;
ALTER TABLE problems FORCE ROW LEVEL SECURITY;
CREATE POLICY problems_isolation ON problems
    USING (cooper_tenant_visible(tenant_id)) WITH CHECK (cooper_tenant_visible(tenant_id));

CREATE TABLE problem_notes (
    id         uuid PRIMARY KEY,
    tenant_id  uuid        NOT NULL REFERENCES tenants (id),
    problem_id uuid        NOT NULL REFERENCES problems (id),
    author_id  uuid REFERENCES analysts (id),
    body       text        NOT NULL CHECK (length(btrim(body)) BETWEEN 1 AND 100000),
    created_at timestamptz NOT NULL
);
CREATE INDEX problem_notes_problem_idx ON problem_notes (problem_id, created_at);
CREATE INDEX problem_notes_tenant_id_idx ON problem_notes (tenant_id);
ALTER TABLE problem_notes ENABLE ROW LEVEL SECURITY;
ALTER TABLE problem_notes FORCE ROW LEVEL SECURITY;
CREATE POLICY problem_notes_isolation ON problem_notes
    USING (cooper_tenant_visible(tenant_id)) WITH CHECK (cooper_tenant_visible(tenant_id));

-- Incident tickets linked to a Problem (FR-PRB-003). Same Company only.
CREATE TABLE problem_tickets (
    tenant_id      uuid        NOT NULL REFERENCES tenants (id),
    problem_id     uuid        NOT NULL REFERENCES problems (id),
    ticket_id      uuid        NOT NULL REFERENCES tickets (id),
    linked_by_kind text        NOT NULL,
    linked_by_id   uuid,
    linked_at      timestamptz NOT NULL,
    PRIMARY KEY (problem_id, ticket_id)
);
CREATE INDEX problem_tickets_ticket_idx ON problem_tickets (ticket_id);
CREATE INDEX problem_tickets_tenant_id_idx ON problem_tickets (tenant_id);
ALTER TABLE problem_tickets ENABLE ROW LEVEL SECURITY;
ALTER TABLE problem_tickets FORCE ROW LEVEL SECURITY;
CREATE POLICY problem_tickets_isolation ON problem_tickets
    USING (cooper_tenant_visible(tenant_id)) WITH CHECK (cooper_tenant_visible(tenant_id));
