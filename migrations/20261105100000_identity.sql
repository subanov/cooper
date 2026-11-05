-- Identity context (ADR-0011): Companies, Analysts, sessions, Teams, Departments,
-- Company access, RBAC, Requesters and Cost Centres.

-- Companies. Called Company in the UI, Tenant in code (Vocabulary §1).
CREATE TABLE tenants (
    id          uuid PRIMARY KEY,
    name        text        NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 200),
    -- The {COMPANY} token in ticket numbers.
    ticket_code text        NOT NULL CHECK (ticket_code ~ '^[A-Z0-9]{1,10}$'),
    is_default  boolean     NOT NULL DEFAULT false,
    is_active   boolean     NOT NULL DEFAULT true,
    created_at  timestamptz NOT NULL,
    updated_at  timestamptz NOT NULL
);
COMMENT ON TABLE tenants IS 'Companies served by this Cooper instance (FR-IAM-008).';
CREATE UNIQUE INDEX tenants_name_key ON tenants (lower(name));
CREATE UNIQUE INDEX tenants_ticket_code_key ON tenants (ticket_code);
CREATE UNIQUE INDEX tenants_single_default ON tenants (is_default) WHERE is_default;

ALTER TABLE tenants ENABLE ROW LEVEL SECURITY;
ALTER TABLE tenants FORCE ROW LEVEL SECURITY;
CREATE POLICY tenants_isolation ON tenants
    USING (cooper_tenant_visible(id)) WITH CHECK (cooper_tenant_visible(id));

-- Public email domains that must never identify a Company (Vocabulary: Freemail Domain).
CREATE TABLE freemail_domains (
    domain text PRIMARY KEY CHECK (domain = lower(domain))
);
INSERT INTO freemail_domains (domain) VALUES
    ('aol.com'), ('gmail.com'), ('googlemail.com'), ('gmx.com'), ('gmx.de'), ('gmx.net'),
    ('hotmail.com'), ('hotmail.co.uk'), ('icloud.com'), ('live.com'), ('mail.com'),
    ('mail.ru'), ('me.com'), ('msn.com'), ('outlook.com'), ('proton.me'),
    ('protonmail.com'), ('web.de'), ('yahoo.com'), ('yahoo.co.uk'), ('yandex.com'),
    ('yandex.ru'), ('zoho.com');

-- Sender Domains: inbound mail from these domains belongs to the Company.
CREATE TABLE tenant_domains (
    id         uuid PRIMARY KEY,
    tenant_id  uuid        NOT NULL REFERENCES tenants (id) ON DELETE CASCADE,
    domain     text        NOT NULL CHECK (domain = lower(domain) AND domain ~ '^[a-z0-9.-]+\.[a-z]{2,}$'),
    created_at timestamptz NOT NULL
);
COMMENT ON TABLE tenant_domains IS 'Sender Domains mapping inbound email to a Company. Unique across Companies.';
CREATE UNIQUE INDEX tenant_domains_domain_key ON tenant_domains (domain);
CREATE INDEX tenant_domains_tenant_id_idx ON tenant_domains (tenant_id);
ALTER TABLE tenant_domains ENABLE ROW LEVEL SECURITY;
ALTER TABLE tenant_domains FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_domains_isolation ON tenant_domains
    USING (cooper_tenant_visible(tenant_id)) WITH CHECK (cooper_tenant_visible(tenant_id));

-- Analysts: IT staff. Instance-wide (not tenant-scoped).
CREATE TABLE analysts (
    id                  uuid PRIMARY KEY,
    username            text        NOT NULL CHECK (length(username) BETWEEN 1 AND 100),
    email               text        NOT NULL CHECK (position('@' IN email) > 1),
    full_name           text        NOT NULL CHECK (length(btrim(full_name)) BETWEEN 1 AND 200),
    is_admin            boolean     NOT NULL DEFAULT false,
    is_active           boolean     NOT NULL DEFAULT true,
    -- Argon2id PHC string. NULL until a password is set (for example SSO-only accounts).
    password_hash       text,
    password_changed_at timestamptz,
    deactivated_at      timestamptz,
    created_at          timestamptz NOT NULL,
    updated_at          timestamptz NOT NULL,
    CONSTRAINT analysts_deactivated_at_matches_flag CHECK (is_active = (deactivated_at IS NULL))
);
COMMENT ON TABLE analysts IS 'IT staff who sign in to Cooper (FR-IAM-001).';
CREATE UNIQUE INDEX analysts_username_key ON analysts (lower(username));
CREATE UNIQUE INDEX analysts_email_key ON analysts (lower(email));

