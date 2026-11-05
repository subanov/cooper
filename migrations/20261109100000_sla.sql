-- SLA (ADR-0015): calendars with working hours and Holidays, the SLA clock on tickets,
-- once-only Warning and Breach records, and SLA Notification Rules.

CREATE TABLE sla_calendars (
    id         uuid PRIMARY KEY,
    name       text        NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 100),
    -- IANA time zone name, for example Europe/London.
    time_zone  text        NOT NULL,
    is_default boolean     NOT NULL DEFAULT false,
    created_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL
);
CREATE UNIQUE INDEX sla_calendars_name_key ON sla_calendars (lower(name));
CREATE UNIQUE INDEX sla_calendars_single_default ON sla_calendars (is_default) WHERE is_default;

-- Working hours per weekday (1 = Monday … 7 = Sunday), local time.
CREATE TABLE sla_calendar_hours (
    calendar_id uuid     NOT NULL REFERENCES sla_calendars (id) ON DELETE CASCADE,
    weekday     smallint NOT NULL CHECK (weekday BETWEEN 1 AND 7),
    start_time  time     NOT NULL,
    end_time    time     NOT NULL,
    PRIMARY KEY (calendar_id, weekday),
    CONSTRAINT sla_calendar_hours_order CHECK (start_time < end_time)
);

CREATE TABLE sla_holidays (
    calendar_id  uuid NOT NULL REFERENCES sla_calendars (id) ON DELETE CASCADE,
    holiday_date date NOT NULL,
    PRIMARY KEY (calendar_id, holiday_date)
);

ALTER TABLE ticket_priorities
    ADD CONSTRAINT ticket_priorities_sla_calendar_fk FOREIGN KEY (sla_calendar_id) REFERENCES sla_calendars (id);

-- The SLA clock on tickets (FR-SLA-001 to FR-SLA-004).
ALTER TABLE tickets
    ADD COLUMN sla_calendar_id         uuid REFERENCES sla_calendars (id),
    ADD COLUMN response_minutes        integer CHECK (response_minutes > 0),
    ADD COLUMN resolution_minutes      integer CHECK (resolution_minutes > 0),
    ADD COLUMN response_due_at         timestamptz,
    ADD COLUMN resolution_due_at       timestamptz,
    ADD COLUMN response_warning_at     timestamptz,
    ADD COLUMN resolution_warning_at   timestamptz,
    ADD COLUMN responded_at            timestamptz,
    ADD COLUMN sla_paused_at           timestamptz,
    ADD COLUMN sla_paused_minutes      bigint NOT NULL DEFAULT 0 CHECK (sla_paused_minutes >= 0);

-- Running clocks the SLA job checks.
CREATE INDEX tickets_response_sla_idx ON tickets (response_warning_at)
    WHERE closed_at IS NULL AND deleted_at IS NULL AND responded_at IS NULL AND sla_paused_at IS NULL;
CREATE INDEX tickets_resolution_sla_idx ON tickets (resolution_warning_at)
    WHERE closed_at IS NULL AND deleted_at IS NULL AND sla_paused_at IS NULL;

-- Each Warning and Breach is raised at most once per ticket, target and trigger (FR-SLA-005).
CREATE TABLE sla_events (
    tenant_id  uuid        NOT NULL REFERENCES tenants (id),
    ticket_id  uuid        NOT NULL REFERENCES tickets (id),
    target     text        NOT NULL CHECK (target IN ('response', 'resolution')),
    trigger    text        NOT NULL CHECK (trigger IN ('warning', 'breach')),
    due_at     timestamptz NOT NULL,
    raised_at  timestamptz NOT NULL,
    PRIMARY KEY (ticket_id, target, trigger)
);
CREATE INDEX sla_events_tenant_id_idx ON sla_events (tenant_id);
ALTER TABLE sla_events ENABLE ROW LEVEL SECURITY;
ALTER TABLE sla_events FORCE ROW LEVEL SECURITY;
CREATE POLICY sla_events_isolation ON sla_events
    USING (cooper_tenant_visible(tenant_id)) WITH CHECK (cooper_tenant_visible(tenant_id));

-- Who is notified for which trigger and target (FR-SLA-006).
CREATE TABLE sla_notification_rules (
    id              uuid PRIMARY KEY,
    name            text        NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 100),
    trigger         text        NOT NULL CHECK (trigger IN ('warning', 'breach')),
    target          text        NOT NULL CHECK (target IN ('response', 'resolution', 'both')),
    notify_assignee boolean     NOT NULL DEFAULT true,
    notify_team     boolean     NOT NULL DEFAULT false,
    analyst_ids     uuid[]      NOT NULL DEFAULT '{}',
    is_active       boolean     NOT NULL DEFAULT true,
    created_at      timestamptz NOT NULL,
    updated_at      timestamptz NOT NULL
);
CREATE UNIQUE INDEX sla_notification_rules_name_key ON sla_notification_rules (lower(name));
