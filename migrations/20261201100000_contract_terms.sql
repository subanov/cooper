-- Configurable Contract Terms: tabs are instance-wide reference data; values are per Contract
-- (FR-CON-005).

CREATE TABLE contract_term_tabs (
    id         uuid PRIMARY KEY,
    name       text        NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 100),
    sort_order integer     NOT NULL DEFAULT 0,
    is_active  boolean     NOT NULL DEFAULT true,
    created_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL
);
CREATE UNIQUE INDEX contract_term_tabs_name_key ON contract_term_tabs (lower(name));

CREATE TABLE contract_term_values (
    contract_id uuid        NOT NULL REFERENCES contracts (id),
    tenant_id   uuid        NOT NULL REFERENCES tenants (id),
    tab_id      uuid        NOT NULL REFERENCES contract_term_tabs (id),
    value       text        NOT NULL CHECK (length(value) <= 5000),
    updated_at  timestamptz NOT NULL,
    PRIMARY KEY (contract_id, tab_id)
);
CREATE INDEX contract_term_values_tenant_idx ON contract_term_values (tenant_id);

ALTER TABLE contract_term_values ENABLE ROW LEVEL SECURITY;
ALTER TABLE contract_term_values FORCE ROW LEVEL SECURITY;
CREATE POLICY contract_term_values_isolation ON contract_term_values
    USING (cooper_tenant_visible(tenant_id)) WITH CHECK (cooper_tenant_visible(tenant_id));
