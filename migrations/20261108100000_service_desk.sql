-- Service desk core (ADR-0014): ticket reference data, Categories, Tickets, Notes,
-- Time Entries and Ticket Number counters.

CREATE EXTENSION IF NOT EXISTS unaccent;

-- unaccent() is STABLE; this wrapper with a fixed dictionary is IMMUTABLE so it can be
-- used in generated search columns (ADR-0006).
CREATE FUNCTION cooper_unaccent(text) RETURNS text
LANGUAGE sql IMMUTABLE PARALLEL SAFE STRICT AS
$$ SELECT public.unaccent('public.unaccent'::regdictionary, $1) $$;

-- ----- Reference data (instance-wide; Administrators configure it) -------------------

CREATE TABLE ticket_statuses (
    id         uuid PRIMARY KEY,
    key        text UNIQUE CHECK (key ~ '^[a-z][a-z0-9_]*$'),
    name       text        NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 100),
    is_closed  boolean     NOT NULL DEFAULT false,
    pauses_sla boolean     NOT NULL DEFAULT false,
    is_default boolean     NOT NULL DEFAULT false,
    sort_order integer     NOT NULL DEFAULT 0,
    is_active  boolean     NOT NULL DEFAULT true,
    created_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL,
    CONSTRAINT ticket_statuses_default_is_open CHECK (NOT (is_default AND is_closed))
);
CREATE UNIQUE INDEX ticket_statuses_name_key ON ticket_statuses (lower(name));
CREATE UNIQUE INDEX ticket_statuses_single_default ON ticket_statuses (is_default) WHERE is_default;

CREATE TABLE ticket_priorities (
    id                 uuid PRIMARY KEY,
    key                text UNIQUE CHECK (key ~ '^[a-z][a-z0-9_]*$'),
    name               text        NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 100),
    -- SLA targets in working minutes (FR-TKT-005); NULL = no target.
    response_minutes   integer CHECK (response_minutes > 0),
    resolution_minutes integer CHECK (resolution_minutes > 0),
    -- SLA Calendar the targets count against (added by the SLA work).
    sla_calendar_id    uuid,
    is_default         boolean     NOT NULL DEFAULT false,
    sort_order         integer     NOT NULL DEFAULT 0,
    is_active          boolean     NOT NULL DEFAULT true,
    created_at         timestamptz NOT NULL,
    updated_at         timestamptz NOT NULL
);
CREATE UNIQUE INDEX ticket_priorities_name_key ON ticket_priorities (lower(name));
CREATE UNIQUE INDEX ticket_priorities_single_default ON ticket_priorities (is_default) WHERE is_default;

CREATE TABLE ticket_types (
    id            uuid PRIMARY KEY,
    key           text UNIQUE CHECK (key ~ '^[a-z][a-z0-9_]*$'),
    name          text        NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 100),
    -- The {TYPE} token in Ticket Numbers.
    number_prefix text CHECK (number_prefix ~ '^[A-Z0-9]{1,10}$'),
    sort_order    integer     NOT NULL DEFAULT 0,
    is_active     boolean     NOT NULL DEFAULT true,
    created_at    timestamptz NOT NULL,
    updated_at    timestamptz NOT NULL
);
CREATE UNIQUE INDEX ticket_types_name_key ON ticket_types (lower(name));

CREATE TABLE ticket_origins (
    id         uuid PRIMARY KEY,
    key        text UNIQUE CHECK (key ~ '^[a-z][a-z0-9_]*$'),
    name       text        NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 100),
    sort_order integer     NOT NULL DEFAULT 0,
    is_active  boolean     NOT NULL DEFAULT true,
    created_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL
);
CREATE UNIQUE INDEX ticket_origins_name_key ON ticket_origins (lower(name));

CREATE TABLE resolution_codes (
    id         uuid PRIMARY KEY,
    key        text UNIQUE CHECK (key ~ '^[a-z][a-z0-9_]*$'),
    name       text        NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 100),
    sort_order integer     NOT NULL DEFAULT 0,
    is_active  boolean     NOT NULL DEFAULT true,
    created_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL
);
CREATE UNIQUE INDEX resolution_codes_name_key ON resolution_codes (lower(name));

