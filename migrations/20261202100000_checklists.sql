-- Checklist Templates (FR-TSK-008), Checklists on tickets and tasks (FR-TKT-032), and
-- closure rules (ADR-0043). Templates and Checklists are Company-scoped (RLS).

CREATE TABLE checklist_templates (
    id         uuid PRIMARY KEY,
    tenant_id  uuid        NOT NULL REFERENCES tenants (id),
    name       text        NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 150),
    category   text        NOT NULL CHECK (length(btrim(category)) BETWEEN 1 AND 100),
    role       text        NOT NULL CHECK (length(btrim(role)) BETWEEN 1 AND 100),
    created_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL
);
CREATE UNIQUE INDEX checklist_templates_name_key ON checklist_templates (tenant_id, lower(name));

-- Versions are never changed once published.
CREATE TABLE checklist_template_versions (
    template_id     uuid        NOT NULL REFERENCES checklist_templates (id),
    tenant_id       uuid        NOT NULL REFERENCES tenants (id),
    version_no      integer     NOT NULL CHECK (version_no >= 1),
    closure_mode    text        NOT NULL CHECK (closure_mode IN ('advisory', 'blocking')),
    created_by_kind text        NOT NULL,
    created_by_id   uuid,
    created_at      timestamptz NOT NULL,
    PRIMARY KEY (template_id, version_no)
);

CREATE TABLE checklist_template_items (
    template_id    uuid    NOT NULL,
    tenant_id      uuid    NOT NULL REFERENCES tenants (id),
    version_no     integer NOT NULL,
    position       integer NOT NULL CHECK (position >= 0),
    item_text      text    NOT NULL CHECK (length(btrim(item_text)) BETWEEN 1 AND 500),
    is_mandatory   boolean NOT NULL,
    requires_input boolean NOT NULL,
    PRIMARY KEY (template_id, version_no, position),
    FOREIGN KEY (template_id, version_no) REFERENCES checklist_template_versions (template_id, version_no)
);

-- A Checklist on a ticket or task: a copy of one template version.
CREATE TABLE checklists (
    id               uuid PRIMARY KEY,
    tenant_id        uuid        NOT NULL REFERENCES tenants (id),
    target_kind      text        NOT NULL CHECK (target_kind IN ('ticket', 'task')),
    target_id        uuid        NOT NULL,
    template_id      uuid        NOT NULL REFERENCES checklist_templates (id),
    template_version integer     NOT NULL,
    name             text        NOT NULL,
    closure_mode     text        NOT NULL CHECK (closure_mode IN ('advisory', 'blocking')),
    created_at       timestamptz NOT NULL,
    FOREIGN KEY (template_id, template_version) REFERENCES checklist_template_versions (template_id, version_no)
);
CREATE INDEX checklists_target_idx ON checklists (tenant_id, target_kind, target_id);

CREATE TABLE checklist_items (
    id             uuid PRIMARY KEY,
    checklist_id   uuid        NOT NULL REFERENCES checklists (id),
    tenant_id      uuid        NOT NULL REFERENCES tenants (id),
    position       integer     NOT NULL CHECK (position >= 0),
    item_text      text        NOT NULL CHECK (length(btrim(item_text)) BETWEEN 1 AND 500),
    is_mandatory   boolean     NOT NULL,
    requires_input boolean     NOT NULL,
    is_done        boolean     NOT NULL DEFAULT false,
    answer         text        CHECK (length(answer) <= 2000),
    done_at        timestamptz,
    done_by_kind   text,
    done_by_id     uuid,
    UNIQUE (checklist_id, position),
    CHECK (NOT is_done OR done_at IS NOT NULL)
);

DO $$
DECLARE t text;
BEGIN
    FOREACH t IN ARRAY ARRAY['checklist_templates', 'checklist_template_versions', 'checklist_template_items',
                             'checklists', 'checklist_items'] LOOP
        EXECUTE format('CREATE INDEX %I ON %I (tenant_id)', t || '_tenant_idx', t);
        EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY', t);
        EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY', t);
        EXECUTE format('CREATE POLICY %I ON %I USING (cooper_tenant_visible(tenant_id)) WITH CHECK (cooper_tenant_visible(tenant_id))',
                       t || '_isolation', t);
    END LOOP;
END $$;