-- Browser sessions (ADR-0011). Only the SHA-256 of the session token is stored.
CREATE TABLE analyst_sessions (
    id           uuid PRIMARY KEY,
    analyst_id   uuid        NOT NULL REFERENCES analysts (id) ON DELETE CASCADE,
    token_hash   bytea       NOT NULL,
    csrf_token   text        NOT NULL,
    created_at   timestamptz NOT NULL,
    last_seen_at timestamptz NOT NULL,
    idle_expires_at timestamptz NOT NULL,
    expires_at   timestamptz NOT NULL,
    revoked_at   timestamptz,
    ip_address   inet,
    user_agent   text
);
CREATE UNIQUE INDEX analyst_sessions_token_hash_key ON analyst_sessions (token_hash);
CREATE INDEX analyst_sessions_analyst_id_idx ON analyst_sessions (analyst_id);
CREATE INDEX analyst_sessions_expires_at_idx ON analyst_sessions (expires_at);

-- Teams (a Team is also a ticket Queue) and their members.
CREATE TABLE teams (
    id          uuid PRIMARY KEY,
    name        text        NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 200),
    description text        NOT NULL DEFAULT '',
    is_active   boolean     NOT NULL DEFAULT true,
    created_at  timestamptz NOT NULL,
    updated_at  timestamptz NOT NULL
);
CREATE UNIQUE INDEX teams_name_key ON teams (lower(name));

CREATE TABLE team_members (
    team_id    uuid NOT NULL REFERENCES teams (id) ON DELETE CASCADE,
    analyst_id uuid NOT NULL REFERENCES analysts (id) ON DELETE CASCADE,
    PRIMARY KEY (team_id, analyst_id)
);
CREATE INDEX team_members_analyst_id_idx ON team_members (analyst_id);

-- Departments: service areas with a default Sensitivity, served by Teams.
CREATE TABLE departments (
    id                  uuid PRIMARY KEY,
    name                text        NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 200),
    default_sensitivity text        NOT NULL DEFAULT 'normal' CHECK (default_sensitivity IN ('normal', 'confidential')),
    is_active           boolean     NOT NULL DEFAULT true,
    created_at          timestamptz NOT NULL,
    updated_at          timestamptz NOT NULL
);
CREATE UNIQUE INDEX departments_name_key ON departments (lower(name));

CREATE TABLE department_teams (
    department_id uuid NOT NULL REFERENCES departments (id) ON DELETE CASCADE,
    team_id       uuid NOT NULL REFERENCES teams (id) ON DELETE CASCADE,
    PRIMARY KEY (department_id, team_id)
);
CREATE INDEX department_teams_team_id_idx ON department_teams (team_id);

-- Company access for Analysts or Teams (FR-IAM-009). tenant_id NULL = all Companies.
CREATE TABLE company_access_grants (
    id         uuid PRIMARY KEY,
    analyst_id uuid REFERENCES analysts (id) ON DELETE CASCADE,
    team_id    uuid REFERENCES teams (id) ON DELETE CASCADE,
    tenant_id  uuid REFERENCES tenants (id) ON DELETE CASCADE,
    created_at timestamptz NOT NULL,
    CONSTRAINT company_access_grants_one_subject CHECK ((analyst_id IS NULL) <> (team_id IS NULL))
);
CREATE UNIQUE INDEX company_access_grants_key
    ON company_access_grants (analyst_id, team_id, tenant_id) NULLS NOT DISTINCT;
CREATE INDEX company_access_grants_team_id_idx ON company_access_grants (team_id);
CREATE INDEX company_access_grants_tenant_id_idx ON company_access_grants (tenant_id);

-- RBAC (FR-IAM-010): Roles are sets of Capabilities, assigned to Analysts or Teams.
CREATE TABLE rbac_roles (
    id          uuid PRIMARY KEY,
    name        text        NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 200),
    description text        NOT NULL DEFAULT '',
    created_at  timestamptz NOT NULL,
    updated_at  timestamptz NOT NULL
);
CREATE UNIQUE INDEX rbac_roles_name_key ON rbac_roles (lower(name));

