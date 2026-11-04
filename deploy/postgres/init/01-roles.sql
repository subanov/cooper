-- Runs once when the local Postgres volume is first created.
--
-- `cooper` (the POSTGRES_USER) owns the schema and runs migrations. In Docker it is a
-- superuser, and superusers bypass Row-Level Security, so the application should connect
-- as `cooper_app` to get RLS as a second layer of tenant isolation (ADR-0004).
CREATE ROLE cooper_app LOGIN PASSWORD 'cooper_app';
GRANT CONNECT ON DATABASE cooper TO cooper_app;
GRANT USAGE ON SCHEMA public TO cooper_app;

-- Tables and sequences created later by `cooper` are usable by the app role.
-- Append-only tables (audit) additionally reject UPDATE/DELETE with triggers.
ALTER DEFAULT PRIVILEGES FOR ROLE cooper IN SCHEMA public
    GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO cooper_app;
ALTER DEFAULT PRIVILEGES FOR ROLE cooper IN SCHEMA public
    GRANT USAGE, SELECT ON SEQUENCES TO cooper_app;
ALTER DEFAULT PRIVILEGES FOR ROLE cooper IN SCHEMA public
    GRANT EXECUTE ON FUNCTIONS TO cooper_app;
