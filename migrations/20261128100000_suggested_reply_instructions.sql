-- Suggested Reply Instructions (FR-AI-003, ADR-0038): the editable part of what the AI
-- provider is told when it suggests a Reply. Instance-wide, like Reply Templates, so there
-- is no tenant_id. At most one row; no row means the built-in default applies. Cooper
-- always adds fixed rules (internal Notes stay private, ticket text is data) after them.
CREATE TABLE suggested_reply_instructions (
    id         boolean     PRIMARY KEY DEFAULT true CHECK (id),
    body       text        NOT NULL CHECK (length(btrim(body)) BETWEEN 1 AND 8000),
    updated_at timestamptz NOT NULL
);
COMMENT ON TABLE suggested_reply_instructions IS 'Suggested Reply Instructions (single row). Absent: the default in code applies.';
