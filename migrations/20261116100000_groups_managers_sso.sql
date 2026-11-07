-- User Groups (FR-IAM-016), Manager Grants (FR-IAM-015, FR-PRT-004) and OIDC single
-- sign-on (FR-IAM-012). ADR-0025.

-- User Groups: instance-wide; members are Analysts and/or Requesters.
CREATE TABLE user_groups (
    id          uuid PRIMARY KEY,
    name        text        NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 100),
    description text        NOT NULL DEFAULT '',
    is_active   boolean     NOT NULL DEFAULT true,
    created_at  timestamptz NOT NULL,
    updated_at  timestamptz NOT NULL
);
CREATE UNIQUE INDEX user_groups_name_key ON user_groups (lower(name));

CREATE TABLE user_group_members (
    group_id     uuid        NOT NULL REFERENCES user_groups (id) ON DELETE CASCADE,
    member_kind  text        NOT NULL CHECK (member_kind IN ('analyst', 'requester')),
    member_id    uuid        NOT NULL,
    -- Membership ends by itself at this time (FR-IAM-016).
    expires_at   timestamptz,
    added_at     timestamptz NOT NULL,
    PRIMARY KEY (group_id, member_kind, member_id)
);
CREATE INDEX user_group_members_member_idx ON user_group_members (member_kind, member_id);

-- Manager Grants: which Requesters' tickets a Requester (the manager) sees on the
-- Portal. Same Company only; exclusions always win.
CREATE TABLE manager_grants (
    id              uuid PRIMARY KEY,
    tenant_id       uuid        NOT NULL REFERENCES tenants (id),
    manager_id      uuid        NOT NULL REFERENCES requesters (id),
    kind            text        NOT NULL CHECK (kind IN ('everyone', 'requester', 'group', 'cost_centre', 'reports')),
    target_id       uuid,
    is_exclusion    boolean     NOT NULL DEFAULT false,
    created_by_kind text        NOT NULL,
    created_by_id   uuid,
    created_at      timestamptz NOT NULL,
    CONSTRAINT manager_grants_target CHECK ((kind IN ('everyone', 'reports')) = (target_id IS NULL)),
    CONSTRAINT manager_grants_exclusion CHECK (NOT is_exclusion OR kind IN ('requester', 'group', 'cost_centre'))
);
CREATE UNIQUE INDEX manager_grants_key ON manager_grants
    (manager_id, kind, coalesce(target_id, '00000000-0000-0000-0000-000000000000'::uuid), is_exclusion);
CREATE INDEX manager_grants_tenant_id_idx ON manager_grants (tenant_id);
ALTER TABLE manager_grants ENABLE ROW LEVEL SECURITY;
ALTER TABLE manager_grants FORCE ROW LEVEL SECURITY;
CREATE POLICY manager_grants_isolation ON manager_grants
    USING (cooper_tenant_visible(tenant_id)) WITH CHECK (cooper_tenant_visible(tenant_id));

-- OIDC providers (FR-IAM-012). The client secret is encrypted at rest (ADR-0013).
CREATE TABLE sso_providers (
    id                     uuid PRIMARY KEY,
    display_name           text        NOT NULL CHECK (length(btrim(display_name)) BETWEEN 1 AND 100),
    issuer_url             text        NOT NULL,
    client_id              text        NOT NULL,
    client_secret_enc      text,
    scopes                 text        NOT NULL DEFAULT 'openid email profile',
    audience               text        NOT NULL CHECK (audience IN ('analysts', 'requesters', 'both')),
    link_by_verified_email boolean     NOT NULL DEFAULT true,
    is_active              boolean     NOT NULL DEFAULT true,
    created_at             timestamptz NOT NULL,
    updated_at             timestamptz NOT NULL
);

-- Sign-ins in progress: state, nonce and PKCE verifier, single use, short-lived.
CREATE TABLE sso_logins (
    state_hash    bytea PRIMARY KEY,
    provider_id   uuid        NOT NULL REFERENCES sso_providers (id) ON DELETE CASCADE,
    audience      text        NOT NULL CHECK (audience IN ('analysts', 'requesters')),
    nonce         text        NOT NULL,
    pkce_verifier text        NOT NULL,
    return_to     text        NOT NULL,
    expires_at    timestamptz NOT NULL
);
CREATE INDEX sso_logins_expires_idx ON sso_logins (expires_at);

-- Who a provider's subject is in Cooper.
CREATE TABLE sso_identities (
    provider_id   uuid        NOT NULL REFERENCES sso_providers (id) ON DELETE CASCADE,
    subject       text        NOT NULL,
    audience      text        NOT NULL CHECK (audience IN ('analysts', 'requesters')),
    analyst_id    uuid REFERENCES analysts (id) ON DELETE CASCADE,
    portal_email  text,
    linked_at     timestamptz NOT NULL,
    last_login_at timestamptz,
    PRIMARY KEY (provider_id, audience, subject),
    CONSTRAINT sso_identities_one_target CHECK ((analyst_id IS NULL) <> (portal_email IS NULL))
);
CREATE UNIQUE INDEX sso_identities_analyst_key ON sso_identities (provider_id, analyst_id) WHERE analyst_id IS NOT NULL;
