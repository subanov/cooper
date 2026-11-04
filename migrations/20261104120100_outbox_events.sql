-- Transactional outbox for domain events (ADR-0005).
-- Producers insert in the same transaction as the state change; cooper-worker delivers
-- each event to its handlers at least once.
CREATE TABLE outbox_events (
    id              uuid PRIMARY KEY,
    tenant_id       uuid,
    event_type      text        NOT NULL,
    aggregate_type  text        NOT NULL,
    aggregate_id    uuid        NOT NULL,
    actor_kind      text        NOT NULL CHECK (actor_kind IN ('analyst', 'requester', 'api_key', 'automation')),
    actor_id        uuid,
    occurred_at     timestamptz NOT NULL,
    payload         jsonb       NOT NULL,
    attempts        integer     NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    next_attempt_at timestamptz NOT NULL,
    published_at    timestamptz,
    -- Set when delivery gave up after the maximum number of attempts.
    dead_at         timestamptz,
    last_error      text,
    CONSTRAINT outbox_events_actor_id_matches_kind CHECK ((actor_kind = 'automation') = (actor_id IS NULL)),
    CONSTRAINT outbox_events_single_terminal_state CHECK (published_at IS NULL OR dead_at IS NULL)
);

COMMENT ON TABLE outbox_events IS
    'Domain events waiting for (or finished with) delivery to in-process handlers.';

CREATE INDEX outbox_events_tenant_id_idx ON outbox_events (tenant_id);
CREATE INDEX outbox_events_pending_idx ON outbox_events (next_attempt_at, id)
    WHERE published_at IS NULL AND dead_at IS NULL;

ALTER TABLE outbox_events ENABLE ROW LEVEL SECURITY;
ALTER TABLE outbox_events FORCE ROW LEVEL SECURITY;
CREATE POLICY outbox_events_tenant ON outbox_events
    USING (cooper_tenant_visible(tenant_id))
    WITH CHECK (tenant_id IS NULL OR cooper_tenant_visible(tenant_id));
