-- Tenant isolation helpers (ADR-0004) and the append-only Audit trail (FR-GEN-001).

-- Row-Level Security predicate shared by every tenant-scoped table.
-- The application sets two transaction-local settings (see cooper_platform::tenancy):
--   cooper.tenant_scope = 'all' | 'tenants'
--   cooper.tenant_ids   = comma-separated UUIDs (may be empty)
-- When neither is set the predicate is false: isolation fails closed.
CREATE FUNCTION cooper_tenant_visible(row_tenant uuid) RETURNS boolean
LANGUAGE sql STABLE PARALLEL SAFE AS $$
    SELECT coalesce(current_setting('cooper.tenant_scope', true), '') = 'all'
        OR coalesce(
               row_tenant = ANY (
                   string_to_array(nullif(current_setting('cooper.tenant_ids', true), ''), ',')::uuid[]
               ),
               false
           )
$$;

COMMENT ON FUNCTION cooper_tenant_visible(uuid) IS
    'RLS predicate: true when the row''s Company is in the current transaction''s tenant scope.';

-- Rejects UPDATE, DELETE and TRUNCATE on append-only tables, whatever the role's grants.
CREATE FUNCTION cooper_reject_modification() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'table % is append-only', TG_TABLE_NAME
        USING ERRCODE = 'insufficient_privilege';
END
$$;

CREATE TABLE audit_entries (
    id             uuid PRIMARY KEY,
    -- Groups the rows written for one audited action (one row per changed field).
    change_set_id  uuid        NOT NULL,
    tenant_id      uuid,
    aggregate_type text        NOT NULL,
    aggregate_id   uuid        NOT NULL,
    action         text        NOT NULL CHECK (action IN ('created', 'updated', 'deleted', 'restored')),
    field          text,
    old_value      jsonb,
    new_value      jsonb,
    actor_kind     text        NOT NULL CHECK (actor_kind IN ('analyst', 'requester', 'api_key', 'automation')),
    actor_id       uuid,
    occurred_at    timestamptz NOT NULL,
    CONSTRAINT audit_entries_actor_id_matches_kind CHECK ((actor_kind = 'automation') = (actor_id IS NULL))
);

COMMENT ON TABLE audit_entries IS
    'Append-only audit trail for every aggregate: who changed which field, from what, to what, and when.';

CREATE INDEX audit_entries_tenant_id_idx ON audit_entries (tenant_id);
CREATE INDEX audit_entries_aggregate_idx ON audit_entries (aggregate_type, aggregate_id, occurred_at);

CREATE TRIGGER audit_entries_append_only
    BEFORE UPDATE OR DELETE ON audit_entries
    FOR EACH ROW EXECUTE FUNCTION cooper_reject_modification();
CREATE TRIGGER audit_entries_no_truncate
    BEFORE TRUNCATE ON audit_entries
    FOR EACH STATEMENT EXECUTE FUNCTION cooper_reject_modification();

ALTER TABLE audit_entries ENABLE ROW LEVEL SECURITY;
ALTER TABLE audit_entries FORCE ROW LEVEL SECURITY;
CREATE POLICY audit_entries_read ON audit_entries FOR SELECT
    USING (cooper_tenant_visible(tenant_id));
-- Instance-wide records (tenant_id NULL) can be audited from any scope.
CREATE POLICY audit_entries_append ON audit_entries FOR INSERT
    WITH CHECK (tenant_id IS NULL OR cooper_tenant_visible(tenant_id));

-- Defence in depth for the application role, when it exists (see deploy/postgres/init).
DO $$
BEGIN
    IF EXISTS (SELECT FROM pg_roles WHERE rolname = 'cooper_app') THEN
        REVOKE UPDATE, DELETE, TRUNCATE ON audit_entries FROM cooper_app;
    END IF;
END
$$;
