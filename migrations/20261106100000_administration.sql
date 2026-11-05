-- Administration (ADR-0012): Module switches, System Settings and the system log.
-- All instance-wide (not tenant-scoped).

-- Module availability (FR-GEN-005). Rows exist only for Modules an Administrator has
-- changed; a missing row means "on".
CREATE TABLE modules (
    key        text PRIMARY KEY CHECK (key ~ '^[a-z][a-z-]*$'),
    is_enabled boolean     NOT NULL,
    updated_at timestamptz NOT NULL
);
COMMENT ON TABLE modules IS 'Module on/off switches. Missing row = enabled.';

-- System Settings (FR-SYS-001). Rows exist only for settings changed from their default;
-- definitions (type, bounds, default) live in code.
CREATE TABLE system_settings (
    key        text PRIMARY KEY CHECK (key ~ '^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*)+$'),
    value      jsonb       NOT NULL,
    updated_at timestamptz NOT NULL
);
COMMENT ON TABLE system_settings IS 'Stored System Setting values. Definitions and defaults are declared in code.';

-- System log (FR-SYS-003): warnings and errors from cooper-server and cooper-worker.
CREATE TABLE system_log_entries (
    id          uuid PRIMARY KEY,
    occurred_at timestamptz NOT NULL,
    level       text        NOT NULL CHECK (level IN ('warn', 'error')),
    source      text        NOT NULL CHECK (length(source) BETWEEN 1 AND 50),
    target      text        NOT NULL,
    message     text        NOT NULL,
    fields      jsonb       NOT NULL DEFAULT '{}'::jsonb,
    request_id  text
);
COMMENT ON TABLE system_log_entries IS 'Warnings and errors written by the system log tracing layer; pruned by a Background Job.';
CREATE INDEX system_log_entries_occurred_idx ON system_log_entries (occurred_at DESC, id DESC);
CREATE INDEX system_log_entries_level_idx ON system_log_entries (level, occurred_at DESC);
