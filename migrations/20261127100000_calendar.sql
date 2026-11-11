-- Calendar (FR-CAL-001; generated contract and asset dates FR-CAL-002, FR-CON-003,
-- FR-AST-012; ADR-0034).

-- Colour-coded Calendar Categories: instance-wide reference data, like other lookups.
-- No seed: Administrators create their own. Cooper creates one system category per
-- source of generated events ('contracts', 'assets') the first time it needs it.
CREATE TABLE calendar_categories (
    id         uuid PRIMARY KEY,
    -- Stable key of a system category; NULL for Administrators' categories.
    key        text UNIQUE,
    name       text        NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 50),
    colour     text        NOT NULL CHECK (colour ~ '^#[0-9a-f]{6}$'),
    -- Inactive categories are not offered for new events; system ones stay active.
    is_active  boolean     NOT NULL DEFAULT true CHECK (is_active OR key IS NULL),
    created_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL
);
CREATE UNIQUE INDEX calendar_categories_name_key ON calendar_categories (lower(name));

-- Calendar Events of a Company. All-day events are dates (no time zone); timed events
-- are UTC instants. Generated events carry their source and are read-only in the API.
CREATE TABLE calendar_events (
    id              uuid PRIMARY KEY,
    tenant_id       uuid        NOT NULL REFERENCES tenants (id),
    title           text        NOT NULL CHECK (length(btrim(title)) BETWEEN 1 AND 255),
    description     text        NOT NULL DEFAULT '' CHECK (length(description) <= 10000),
    category_id     uuid        NOT NULL REFERENCES calendar_categories (id),
    location        text        CHECK (location IS NULL OR length(btrim(location)) BETWEEN 1 AND 255),
    all_day         boolean     NOT NULL,
    start_date      date,
    end_date        date,
    starts_at       timestamptz,
    ends_at         timestamptz,
    -- The record a generated event comes from, and which of its dates it marks.
    source_kind     text        CHECK (source_kind IN ('contract', 'asset')),
    source_id       uuid,
    source_date     text        CHECK (source_date IN ('notice_date', 'end_date', 'warranty_expires_on', 'lease_expires_on')),
    created_by_kind text        NOT NULL,
    created_by_id   uuid,
    created_at      timestamptz NOT NULL,
    updated_at      timestamptz NOT NULL,
    CHECK (CASE WHEN all_day
                THEN start_date IS NOT NULL AND end_date IS NOT NULL AND end_date >= start_date
                     AND starts_at IS NULL AND ends_at IS NULL
                ELSE starts_at IS NOT NULL AND ends_at IS NOT NULL AND ends_at >= starts_at
                     AND start_date IS NULL AND end_date IS NULL
           END),
    CHECK ((source_kind IS NULL) = (source_id IS NULL) AND (source_kind IS NULL) = (source_date IS NULL)),
    -- Generated events are whole days.
    CHECK (source_kind IS NULL OR all_day)
);
CREATE UNIQUE INDEX calendar_events_source_key
    ON calendar_events (tenant_id, source_kind, source_id, source_date) WHERE source_kind IS NOT NULL;
CREATE INDEX calendar_events_days_idx ON calendar_events (tenant_id, start_date, end_date) WHERE all_day;
CREATE INDEX calendar_events_times_idx ON calendar_events (tenant_id, starts_at, ends_at) WHERE NOT all_day;
CREATE INDEX calendar_events_category_idx ON calendar_events (category_id);

-- The last outbox event applied to each source's generated events, so a late
-- redelivery of an older event cannot undo a newer one (even after its events were
-- removed).
CREATE TABLE calendar_sources (
    tenant_id       uuid        NOT NULL REFERENCES tenants (id),
    source_kind     text        NOT NULL CHECK (source_kind IN ('contract', 'asset')),
    source_id       uuid        NOT NULL,
    synced_event_id uuid        NOT NULL,
    synced_at       timestamptz NOT NULL,
    PRIMARY KEY (tenant_id, source_kind, source_id)
);

DO $$
DECLARE t text;
BEGIN
    FOREACH t IN ARRAY ARRAY['calendar_events', 'calendar_sources'] LOOP
        EXECUTE format('CREATE INDEX %I ON %I (tenant_id)', t || '_tenant_idx', t);
        EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY', t);
        EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY', t);
        EXECUTE format('CREATE POLICY %I ON %I USING (cooper_tenant_visible(tenant_id)) WITH CHECK (cooper_tenant_visible(tenant_id))',
                       t || '_isolation', t);
    END LOOP;
END $$;