-- Seeds (Vocabulary; FreeITSM 4.9.0). Keys are stable: code uses keys, never names.
INSERT INTO ticket_statuses (id, key, name, is_closed, pauses_sla, is_default, sort_order, created_at, updated_at) VALUES
    ('019a0000-0000-7000-8000-000000000101', 'open',              'Open',              false, false, true,  10, now(), now()),
    ('019a0000-0000-7000-8000-000000000102', 'in_progress',       'In Progress',       false, false, false, 20, now(), now()),
    ('019a0000-0000-7000-8000-000000000103', 'on_hold',           'On Hold',           false, true,  false, 30, now(), now()),
    ('019a0000-0000-7000-8000-000000000104', 'awaiting_response', 'Awaiting Response', false, true,  false, 40, now(), now()),
    ('019a0000-0000-7000-8000-000000000105', 'closed',            'Closed',            true,  false, false, 50, now(), now());

INSERT INTO ticket_priorities (id, key, name, is_default, sort_order, created_at, updated_at) VALUES
    ('019a0000-0000-7000-8000-000000000201', 'low',      'Low',      false, 10, now(), now()),
    ('019a0000-0000-7000-8000-000000000202', 'normal',   'Normal',   true,  20, now(), now()),
    ('019a0000-0000-7000-8000-000000000203', 'high',     'High',     false, 30, now(), now()),
    ('019a0000-0000-7000-8000-000000000204', 'critical', 'Critical', false, 40, now(), now()),
    ('019a0000-0000-7000-8000-000000000205', 'urgent',   'Urgent',   false, 50, now(), now());

INSERT INTO ticket_types (id, key, name, sort_order, created_at, updated_at) VALUES
    ('019a0000-0000-7000-8000-000000000301', 'incident',        'Incident',        10, now(), now()),
    ('019a0000-0000-7000-8000-000000000302', 'service_request', 'Service Request', 20, now(), now()),
    ('019a0000-0000-7000-8000-000000000303', 'question',        'Question',        30, now(), now());

INSERT INTO ticket_origins (id, key, name, sort_order, created_at, updated_at) VALUES
    ('019a0000-0000-7000-8000-000000000401', 'email',   'Email',   10, now(), now()),
    ('019a0000-0000-7000-8000-000000000402', 'phone',   'Phone',   20, now(), now()),
    ('019a0000-0000-7000-8000-000000000403', 'portal',  'Portal',  30, now(), now()),
    ('019a0000-0000-7000-8000-000000000404', 'walk_up', 'Walk-up', 40, now(), now());

INSERT INTO resolution_codes (id, key, name, sort_order, created_at, updated_at) VALUES
    ('019a0000-0000-7000-8000-000000000501', 'fixed_remotely',        'Fixed remotely',        10,  now(), now()),
    ('019a0000-0000-7000-8000-000000000502', 'fixed_on_site',         'Fixed on site',         20,  now(), now()),
    ('019a0000-0000-7000-8000-000000000503', 'hardware_replaced',     'Hardware replaced',     30,  now(), now()),
    ('019a0000-0000-7000-8000-000000000504', 'configuration_change',  'Configuration change',  40,  now(), now()),
    ('019a0000-0000-7000-8000-000000000505', 'training_given',        'Training given',        50,  now(), now()),
    ('019a0000-0000-7000-8000-000000000506', 'access_granted',        'Access granted',        60,  now(), now()),
    ('019a0000-0000-7000-8000-000000000507', 'no_fault_found',        'No fault found',        70,  now(), now()),
    ('019a0000-0000-7000-8000-000000000508', 'duplicate',             'Duplicate',             80,  now(), now()),
    ('019a0000-0000-7000-8000-000000000509', 'withdrawn',             'Withdrawn',             90,  now(), now()),
    ('019a0000-0000-7000-8000-000000000510', 'referred_to_supplier',  'Referred to supplier',  100, now(), now());

