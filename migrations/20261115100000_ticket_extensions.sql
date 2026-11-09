-- Ticket extensions (ADR-0024): Ticket Number History (FR-TKT-003), Ticket Type Rules
-- (FR-TKT-011), snooze metadata (FR-TKT-012), Ticket Presence (FR-TKT-025), Merge and
-- Split (FR-TKT-026, FR-TKT-027), Ticket Links (FR-TKT-028) and Ticket Fields
-- (FR-TKT-029).

-- Snooze: who put the ticket to sleep, and when (FR-TKT-012).
ALTER TABLE tickets
    ADD COLUMN snoozed_by_id  uuid REFERENCES analysts (id),
    ADD COLUMN snoozed_at     timestamptz,
    -- Merge (FR-TKT-026): a merged-away ticket stays, closed, and points at the survivor
    -- so old SDREF replies and links still find the conversation.
    ADD COLUMN merged_into_id uuid REFERENCES tickets (id),
    ADD CONSTRAINT tickets_not_merged_into_self CHECK (merged_into_id IS DISTINCT FROM id);
CREATE INDEX tickets_merged_into_idx ON tickets (merged_into_id) WHERE merged_into_id IS NOT NULL;

-- Ticket Number History (FR-TKT-003): numbers a ticket used to have. They still resolve.
CREATE TABLE ticket_number_history (
    id            uuid PRIMARY KEY,
    tenant_id     uuid        NOT NULL REFERENCES tenants (id),
    ticket_id     uuid        NOT NULL REFERENCES tickets (id),
    ticket_number text        NOT NULL,
    reason        text        NOT NULL CHECK (reason IN ('renumbered', 'imported')),
    retired_at    timestamptz NOT NULL
);
COMMENT ON TABLE ticket_number_history IS 'Previous Ticket Numbers; they still resolve to the ticket (FR-TKT-003).';
-- One retired number belongs to one ticket, for ever.
CREATE UNIQUE INDEX ticket_number_history_number_key ON ticket_number_history (ticket_number);
CREATE INDEX ticket_number_history_ticket_idx ON ticket_number_history (ticket_id);
CREATE INDEX ticket_number_history_tenant_id_idx ON ticket_number_history (tenant_id);
ALTER TABLE ticket_number_history ENABLE ROW LEVEL SECURITY;
ALTER TABLE ticket_number_history FORCE ROW LEVEL SECURITY;
CREATE POLICY ticket_number_history_isolation ON ticket_number_history
    USING (cooper_tenant_visible(tenant_id)) WITH CHECK (cooper_tenant_visible(tenant_id));

-- Ticket Type Rules (FR-TKT-011): reference data, instance-wide like Ticket Types. A
-- rule can be limited to Companies through its conditions.
CREATE TABLE ticket_type_rules (
    id             uuid PRIMARY KEY,
    name           text        NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 100),
    ticket_type_id uuid        NOT NULL REFERENCES ticket_types (id),
    conditions     jsonb       NOT NULL,
    sort_order     integer     NOT NULL DEFAULT 0,
    is_active      boolean     NOT NULL DEFAULT true,
    created_at     timestamptz NOT NULL,
    updated_at     timestamptz NOT NULL
);
COMMENT ON TABLE ticket_type_rules IS 'Rules choosing the Ticket Type of a new ticket, first match wins (FR-TKT-011).';
CREATE INDEX ticket_type_rules_order_idx ON ticket_type_rules (sort_order, id) WHERE is_active;

-- Ticket Presence (FR-TKT-025): who has a ticket open right now. A heartbeat, not an
-- audit trail: rows are overwritten and pruned.
CREATE TABLE ticket_presence (
    tenant_id    uuid        NOT NULL REFERENCES tenants (id),
    ticket_id    uuid        NOT NULL REFERENCES tickets (id) ON DELETE CASCADE,
    analyst_id   uuid        NOT NULL REFERENCES analysts (id) ON DELETE CASCADE,
    is_composing boolean     NOT NULL DEFAULT false,
    last_seen    timestamptz NOT NULL,
    PRIMARY KEY (ticket_id, analyst_id)
);
CREATE INDEX ticket_presence_last_seen_idx ON ticket_presence (last_seen);
CREATE INDEX ticket_presence_tenant_id_idx ON ticket_presence (tenant_id);
ALTER TABLE ticket_presence ENABLE ROW LEVEL SECURITY;
ALTER TABLE ticket_presence FORCE ROW LEVEL SECURITY;
CREATE POLICY ticket_presence_isolation ON ticket_presence
    USING (cooper_tenant_visible(tenant_id)) WITH CHECK (cooper_tenant_visible(tenant_id));

