-- Scheduled work on tickets for an Owner (FR-TKT-031) and the rota (FR-SLA-008; ADR-0042).

CREATE TABLE ticket_scheduled_work (
    id          uuid PRIMARY KEY,
    tenant_id   uuid        NOT NULL REFERENCES tenants (id),
    ticket_id   uuid        NOT NULL REFERENCES tickets (id),
    owner_id    uuid        NOT NULL REFERENCES analysts (id),
    all_day     boolean     NOT NULL,
    starts_on   date        NOT NULL,
    ends_on     date        NOT NULL,
    starts_at   timestamptz,
    ends_at     timestamptz,
    notes       text        NOT NULL DEFAULT '' CHECK (length(notes) <= 2000),
    is_active   boolean     NOT NULL DEFAULT true,
    created_by_kind text    NOT NULL,
    created_by_id   uuid,
    created_at  timestamptz NOT NULL,
    updated_at  timestamptz NOT NULL,
    CHECK (ends_on >= starts_on),
    CHECK ((all_day AND starts_at IS NULL AND ends_at IS NULL)
        OR (NOT all_day AND starts_at IS NOT NULL AND ends_at IS NOT NULL AND ends_at > starts_at))
);
CREATE INDEX ticket_scheduled_work_owner_idx ON ticket_scheduled_work (tenant_id, owner_id, starts_on);
CREATE INDEX ticket_scheduled_work_ticket_idx ON ticket_scheduled_work (ticket_id);

-- Rota Locations and Shifts are Company reference data (Office, WFH, ...).
CREATE TABLE rota_locations (
    id         uuid PRIMARY KEY,
    tenant_id  uuid        NOT NULL REFERENCES tenants (id),
    name       text        NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 100),
    is_active  boolean     NOT NULL DEFAULT true,
    created_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL
);
CREATE UNIQUE INDEX rota_locations_name_key ON rota_locations (tenant_id, lower(name));

CREATE TABLE rota_shifts (
    id         uuid PRIMARY KEY,
    tenant_id  uuid        NOT NULL REFERENCES tenants (id),
    name       text        NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 100),
    starts_at  time        NOT NULL,
    ends_at    time        NOT NULL,
    is_active  boolean     NOT NULL DEFAULT true,
    created_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL,
    CHECK (starts_at <> ends_at)
);
CREATE UNIQUE INDEX rota_shifts_name_key ON rota_shifts (tenant_id, lower(name));

-- Analysts scheduled on a day, in a shift and a location; on call when flagged.
CREATE TABLE rota_entries (
    id          uuid PRIMARY KEY,
    tenant_id   uuid    NOT NULL REFERENCES tenants (id),
    analyst_id  uuid    NOT NULL REFERENCES analysts (id),
    work_date   date    NOT NULL,
    shift_id    uuid    NOT NULL REFERENCES rota_shifts (id),
    location_id uuid    NOT NULL REFERENCES rota_locations (id),
    on_call     boolean NOT NULL DEFAULT false,
    UNIQUE (analyst_id, work_date, shift_id)
);
CREATE INDEX rota_entries_day_idx ON rota_entries (tenant_id, work_date);

DO $$
DECLARE t text;
BEGIN
    FOREACH t IN ARRAY ARRAY['ticket_scheduled_work', 'rota_locations', 'rota_shifts', 'rota_entries'] LOOP
        EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY', t);
        EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY', t);
        EXECUTE format('CREATE POLICY %I ON %I USING (cooper_tenant_visible(tenant_id)) WITH CHECK (cooper_tenant_visible(tenant_id))',
                       t || '_isolation', t);
    END LOOP;
END $$;