CREATE TABLE rbac_role_capabilities (
    role_id    uuid NOT NULL REFERENCES rbac_roles (id) ON DELETE CASCADE,
    capability text NOT NULL CHECK (capability ~ '^[a-z][a-z0-9_-]*\.[a-z][a-z0-9_-]*$'),
    PRIMARY KEY (role_id, capability)
);

CREATE TABLE rbac_role_assignments (
    id         uuid PRIMARY KEY,
    role_id    uuid        NOT NULL REFERENCES rbac_roles (id) ON DELETE CASCADE,
    analyst_id uuid REFERENCES analysts (id) ON DELETE CASCADE,
    team_id    uuid REFERENCES teams (id) ON DELETE CASCADE,
    created_at timestamptz NOT NULL,
    CONSTRAINT rbac_role_assignments_one_subject CHECK ((analyst_id IS NULL) <> (team_id IS NULL))
);
CREATE UNIQUE INDEX rbac_role_assignments_key
    ON rbac_role_assignments (role_id, analyst_id, team_id) NULLS NOT DISTINCT;
CREATE INDEX rbac_role_assignments_analyst_id_idx ON rbac_role_assignments (analyst_id);
CREATE INDEX rbac_role_assignments_team_id_idx ON rbac_role_assignments (team_id);

-- Cost Centres, per Company.
CREATE TABLE cost_centres (
    id         uuid PRIMARY KEY,
    tenant_id  uuid        NOT NULL REFERENCES tenants (id),
    code       text        NOT NULL CHECK (length(btrim(code)) BETWEEN 1 AND 50),
    name       text        NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 200),
    is_active  boolean     NOT NULL DEFAULT true,
    created_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL
);
CREATE UNIQUE INDEX cost_centres_code_key ON cost_centres (tenant_id, lower(code));
CREATE INDEX cost_centres_tenant_id_idx ON cost_centres (tenant_id);
ALTER TABLE cost_centres ENABLE ROW LEVEL SECURITY;
ALTER TABLE cost_centres FORCE ROW LEVEL SECURITY;
CREATE POLICY cost_centres_isolation ON cost_centres
    USING (cooper_tenant_visible(tenant_id)) WITH CHECK (cooper_tenant_visible(tenant_id));

-- Requesters: end users who ask IT for help (FreeITSM table `users`).
CREATE TABLE requesters (
    id             uuid PRIMARY KEY,
    tenant_id      uuid        NOT NULL REFERENCES tenants (id),
    full_name      text        NOT NULL CHECK (length(btrim(full_name)) BETWEEN 1 AND 200),
    email          text        NOT NULL CHECK (position('@' IN email) > 1),
    username       text,
    phone          text,
    manager_id     uuid REFERENCES requesters (id),
    cost_centre_id uuid REFERENCES cost_centres (id),
    is_active      boolean     NOT NULL DEFAULT true,
    deactivated_at timestamptz,
    created_at     timestamptz NOT NULL,
    updated_at     timestamptz NOT NULL,
    CONSTRAINT requesters_deactivated_at_matches_flag CHECK (is_active = (deactivated_at IS NULL)),
    CONSTRAINT requesters_not_own_manager CHECK (manager_id IS DISTINCT FROM id)
);
COMMENT ON TABLE requesters IS 'People who ask IT for help (FR-IAM-011). FreeITSM: users.';
CREATE UNIQUE INDEX requesters_email_key ON requesters (tenant_id, lower(email));
CREATE UNIQUE INDEX requesters_username_key ON requesters (tenant_id, lower(username)) WHERE username IS NOT NULL;
CREATE INDEX requesters_tenant_id_idx ON requesters (tenant_id);
CREATE INDEX requesters_manager_id_idx ON requesters (manager_id);
CREATE INDEX requesters_name_idx ON requesters (tenant_id, lower(full_name), id);
ALTER TABLE requesters ENABLE ROW LEVEL SECURITY;
ALTER TABLE requesters FORCE ROW LEVEL SECURITY;
CREATE POLICY requesters_isolation ON requesters
    USING (cooper_tenant_visible(tenant_id)) WITH CHECK (cooper_tenant_visible(tenant_id));