-- Ticket Links (FR-TKT-028). Stored as three kinds; "child of" is a parent link the
-- other way round. Links never cross Companies.
CREATE TABLE ticket_links (
    id               uuid PRIMARY KEY,
    tenant_id        uuid        NOT NULL REFERENCES tenants (id),
    source_ticket_id uuid        NOT NULL REFERENCES tickets (id),
    target_ticket_id uuid        NOT NULL REFERENCES tickets (id),
    kind             text        NOT NULL CHECK (kind IN ('related', 'duplicate', 'parent')),
    created_by_kind  text        NOT NULL,
    created_by_id    uuid,
    created_at       timestamptz NOT NULL,
    CONSTRAINT ticket_links_not_self CHECK (source_ticket_id <> target_ticket_id)
);
COMMENT ON TABLE ticket_links IS 'Links between tickets: related, duplicate (source duplicates target), parent (source is parent of target).';
CREATE UNIQUE INDEX ticket_links_pair_key ON ticket_links (source_ticket_id, target_ticket_id, kind);
-- "Related" is symmetric: A–B and B–A are the same link.
CREATE UNIQUE INDEX ticket_links_related_key ON ticket_links
    (least(source_ticket_id, target_ticket_id), greatest(source_ticket_id, target_ticket_id)) WHERE kind = 'related';
-- A ticket has at most one parent and duplicates at most one master.
CREATE UNIQUE INDEX ticket_links_one_parent_key ON ticket_links (target_ticket_id) WHERE kind = 'parent';
CREATE UNIQUE INDEX ticket_links_one_master_key ON ticket_links (source_ticket_id) WHERE kind = 'duplicate';
CREATE INDEX ticket_links_target_idx ON ticket_links (target_ticket_id);
CREATE INDEX ticket_links_tenant_id_idx ON ticket_links (tenant_id);
ALTER TABLE ticket_links ENABLE ROW LEVEL SECURITY;
ALTER TABLE ticket_links FORCE ROW LEVEL SECURITY;
CREATE POLICY ticket_links_isolation ON ticket_links
    USING (cooper_tenant_visible(tenant_id)) WITH CHECK (cooper_tenant_visible(tenant_id));

-- Merges (FR-TKT-026): exactly what moved, so a merge can be undone.
CREATE TABLE ticket_merges (
    id                       uuid PRIMARY KEY,
    tenant_id                uuid        NOT NULL REFERENCES tenants (id),
    source_ticket_id         uuid        NOT NULL REFERENCES tickets (id),
    target_ticket_id         uuid        NOT NULL REFERENCES tickets (id),
    moved                    jsonb       NOT NULL,
    link_id                  uuid,
    source_status_id         uuid        NOT NULL REFERENCES ticket_statuses (id),
    source_closed_at         timestamptz,
    source_resolution_code_id uuid REFERENCES resolution_codes (id),
    source_closure_category_id uuid REFERENCES ticket_categories (id),
    merged_by_kind           text        NOT NULL,
    merged_by_id             uuid,
    merged_at                timestamptz NOT NULL,
    undone_by_kind           text,
    undone_by_id             uuid,
    undone_at                timestamptz,
    CONSTRAINT ticket_merges_distinct CHECK (source_ticket_id <> target_ticket_id)
);
COMMENT ON TABLE ticket_merges IS 'Ticket merges with the rows they moved and the source''s previous state, for undo (FR-TKT-026).';
CREATE INDEX ticket_merges_source_idx ON ticket_merges (source_ticket_id);
CREATE INDEX ticket_merges_target_idx ON ticket_merges (target_ticket_id);
CREATE INDEX ticket_merges_tenant_id_idx ON ticket_merges (tenant_id);
ALTER TABLE ticket_merges ENABLE ROW LEVEL SECURITY;
ALTER TABLE ticket_merges FORCE ROW LEVEL SECURITY;
CREATE POLICY ticket_merges_isolation ON ticket_merges
    USING (cooper_tenant_visible(tenant_id)) WITH CHECK (cooper_tenant_visible(tenant_id));

