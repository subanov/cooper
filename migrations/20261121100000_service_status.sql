-- Service Status (FR-SST-001 to -007, -009; FR-PRT-006; ADR-0030).
--
-- Services, Service Incidents and Planned Maintenance belong to one Company or are shared
-- with every Company (tenant_id IS NULL). Their RLS policy therefore lets NULL rows
-- through, as for Knowledge. An Incident and its Services always share one Company (or
-- are all shared), so an Incident can never show one Company's Services to another.

-- Reference data (instance-wide).
CREATE TABLE impact_levels (
    id                 uuid PRIMARY KEY,
    key                text UNIQUE,
    name               text        NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 50),
    colour             text        CHECK (colour ~ '^#[0-9a-fA-F]{6}$'),
    -- 1 is the worst. Unique, so "the worst Impact Level" has one answer.
    severity_order     integer     NOT NULL UNIQUE CHECK (severity_order BETWEEN 1 AND 1000),
    counts_as_downtime boolean     NOT NULL DEFAULT false,
    -- The all-clear shown when a Service has no unresolved Incident.
    is_default         boolean     NOT NULL DEFAULT false CHECK (NOT (is_default AND counts_as_downtime)),
    is_active          boolean     NOT NULL DEFAULT true CHECK (is_active OR NOT is_default),
    created_at         timestamptz NOT NULL,
    updated_at         timestamptz NOT NULL
);
CREATE UNIQUE INDEX impact_levels_name_key ON impact_levels (lower(name));
CREATE UNIQUE INDEX impact_levels_one_default ON impact_levels (is_default) WHERE is_default;

CREATE TABLE service_incident_statuses (
    id          uuid PRIMARY KEY,
    key         text UNIQUE,
    name        text        NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 50),
    colour      text        CHECK (colour ~ '^#[0-9a-fA-F]{6}$'),
    is_resolved boolean     NOT NULL DEFAULT false,
    is_default  boolean     NOT NULL DEFAULT false CHECK (NOT (is_default AND is_resolved)),
    sort_order  integer     NOT NULL DEFAULT 0,
    is_active   boolean     NOT NULL DEFAULT true CHECK (is_active OR NOT is_default),
    created_at  timestamptz NOT NULL,
    updated_at  timestamptz NOT NULL
);
CREATE UNIQUE INDEX service_incident_statuses_name_key ON service_incident_statuses (lower(name));
CREATE UNIQUE INDEX service_incident_statuses_one_default ON service_incident_statuses (is_default) WHERE is_default;

INSERT INTO impact_levels (id, key, name, colour, severity_order, counts_as_downtime, is_default, created_at, updated_at) VALUES
    ('019a0000-0000-7000-8000-000000000c01', 'major_outage',   'Major Outage',   '#dc2626', 1, true,  false, now(), now()),
    ('019a0000-0000-7000-8000-000000000c02', 'partial_outage', 'Partial Outage', '#ea580c', 2, true,  false, now(), now()),
    ('019a0000-0000-7000-8000-000000000c03', 'degraded',       'Degraded',       '#ca8a04', 3, true,  false, now(), now()),
    ('019a0000-0000-7000-8000-000000000c04', 'maintenance',    'Maintenance',    '#2563eb', 4, false, false, now(), now()),
    ('019a0000-0000-7000-8000-000000000c05', 'operational',    'Operational',    '#16a34a', 5, false, true,  now(), now()),
    ('019a0000-0000-7000-8000-000000000c06', 'no_disruption',  'No Disruption',  '#64748b', 6, false, false, now(), now());

INSERT INTO service_incident_statuses (id, key, name, colour, is_resolved, is_default, sort_order, created_at, updated_at) VALUES
    ('019a0000-0000-7000-8000-000000000c11', 'investigating', 'Investigating', '#dc2626', false, true,  10, now(), now()),
    ('019a0000-0000-7000-8000-000000000c12', 'identified',    'Identified',    '#ea580c', false, false, 20, now(), now()),
    ('019a0000-0000-7000-8000-000000000c13', 'monitoring',    'Monitoring',    '#2563eb', false, false, 30, now(), now()),
    ('019a0000-0000-7000-8000-000000000c14', 'third_party',   '3rd Party',     '#7c3aed', false, false, 40, now(), now()),
    ('019a0000-0000-7000-8000-000000000c15', 'resolved',      'Resolved',      '#16a34a', true,  false, 50, now(), now());

-- Services (FR-SST-001). Deactivated, never deleted.
CREATE TABLE status_services (
    id          uuid PRIMARY KEY,
    tenant_id   uuid REFERENCES tenants (id),
    name        text        NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 100),
    description text        NOT NULL DEFAULT '' CHECK (length(description) <= 2000),
    sort_order  integer     NOT NULL DEFAULT 0,
    is_active   boolean     NOT NULL DEFAULT true,
    created_at  timestamptz NOT NULL,
    updated_at  timestamptz NOT NULL
);
CREATE UNIQUE INDEX status_services_name_key
    ON status_services (coalesce(tenant_id, '00000000-0000-0000-0000-000000000000'::uuid), lower(name));

