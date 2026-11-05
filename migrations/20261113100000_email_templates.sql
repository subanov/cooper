-- Email Templates (FR-EML-009): automated emails on ticket events, matched by the
-- recipient's address or domain. Shared by every Company, like Reply Templates.
CREATE TABLE email_templates (
    id            uuid PRIMARY KEY,
    name          text        NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 100),
    event         text        NOT NULL CHECK (event IN ('ticket_created', 'ticket_assigned', 'ticket_replied', 'ticket_closed')),
    match_pattern text        CHECK (match_pattern IS NULL OR (match_pattern = lower(match_pattern) AND length(match_pattern) BETWEEN 3 AND 254)),
    subject       text        NOT NULL CHECK (length(subject) BETWEEN 1 AND 200),
    body          text        NOT NULL CHECK (length(body) BETWEEN 1 AND 20000),
    is_active     boolean     NOT NULL DEFAULT true,
    created_at    timestamptz NOT NULL,
    updated_at    timestamptz NOT NULL
);
CREATE UNIQUE INDEX email_templates_name_key ON email_templates (lower(name));
CREATE INDEX email_templates_event_idx ON email_templates (event) WHERE is_active;