-- Categories: a tree of at most 3 levels (FR-TKT-007).
CREATE TABLE ticket_categories (
    id             uuid PRIMARY KEY,
    parent_id      uuid REFERENCES ticket_categories (id),
    name           text        NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 100),
    depth          smallint    NOT NULL CHECK (depth BETWEEN 1 AND 3),
    portal_visible boolean     NOT NULL DEFAULT false,
    sort_order     integer     NOT NULL DEFAULT 0,
    is_active      boolean     NOT NULL DEFAULT true,
    created_at     timestamptz NOT NULL,
    updated_at     timestamptz NOT NULL,
    CONSTRAINT ticket_categories_root_depth CHECK ((parent_id IS NULL) = (depth = 1))
);
CREATE UNIQUE INDEX ticket_categories_sibling_name_key
    ON ticket_categories (parent_id, lower(name)) NULLS NOT DISTINCT;

-- A category limited to Ticket Types; no rows = every type.
CREATE TABLE ticket_category_types (
    category_id    uuid NOT NULL REFERENCES ticket_categories (id) ON DELETE CASCADE,
    ticket_type_id uuid NOT NULL REFERENCES ticket_types (id) ON DELETE CASCADE,
    PRIMARY KEY (category_id, ticket_type_id)
);

-- ----- Tickets (tenant-scoped) --------------------------------------------------------

-- One Ticket Number sequence per Company. The row lock serialises concurrent creates,
-- so numbers never repeat.
CREATE TABLE ticket_number_counters (
    tenant_id  uuid PRIMARY KEY REFERENCES tenants (id),
    next_value bigint NOT NULL CHECK (next_value > 0)
);
ALTER TABLE ticket_number_counters ENABLE ROW LEVEL SECURITY;
ALTER TABLE ticket_number_counters FORCE ROW LEVEL SECURITY;
CREATE POLICY ticket_number_counters_isolation ON ticket_number_counters
    USING (cooper_tenant_visible(tenant_id)) WITH CHECK (cooper_tenant_visible(tenant_id));

CREATE TABLE tickets (
    id                  uuid PRIMARY KEY,
    tenant_id           uuid        NOT NULL REFERENCES tenants (id),
    ticket_number       text        NOT NULL,
    subject             text        NOT NULL CHECK (length(btrim(subject)) BETWEEN 1 AND 500),
    description         text        NOT NULL DEFAULT '',
    requester_id        uuid REFERENCES requesters (id),
    department_id       uuid REFERENCES departments (id),
    type_id             uuid REFERENCES ticket_types (id),
    category_id         uuid REFERENCES ticket_categories (id),
    priority_id         uuid        NOT NULL REFERENCES ticket_priorities (id),
    status_id           uuid        NOT NULL REFERENCES ticket_statuses (id),
    origin_id           uuid REFERENCES ticket_origins (id),
    assigned_team_id    uuid REFERENCES teams (id),
    assigned_analyst_id uuid REFERENCES analysts (id),
    sensitivity         text        NOT NULL DEFAULT 'normal' CHECK (sensitivity IN ('normal', 'confidential')),
    first_time_fix      boolean     NOT NULL DEFAULT false,
    training_provided   boolean     NOT NULL DEFAULT false,
    resolution_code_id  uuid REFERENCES resolution_codes (id),
    closure_category_id uuid REFERENCES ticket_categories (id),
    closed_at           timestamptz,
    snoozed_until       timestamptz,
    snooze_reason       text,
    deleted_at          timestamptz,
    created_by_kind     text        NOT NULL CHECK (created_by_kind IN ('analyst', 'requester', 'api_key', 'automation')),
    created_by_id       uuid,
    created_at          timestamptz NOT NULL,
    updated_at          timestamptz NOT NULL,
    search_vector       tsvector GENERATED ALWAYS AS (
        setweight(to_tsvector('simple', cooper_unaccent(ticket_number || ' ' || subject)), 'A') ||
        setweight(to_tsvector('simple', cooper_unaccent(left(description, 100000))), 'B')
    ) STORED,
    CONSTRAINT tickets_resolution_when_closed CHECK (closed_at IS NULL OR resolution_code_id IS NOT NULL)
);
COMMENT ON TABLE tickets IS 'Tickets (FR-TKT-001). Soft-deleted rows have deleted_at (FR-GEN-004).';
CREATE UNIQUE INDEX tickets_ticket_number_key ON tickets (ticket_number);
CREATE INDEX tickets_ticket_number_prefix_idx ON tickets (ticket_number text_pattern_ops);
CREATE INDEX tickets_tenant_id_idx ON tickets (tenant_id);
CREATE INDEX tickets_list_idx ON tickets (tenant_id, created_at DESC, id DESC) WHERE deleted_at IS NULL;
CREATE INDEX tickets_updated_idx ON tickets (updated_at DESC, id DESC) WHERE deleted_at IS NULL;
CREATE INDEX tickets_assigned_analyst_idx ON tickets (assigned_analyst_id) WHERE deleted_at IS NULL AND closed_at IS NULL;
CREATE INDEX tickets_assigned_team_idx ON tickets (assigned_team_id) WHERE deleted_at IS NULL AND closed_at IS NULL;
CREATE INDEX tickets_requester_idx ON tickets (requester_id);
CREATE INDEX tickets_status_idx ON tickets (status_id);
CREATE INDEX tickets_search_idx ON tickets USING gin (search_vector);
ALTER TABLE tickets ENABLE ROW LEVEL SECURITY;
ALTER TABLE tickets FORCE ROW LEVEL SECURITY;
CREATE POLICY tickets_isolation ON tickets
    USING (cooper_tenant_visible(tenant_id)) WITH CHECK (cooper_tenant_visible(tenant_id));

