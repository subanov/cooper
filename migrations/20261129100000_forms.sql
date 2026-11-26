-- Forms (FR-FRM-001, ADR-0039): administrator-built, versioned Forms per Company.

CREATE TABLE forms (
    id              uuid PRIMARY KEY,
    tenant_id       uuid        NOT NULL REFERENCES tenants (id),
    name            text        NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 150),
    description     text        NOT NULL DEFAULT '' CHECK (length(description) <= 2000),
    is_active       boolean     NOT NULL DEFAULT true,
    created_by_kind text        NOT NULL,
    created_by_id   uuid,
    created_at      timestamptz NOT NULL,
    updated_at      timestamptz NOT NULL
);
-- Form names are unique per Company (Cooper rule; FreeITSM does not enforce it).
CREATE UNIQUE INDEX forms_name_key ON forms (tenant_id, lower(name));

-- One row per published version. Versions are never updated.
CREATE TABLE form_versions (
    form_id         uuid        NOT NULL REFERENCES forms (id),
    tenant_id       uuid        NOT NULL REFERENCES tenants (id),
    version_no      integer     NOT NULL CHECK (version_no >= 1),
    created_by_kind text        NOT NULL,
    created_by_id   uuid,
    created_at      timestamptz NOT NULL,
    PRIMARY KEY (form_id, version_no)
);

-- Typed Form Fields of a version, in layout order.
CREATE TABLE form_fields (
    form_id      uuid     NOT NULL,
    tenant_id    uuid     NOT NULL REFERENCES tenants (id),
    version_no   integer  NOT NULL,
    position     integer  NOT NULL CHECK (position >= 0),
    field_key    text     NOT NULL CHECK (field_key ~ '^[a-z][a-z0-9_]{0,49}$'),
    label        text     NOT NULL CHECK (length(btrim(label)) BETWEEN 1 AND 255),
    kind         text     NOT NULL CHECK (kind IN ('text', 'long_text', 'number', 'date', 'boolean', 'choice')),
    is_required  boolean  NOT NULL DEFAULT false,
    options      text[]   NOT NULL DEFAULT '{}',
    width        text     NOT NULL CHECK (width IN ('full', 'half')),
    PRIMARY KEY (form_id, version_no, position),
    UNIQUE (form_id, version_no, field_key),
    FOREIGN KEY (form_id, version_no) REFERENCES form_versions (form_id, version_no),
    -- Only a choice field has options.
    CHECK ((kind = 'choice') = (cardinality(options) > 0))
);

CREATE INDEX forms_tenant_idx ON forms (tenant_id);
CREATE INDEX form_versions_tenant_idx ON form_versions (tenant_id);
CREATE INDEX form_fields_tenant_idx ON form_fields (tenant_id);

DO $$
DECLARE t text;
BEGIN
    FOREACH t IN ARRAY ARRAY['forms', 'form_versions', 'form_fields'] LOOP
        EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY', t);
        EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY', t);
        EXECUTE format('CREATE POLICY %I ON %I USING (cooper_tenant_visible(tenant_id)) WITH CHECK (cooper_tenant_visible(tenant_id))',
                       t || '_isolation', t);
    END LOOP;
END $$;
