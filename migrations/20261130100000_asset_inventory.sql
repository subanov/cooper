-- Asset custom fields, software inventory, licences and CSV import support (FR-AST-003,
-- FR-AST-008, FR-AST-013, FR-AST-014; ADR-0041).

-- Field Sets are instance-wide reference data (like Asset Types). A Field Set applies to
-- the Asset Types it is assigned to.
CREATE TABLE asset_field_sets (
    id         uuid PRIMARY KEY,
    name       text        NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 100),
    sort_order integer     NOT NULL DEFAULT 0,
    is_active  boolean     NOT NULL DEFAULT true,
    created_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL
);
CREATE UNIQUE INDEX asset_field_sets_name_key ON asset_field_sets (lower(name));

CREATE TABLE asset_field_set_types (
    field_set_id  uuid NOT NULL REFERENCES asset_field_sets (id),
    asset_type_id uuid NOT NULL REFERENCES asset_types (id),
    PRIMARY KEY (field_set_id, asset_type_id)
);

-- A custom field. Keys are unique across all Field Sets, so one key means one field.
CREATE TABLE asset_field_definitions (
    id            uuid PRIMARY KEY,
    field_set_id  uuid    NOT NULL REFERENCES asset_field_sets (id),
    field_key     text    NOT NULL CHECK (field_key ~ '^[a-z][a-z0-9_]{0,49}$'),
    label         text    NOT NULL CHECK (length(btrim(label)) BETWEEN 1 AND 255),
    kind          text    NOT NULL CHECK (kind IN ('text', 'number', 'date', 'boolean', 'choice')),
    options       text[]  NOT NULL DEFAULT '{}',
    is_required   boolean NOT NULL DEFAULT false,
    is_unique     boolean NOT NULL DEFAULT false,
    is_searchable boolean NOT NULL DEFAULT false,
    show_in_list  boolean NOT NULL DEFAULT false,
    sort_order    integer NOT NULL DEFAULT 0,
    is_active     boolean NOT NULL DEFAULT true,
    CHECK ((kind = 'choice') = (cardinality(options) > 0))
);
CREATE UNIQUE INDEX asset_field_definitions_key_key ON asset_field_definitions (field_key);
CREATE INDEX asset_field_definitions_set_idx ON asset_field_definitions (field_set_id);

-- The value of a custom field on an Asset (Company-scoped).
CREATE TABLE asset_custom_values (
    asset_id  uuid NOT NULL REFERENCES assets (id),
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    field_id  uuid NOT NULL REFERENCES asset_field_definitions (id),
    value     text NOT NULL CHECK (length(value) <= 2000),
    PRIMARY KEY (asset_id, field_id)
);
CREATE INDEX asset_custom_values_tenant_idx ON asset_custom_values (tenant_id);
CREATE INDEX asset_custom_values_lookup_idx ON asset_custom_values (field_id, tenant_id, lower(value));

-- Applications are distinct per Company by name and publisher (FR-AST-013).
CREATE TABLE software_applications (
    id         uuid PRIMARY KEY,
    tenant_id  uuid        NOT NULL REFERENCES tenants (id),
    name       text        NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 255),
    publisher  text        NOT NULL DEFAULT '' CHECK (length(publisher) <= 255),
    created_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL
);
CREATE UNIQUE INDEX software_applications_key ON software_applications (tenant_id, lower(name), lower(publisher));
CREATE INDEX software_applications_tenant_idx ON software_applications (tenant_id);

-- Installed software, as last reported by the inventory.
CREATE TABLE asset_installations (
    asset_id       uuid        NOT NULL REFERENCES assets (id),
    tenant_id      uuid        NOT NULL REFERENCES tenants (id),
    application_id uuid        NOT NULL REFERENCES software_applications (id),
    version        text        NOT NULL DEFAULT '' CHECK (length(version) <= 100),
    last_seen_at   timestamptz NOT NULL,
    PRIMARY KEY (asset_id, application_id)
);
CREATE INDEX asset_installations_tenant_idx ON asset_installations (tenant_id);
CREATE INDEX asset_installations_application_idx ON asset_installations (application_id);

-- The hardware snapshot of an Asset, as last pushed (FR-AST-008).
CREATE TABLE asset_inventory (
    asset_id     uuid        PRIMARY KEY REFERENCES assets (id),
    tenant_id    uuid        NOT NULL REFERENCES tenants (id),
    snapshot     jsonb       NOT NULL,
    collected_at timestamptz NOT NULL
);
CREATE INDEX asset_inventory_tenant_idx ON asset_inventory (tenant_id);

-- Licences (FR-AST-014). The key is sealed with the keyring (ADR-0013).
CREATE TABLE licences (
    id              uuid PRIMARY KEY,
    tenant_id       uuid        NOT NULL REFERENCES tenants (id),
    licence_type    text        NOT NULL CHECK (length(btrim(licence_type)) BETWEEN 1 AND 100),
    licence_key     text        CHECK (length(licence_key) <= 4000),
    key_hint        text        CHECK (length(key_hint) <= 8),
    quantity        integer     NOT NULL CHECK (quantity >= 0),
    renewal_date    date,
    notice_period_days integer  CHECK (notice_period_days BETWEEN 0 AND 3650),
    cost            numeric(14, 2) CHECK (cost >= 0),
    currency        text        CHECK (currency ~ '^[A-Z]{3}$'),
    status          text        NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'expired', 'cancelled')),
    created_by_kind text        NOT NULL,
    created_by_id   uuid,
    created_at      timestamptz NOT NULL,
    updated_at      timestamptz NOT NULL,
    CHECK ((cost IS NULL) = (currency IS NULL))
);
CREATE INDEX licences_tenant_idx ON licences (tenant_id);

CREATE TABLE licence_applications (
    licence_id     uuid NOT NULL REFERENCES licences (id),
    tenant_id      uuid NOT NULL REFERENCES tenants (id),
    application_id uuid NOT NULL REFERENCES software_applications (id),
    PRIMARY KEY (licence_id, application_id)
);
CREATE INDEX licence_applications_tenant_idx ON licence_applications (tenant_id);

DO $$
DECLARE t text;
BEGIN
    FOREACH t IN ARRAY ARRAY['asset_custom_values', 'software_applications', 'asset_installations', 'asset_inventory', 'licences', 'licence_applications'] LOOP
        EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY', t);
        EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY', t);
        EXECUTE format('CREATE POLICY %I ON %I USING (cooper_tenant_visible(tenant_id)) WITH CHECK (cooper_tenant_visible(tenant_id))',
                       t || '_isolation', t);
    END LOOP;
END $$;