-- Notes (FR-TKT-020): internal by default.
CREATE TABLE ticket_notes (
    id            uuid PRIMARY KEY,
    tenant_id     uuid        NOT NULL REFERENCES tenants (id),
    ticket_id     uuid        NOT NULL REFERENCES tickets (id),
    author_id     uuid REFERENCES analysts (id),
    body          text        NOT NULL CHECK (length(btrim(body)) BETWEEN 1 AND 100000),
    is_public     boolean     NOT NULL DEFAULT false,
    created_at    timestamptz NOT NULL,
    search_vector tsvector GENERATED ALWAYS AS (to_tsvector('simple', cooper_unaccent(body))) STORED
);
CREATE INDEX ticket_notes_ticket_idx ON ticket_notes (ticket_id, created_at);
CREATE INDEX ticket_notes_tenant_id_idx ON ticket_notes (tenant_id);
CREATE INDEX ticket_notes_search_idx ON ticket_notes USING gin (search_vector);
ALTER TABLE ticket_notes ENABLE ROW LEVEL SECURITY;
ALTER TABLE ticket_notes FORCE ROW LEVEL SECURITY;
CREATE POLICY ticket_notes_isolation ON ticket_notes
    USING (cooper_tenant_visible(tenant_id)) WITH CHECK (cooper_tenant_visible(tenant_id));

-- Time Entries (FR-TKT-023).
CREATE TABLE ticket_time_entries (
    id         uuid PRIMARY KEY,
    tenant_id  uuid        NOT NULL REFERENCES tenants (id),
    ticket_id  uuid        NOT NULL REFERENCES tickets (id),
    analyst_id uuid        NOT NULL REFERENCES analysts (id),
    minutes    integer     NOT NULL CHECK (minutes BETWEEN 1 AND 1440),
    work_date  date        NOT NULL,
    note       text        NOT NULL DEFAULT '',
    created_at timestamptz NOT NULL
);
CREATE INDEX ticket_time_entries_ticket_idx ON ticket_time_entries (ticket_id, work_date);
CREATE INDEX ticket_time_entries_tenant_id_idx ON ticket_time_entries (tenant_id);
ALTER TABLE ticket_time_entries ENABLE ROW LEVEL SECURITY;
ALTER TABLE ticket_time_entries FORCE ROW LEVEL SECURITY;
CREATE POLICY ticket_time_entries_isolation ON ticket_time_entries
    USING (cooper_tenant_visible(tenant_id)) WITH CHECK (cooper_tenant_visible(tenant_id));
