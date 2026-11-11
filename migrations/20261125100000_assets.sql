-- Assets (FR-AST-001, -002, -004, -005, -006; ADR-0034).

-- Reference data (FR-AST-002): instance-wide like the other contexts' lookups, no seed
-- values (Administrators create them). Rows are deactivated, never deleted, so Assets
-- never lose their type, status or Location.
CREATE TABLE asset_types (
    id         uuid PRIMARY KEY,
    name       text        NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 100),
    sort_order integer     NOT NULL DEFAULT 0,
    is_active  boolean     NOT NULL DEFAULT true,
    created_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL
);
CREATE UNIQUE INDEX asset_types_name_key ON asset_types (lower(name));

CREATE TABLE asset_statuses (
    id         uuid PRIMARY KEY,
    name       text        NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 100),
    -- The Asset has left the estate. Only Retire moves an Asset into such a status.
    is_retired boolean     NOT NULL DEFAULT false,
    colour     text        CHECK (colour ~ '^#[0-9a-fA-F]{6}$'),
    sort_order integer     NOT NULL DEFAULT 0,
    is_active  boolean     NOT NULL DEFAULT true,
    created_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL
);
CREATE UNIQUE INDEX asset_statuses_name_key ON asset_statuses (lower(name));

-- A tree of physical places (site, building, floor, room …). The application keeps it
-- free of loops and at most six levels deep.
CREATE TABLE asset_locations (
    id         uuid PRIMARY KEY,
    parent_id  uuid REFERENCES asset_locations (id),
    name       text        NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 100),
    sort_order integer     NOT NULL DEFAULT 0,
    is_active  boolean     NOT NULL DEFAULT true,
    created_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL,
    CHECK (parent_id IS DISTINCT FROM id)
);
CREATE UNIQUE INDEX asset_locations_sibling_name_key
    ON asset_locations (parent_id, lower(name)) NULLS NOT DISTINCT;

CREATE TABLE assets (
    id                  uuid PRIMARY KEY,
    tenant_id           uuid        NOT NULL REFERENCES tenants (id),
    hostname            text        CHECK (length(btrim(hostname)) BETWEEN 1 AND 255),
    -- The manufacturer serial number.
    service_tag         text        CHECK (length(btrim(service_tag)) BETWEEN 1 AND 100),
    -- The label on the item, unique per Company (see assets_asset_tag_key).
    asset_tag           text        CHECK (length(btrim(asset_tag)) BETWEEN 1 AND 50),
    asset_type_id       uuid REFERENCES asset_types (id),
    asset_status_id     uuid REFERENCES asset_statuses (id),
    location_id         uuid REFERENCES asset_locations (id),
    -- A Supplier of the Contracts context. No foreign key: the Contracts context owns
    -- that table and is checked through its application API (ADR-0034).
    supplier_id         uuid,
    cost_centre_id      uuid REFERENCES cost_centres (id),
    purchase_date       date,
    purchase_cost       numeric(14, 2) CHECK (purchase_cost >= 0),
    purchase_currency   text        CHECK (purchase_currency ~ '^[A-Z]{3}$'),
    warranty_expires_on date,
    lease_starts_on     date,
    lease_expires_on    date,
    -- Retirement (FR-AST-005): the Asset left the estate on this day for this reason.
    -- Retired Assets are excluded from active counts.
    retired_on          date,
    retirement_reason   text        CHECK (length(btrim(retirement_reason)) BETWEEN 1 AND 1000),
    retired_at          timestamptz,
    retired_by_kind     text,
    retired_by_id       uuid,
    created_by_kind     text        NOT NULL,
    created_by_id       uuid,
    created_at          timestamptz NOT NULL,
    updated_at          timestamptz NOT NULL,
    CHECK (hostname IS NOT NULL OR service_tag IS NOT NULL OR asset_tag IS NOT NULL),
    CHECK ((purchase_cost IS NULL) = (purchase_currency IS NULL)),
    CHECK (lease_starts_on IS NULL OR lease_expires_on IS NULL OR lease_starts_on <= lease_expires_on),
    CHECK ((retired_on IS NULL) = (retirement_reason IS NULL)
       AND (retired_on IS NULL) = (retired_at IS NULL)
       AND (retired_on IS NULL) = (retired_by_kind IS NULL))
);
CREATE UNIQUE INDEX assets_asset_tag_key ON assets (tenant_id, lower(asset_tag)) WHERE asset_tag IS NOT NULL;
CREATE INDEX assets_active_idx ON assets (tenant_id, id) WHERE retired_on IS NULL;
CREATE INDEX assets_type_idx ON assets (asset_type_id);
CREATE INDEX assets_status_idx ON assets (asset_status_id);
CREATE INDEX assets_location_idx ON assets (location_id);
CREATE INDEX assets_hostname_idx ON assets (lower(hostname));

-- The Check Out log (FR-AST-004). An open row (checked_in_at IS NULL) is the current
-- Holder; at most one per Asset.
CREATE TABLE asset_checkouts (
    id                  uuid PRIMARY KEY,
    tenant_id           uuid        NOT NULL REFERENCES tenants (id),
    asset_id            uuid        NOT NULL REFERENCES assets (id),
    holder_kind         text        NOT NULL CHECK (holder_kind IN ('requester', 'analyst')),
    holder_id           uuid        NOT NULL,
    checked_out_at      timestamptz NOT NULL,
    checked_out_by_kind text        NOT NULL,
    checked_out_by_id   uuid,
    checkout_note       text        NOT NULL DEFAULT '' CHECK (length(checkout_note) <= 1000),
    due_back_on         date,
    checked_in_at       timestamptz,
    checked_in_by_kind  text,
    checked_in_by_id    uuid,
    checkin_note        text        NOT NULL DEFAULT '' CHECK (length(checkin_note) <= 1000),
    CHECK ((checked_in_at IS NULL) = (checked_in_by_kind IS NULL)),
    CHECK (checked_in_at IS NULL OR checked_in_at >= checked_out_at)
);
CREATE UNIQUE INDEX asset_checkouts_one_open ON asset_checkouts (asset_id) WHERE checked_in_at IS NULL;
CREATE INDEX asset_checkouts_asset_idx ON asset_checkouts (asset_id, checked_out_at);
CREATE INDEX asset_checkouts_holder_idx ON asset_checkouts (holder_kind, holder_id) WHERE checked_in_at IS NULL;

-- Links to tickets and Contracts (FR-AST-006). Links to CIs arrive with the CMDB
-- (FR-CMD-005).
CREATE TABLE asset_links (
    tenant_id      uuid        NOT NULL REFERENCES tenants (id),
    asset_id       uuid        NOT NULL REFERENCES assets (id),
    kind           text        NOT NULL CHECK (kind IN ('ticket', 'contract')),
    target_id      uuid        NOT NULL,
    linked_by_kind text        NOT NULL,
    linked_by_id   uuid,
    linked_at      timestamptz NOT NULL,
    PRIMARY KEY (asset_id, kind, target_id)
);
CREATE INDEX asset_links_target_idx ON asset_links (kind, target_id);

DO $$
DECLARE t text;
BEGIN
    FOREACH t IN ARRAY ARRAY['assets', 'asset_checkouts', 'asset_links'] LOOP
        EXECUTE format('CREATE INDEX %I ON %I (tenant_id)', t || '_tenant_idx', t);
        EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY', t);
        EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY', t);
        EXECUTE format('CREATE POLICY %I ON %I USING (cooper_tenant_visible(tenant_id)) WITH CHECK (cooper_tenant_visible(tenant_id))',
                       t || '_isolation', t);
    END LOOP;
END $$;