-- Planned Maintenance (FR-SST-007). Created before status_incidents, which point back.
CREATE TABLE planned_maintenances (
    id              uuid PRIMARY KEY,
    tenant_id       uuid REFERENCES tenants (id),
    title           text        NOT NULL CHECK (length(btrim(title)) BETWEEN 1 AND 255),
    -- Public text announced with the maintenance and posted when it starts.
    description     text        NOT NULL DEFAULT '' CHECK (length(description) <= 20000),
    planned_start   timestamptz NOT NULL,
    planned_end     timestamptz NOT NULL,
    state           text        NOT NULL CHECK (state IN ('scheduled', 'started', 'finished', 'cancelled')),
    -- The Service Incident it became when it started.
    incident_id     uuid,
    created_by_kind text        NOT NULL,
    created_by_id   uuid,
    created_at      timestamptz NOT NULL,
    updated_at      timestamptz NOT NULL,
    CHECK (planned_start < planned_end),
    CHECK ((state IN ('scheduled', 'cancelled')) = (incident_id IS NULL))
);
CREATE INDEX planned_maintenances_due_idx ON planned_maintenances (state, planned_start);

CREATE TABLE planned_maintenance_services (
    tenant_id              uuid REFERENCES tenants (id),
    planned_maintenance_id uuid NOT NULL REFERENCES planned_maintenances (id),
    service_id             uuid NOT NULL REFERENCES status_services (id),
    impact_level_id        uuid NOT NULL REFERENCES impact_levels (id),
    PRIMARY KEY (planned_maintenance_id, service_id)
);

-- Service Incidents (FR-SST-004).
CREATE TABLE status_incidents (
    id                     uuid PRIMARY KEY,
    tenant_id              uuid REFERENCES tenants (id),
    title                  text        NOT NULL CHECK (length(btrim(title)) BETWEEN 1 AND 255),
    status_id              uuid        NOT NULL REFERENCES service_incident_statuses (id),
    -- When the disruption began (backdated to the planned start for maintenance).
    started_at             timestamptz NOT NULL,
    -- Stamped on entering a resolved status, cleared on leaving it.
    resolved_at            timestamptz,
    planned_maintenance_id uuid UNIQUE REFERENCES planned_maintenances (id),
    created_by_kind        text        NOT NULL,
    created_by_id          uuid,
    created_at             timestamptz NOT NULL,
    updated_at             timestamptz NOT NULL,
    CHECK (resolved_at IS NULL OR resolved_at >= started_at)
);
CREATE INDEX status_incidents_started_idx ON status_incidents (started_at DESC, id DESC);
CREATE INDEX status_incidents_open_idx ON status_incidents (resolved_at) WHERE resolved_at IS NULL;

ALTER TABLE planned_maintenances
    ADD CONSTRAINT planned_maintenances_incident_fkey FOREIGN KEY (incident_id) REFERENCES status_incidents (id);

-- The Impact Level on each affected Service now.
CREATE TABLE status_incident_services (
    tenant_id       uuid REFERENCES tenants (id),
    incident_id     uuid NOT NULL REFERENCES status_incidents (id),
    service_id      uuid NOT NULL REFERENCES status_services (id),
    impact_level_id uuid NOT NULL REFERENCES impact_levels (id),
    PRIMARY KEY (incident_id, service_id)
);
CREATE INDEX status_incident_services_service_idx ON status_incident_services (service_id);

-- Incident Updates (FR-SST-005): the timeline, each with a snapshot of the status and
-- the impacts at that moment. Only body and visibility can be corrected afterwards.
CREATE TABLE status_incident_updates (
    id          uuid PRIMARY KEY,
    tenant_id   uuid REFERENCES tenants (id),
    incident_id uuid        NOT NULL REFERENCES status_incidents (id),
    status_id   uuid        NOT NULL REFERENCES service_incident_statuses (id),
    body        text        NOT NULL DEFAULT '' CHECK (length(body) <= 20000),
    is_public   boolean     NOT NULL DEFAULT false,
    author_kind text        NOT NULL,
    author_id   uuid,
    posted_at   timestamptz NOT NULL,
    edited_at   timestamptz
);
CREATE INDEX status_incident_updates_incident_idx ON status_incident_updates (incident_id, posted_at, id);

CREATE TABLE status_incident_update_services (
    tenant_id       uuid REFERENCES tenants (id),
    update_id       uuid NOT NULL REFERENCES status_incident_updates (id),
    service_id      uuid NOT NULL REFERENCES status_services (id),
    impact_level_id uuid NOT NULL REFERENCES impact_levels (id),
    PRIMARY KEY (update_id, service_id)
);

DO $$
DECLARE t text;
BEGIN
    FOREACH t IN ARRAY ARRAY['status_services', 'planned_maintenances', 'planned_maintenance_services', 'status_incidents',
                             'status_incident_services', 'status_incident_updates', 'status_incident_update_services'] LOOP
        EXECUTE format('CREATE INDEX %I ON %I (tenant_id)', t || '_tenant_idx', t);
        EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY', t);
        EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY', t);
        EXECUTE format('CREATE POLICY %I ON %I USING (tenant_id IS NULL OR cooper_tenant_visible(tenant_id))
                        WITH CHECK (tenant_id IS NULL OR cooper_tenant_visible(tenant_id))', t || '_isolation', t);
    END LOOP;
END $$;