-- Splits (FR-TKT-027): which emails and Notes moved to the new ticket, and the marker
-- Note left behind.
CREATE TABLE ticket_splits (
    id               uuid PRIMARY KEY,
    tenant_id        uuid        NOT NULL REFERENCES tenants (id),
    source_ticket_id uuid        NOT NULL REFERENCES tickets (id),
    new_ticket_id    uuid        NOT NULL REFERENCES tickets (id),
    moved            jsonb       NOT NULL,
    marker_note_id   uuid,
    link_id          uuid,
    split_by_kind    text        NOT NULL,
    split_by_id      uuid,
    split_at         timestamptz NOT NULL,
    undone_by_kind   text,
    undone_by_id     uuid,
    undone_at        timestamptz,
    CONSTRAINT ticket_splits_distinct CHECK (source_ticket_id <> new_ticket_id)
);
COMMENT ON TABLE ticket_splits IS 'Ticket splits with the rows they moved, for undo (FR-TKT-027).';
CREATE INDEX ticket_splits_source_idx ON ticket_splits (source_ticket_id);
CREATE INDEX ticket_splits_new_idx ON ticket_splits (new_ticket_id);
CREATE INDEX ticket_splits_tenant_id_idx ON ticket_splits (tenant_id);
ALTER TABLE ticket_splits ENABLE ROW LEVEL SECURITY;
ALTER TABLE ticket_splits FORCE ROW LEVEL SECURITY;
CREATE POLICY ticket_splits_isolation ON ticket_splits
    USING (cooper_tenant_visible(tenant_id)) WITH CHECK (cooper_tenant_visible(tenant_id));

-- Ticket Fields (FR-TKT-029): custom, typed fields; reference data, instance-wide.
CREATE TABLE ticket_fields (
    id                 uuid PRIMARY KEY,
    key                text        NOT NULL UNIQUE CHECK (key ~ '^[a-z][a-z0-9_]{0,62}$'),
    label              text        NOT NULL CHECK (length(btrim(label)) BETWEEN 1 AND 150),
    field_type         text        NOT NULL
        CHECK (field_type IN ('text', 'long_text', 'number', 'date', 'dropdown', 'checkbox')),
    options            jsonb       NOT NULL DEFAULT '[]',
    help_text          text        NOT NULL DEFAULT '',
    -- Empty: the field applies to every Ticket Type.
    type_ids           uuid[]      NOT NULL DEFAULT '{}',
    required_on_create boolean     NOT NULL DEFAULT false,
    required_on_close  boolean     NOT NULL DEFAULT false,
    ask_requester      boolean     NOT NULL DEFAULT false,
    sort_order         integer     NOT NULL DEFAULT 0,
    is_active          boolean     NOT NULL DEFAULT true,
    created_at         timestamptz NOT NULL,
    updated_at         timestamptz NOT NULL
);
COMMENT ON TABLE ticket_fields IS 'Custom Ticket Fields (FR-TKT-029). Retired, never deleted: values refer to them.';

CREATE TABLE ticket_field_values (
    tenant_id  uuid        NOT NULL REFERENCES tenants (id),
    ticket_id  uuid        NOT NULL REFERENCES tickets (id),
    field_id   uuid        NOT NULL REFERENCES ticket_fields (id),
    value      jsonb       NOT NULL,
    updated_at timestamptz NOT NULL,
    PRIMARY KEY (ticket_id, field_id)
);
CREATE INDEX ticket_field_values_field_idx ON ticket_field_values (field_id);
CREATE INDEX ticket_field_values_tenant_id_idx ON ticket_field_values (tenant_id);
ALTER TABLE ticket_field_values ENABLE ROW LEVEL SECURITY;
ALTER TABLE ticket_field_values FORCE ROW LEVEL SECURITY;
CREATE POLICY ticket_field_values_isolation ON ticket_field_values
    USING (cooper_tenant_visible(tenant_id)) WITH CHECK (cooper_tenant_visible(tenant_id));
