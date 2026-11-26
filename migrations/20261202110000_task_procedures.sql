-- Knowledge Articles attached to tasks as procedures, snapshotted when attached (FR-TSK-007).
CREATE TABLE task_procedures (
    id               uuid PRIMARY KEY,
    tenant_id        uuid        NOT NULL REFERENCES tenants (id),
    task_id          uuid        NOT NULL REFERENCES tasks (id),
    article_id       uuid        NOT NULL,
    article_version  integer     NOT NULL CHECK (article_version >= 1),
    title            text        NOT NULL CHECK (length(btrim(title)) BETWEEN 1 AND 500),
    body             text        NOT NULL CHECK (length(body) <= 200000),
    attached_by_kind text        NOT NULL,
    attached_by_id   uuid,
    attached_at      timestamptz NOT NULL
);
CREATE INDEX task_procedures_task_idx ON task_procedures (task_id);
CREATE INDEX task_procedures_tenant_idx ON task_procedures (tenant_id);
ALTER TABLE task_procedures ENABLE ROW LEVEL SECURITY;
ALTER TABLE task_procedures FORCE ROW LEVEL SECURITY;
CREATE POLICY task_procedures_isolation ON task_procedures
    USING (cooper_tenant_visible(tenant_id)) WITH CHECK (cooper_tenant_visible(tenant_id));
