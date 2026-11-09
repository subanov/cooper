-- Reporting (FR-RPT-001, -002, -003, -005, -007; ADR-0032).

-- Watchtower Items (FR-RPT-003): which values a Watchtower count includes. Instance-wide,
-- like the statuses and Priorities they name (reference data has no Company). No rows
-- of a kind = the default rule of that kind. Values are validated against the owning
-- context's reference data when saved; there is no foreign key, since one column names
-- rows of four tables owned by three contexts.
CREATE TABLE watchtower_items (
    item       text        NOT NULL CHECK (item IN ('ticket_statuses', 'ticket_priorities', 'task_statuses', 'check_statuses')),
    member_id  uuid        NOT NULL,
    created_at timestamptz NOT NULL,
    PRIMARY KEY (item, member_id)
);
COMMENT ON TABLE watchtower_items IS 'Watchtower Items (FR-RPT-003): the statuses and Priorities Watchtower tiles count.';

-- Report queries. Created-in-period reports use tickets_list_idx (tenant_id, created_at).
-- Closed-in-period reports (closed per day/month, First Time Fix):
CREATE INDEX tickets_closed_at_idx ON tickets (tenant_id, closed_at)
    WHERE deleted_at IS NULL AND closed_at IS NOT NULL;
-- Open tickets by status, Priority or Assignee (Dashboard Widgets, the Watchtower):
CREATE INDEX tickets_open_status_idx ON tickets (tenant_id, status_id, priority_id)
    WHERE deleted_at IS NULL AND closed_at IS NULL;
-- Time-entry report (work dates in a period):
CREATE INDEX ticket_time_entries_work_date_idx ON ticket_time_entries (tenant_id, work_date);
CREATE INDEX task_time_entries_work_date_idx ON task_time_entries (tenant_id, work_date);
