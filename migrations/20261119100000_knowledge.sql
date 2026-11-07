-- Knowledge (FR-KNW-001 to -009, FR-PRT-005, FR-PRB-005; ADR-0028).
--
-- Articles and Folders may belong to a Company or be shared with every Company
-- (tenant_id IS NULL), as in FreeITSM. Their RLS policy therefore lets NULL rows through;
-- the application adds the audience ladder and the access lists on top.

CREATE TABLE knowledge_folders (
    id                  uuid PRIMARY KEY,
    tenant_id           uuid REFERENCES tenants (id),
    parent_id           uuid REFERENCES knowledge_folders (id),
    name                text        NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 255),
    -- Open (false): readable unless an access row names you. Restricted (true): only
    -- if one does. The polarity lives on the object, never on the rows.
    is_restricted       boolean     NOT NULL DEFAULT false,
    -- Take permissions from the parent and ignore my own rows.
    inherit_permissions boolean     NOT NULL DEFAULT true,
    owner_id            uuid REFERENCES analysts (id),
    created_by_kind     text        NOT NULL,
    created_by_id       uuid,
    created_at          timestamptz NOT NULL,
    updated_at          timestamptz NOT NULL,
    CHECK (parent_id IS DISTINCT FROM id)
);
CREATE UNIQUE INDEX knowledge_folders_name_key
    ON knowledge_folders (coalesce(parent_id, '00000000-0000-0000-0000-000000000000'::uuid), lower(name));

CREATE TABLE knowledge_articles (
    id                  uuid PRIMARY KEY,
    tenant_id           uuid REFERENCES tenants (id),
    folder_id           uuid REFERENCES knowledge_folders (id),
    title               text        NOT NULL CHECK (length(btrim(title)) BETWEEN 1 AND 255),
    -- Sanitised HTML (ammonia), never raw input.
    body                text        NOT NULL DEFAULT '',
    -- Plain text of the body, for search and for inserting into replies.
    body_text           text        NOT NULL DEFAULT '',
    tags                text[]      NOT NULL DEFAULT '{}',
    audience            text        NOT NULL DEFAULT 'internal' CHECK (audience IN ('internal', 'customer', 'public')),
    author_id           uuid REFERENCES analysts (id),
    owner_id            uuid REFERENCES analysts (id),
    is_published        boolean     NOT NULL DEFAULT false,
    is_archived         boolean     NOT NULL DEFAULT false,
    archived_at         timestamptz,
    is_restricted       boolean     NOT NULL DEFAULT false,
    inherit_permissions boolean     NOT NULL DEFAULT true,
    next_review_date    date,
    -- The Review Date the Owner was last notified about (one Notification per date).
    review_notified_for date,
    version             integer     NOT NULL DEFAULT 1 CHECK (version >= 1),
    view_count          bigint      NOT NULL DEFAULT 0,
    -- The Known Error this Article was published from (FR-PRB-005), by ID only.
    source_problem_id   uuid,
    created_by_kind     text        NOT NULL,
    created_by_id       uuid,
    created_at          timestamptz NOT NULL,
    updated_at          timestamptz NOT NULL,
    search              tsvector GENERATED ALWAYS AS (
        setweight(to_tsvector('english'::regconfig, title), 'A')
        || setweight(to_tsvector('english'::regconfig, body_text), 'B')
    ) STORED
);
CREATE INDEX knowledge_folders_tenant_idx ON knowledge_folders (tenant_id);
CREATE INDEX knowledge_articles_tenant_idx ON knowledge_articles (tenant_id);
CREATE INDEX knowledge_articles_folder_idx ON knowledge_articles (folder_id);
CREATE INDEX knowledge_articles_search_idx ON knowledge_articles USING gin (search);
CREATE INDEX knowledge_articles_tags_idx ON knowledge_articles USING gin (tags);
CREATE INDEX knowledge_articles_review_idx ON knowledge_articles (next_review_date) WHERE NOT is_archived;
CREATE UNIQUE INDEX knowledge_articles_problem_key ON knowledge_articles (source_problem_id) WHERE source_problem_id IS NOT NULL;

-- Every saved state of an Article (FR-KNW-002). Immutable.
CREATE TABLE knowledge_article_versions (
    article_id     uuid        NOT NULL REFERENCES knowledge_articles (id),
    tenant_id      uuid REFERENCES tenants (id),
    version_no     integer     NOT NULL CHECK (version_no >= 1),
    title          text        NOT NULL,
    body           text        NOT NULL,
    saved_by_kind  text        NOT NULL,
    saved_by_id    uuid,
    saved_at       timestamptz NOT NULL,
    PRIMARY KEY (article_id, version_no)
);
CREATE INDEX knowledge_article_versions_tenant_idx ON knowledge_article_versions (tenant_id);
CREATE TRIGGER knowledge_article_versions_append_only BEFORE UPDATE OR DELETE ON knowledge_article_versions
    FOR EACH ROW EXECUTE FUNCTION cooper_reject_modification();

-- Access lists of Folders and Articles (FR-KNW-005).
CREATE TABLE knowledge_acl (
    -- The Company of the Folder or Article (NULL when it is shared).
    tenant_id      uuid REFERENCES tenants (id),
    object_type    text        NOT NULL CHECK (object_type IN ('folder', 'article')),
    object_id      uuid        NOT NULL,
    principal_type text        NOT NULL CHECK (principal_type IN ('analyst', 'team', 'requester', 'user_group')),
    principal_id   uuid        NOT NULL,
    created_by_kind text       NOT NULL,
    created_by_id  uuid,
    created_at     timestamptz NOT NULL,
    PRIMARY KEY (object_type, object_id, principal_type, principal_id)
);
CREATE INDEX knowledge_acl_tenant_idx ON knowledge_acl (tenant_id);

-- Views and administrator overrides (FR-KNW-009). A different volume class from the
-- audit trail of changes, so it has its own append-only table.
CREATE TABLE knowledge_access_log (
    id          uuid PRIMARY KEY,
    tenant_id   uuid REFERENCES tenants (id),
    article_id  uuid        NOT NULL REFERENCES knowledge_articles (id),
    action      text        NOT NULL CHECK (action IN ('viewed', 'admin_override')),
    actor_kind  text        NOT NULL,
    actor_id    uuid,
    occurred_at timestamptz NOT NULL
);
CREATE INDEX knowledge_access_log_article_idx ON knowledge_access_log (article_id, occurred_at);
CREATE INDEX knowledge_access_log_tenant_idx ON knowledge_access_log (tenant_id);
CREATE TRIGGER knowledge_access_log_append_only BEFORE UPDATE OR DELETE ON knowledge_access_log
    FOR EACH ROW EXECUTE FUNCTION cooper_reject_modification();

-- Articles linked to tickets (FR-KNW-008), in the ticket's Company.
CREATE TABLE knowledge_article_tickets (
    tenant_id      uuid        NOT NULL REFERENCES tenants (id),
    article_id     uuid        NOT NULL REFERENCES knowledge_articles (id),
    ticket_id      uuid        NOT NULL REFERENCES tickets (id),
    linked_by_kind text        NOT NULL,
    linked_by_id   uuid,
    linked_at      timestamptz NOT NULL,
    PRIMARY KEY (article_id, ticket_id)
);
CREATE INDEX knowledge_article_tickets_tenant_idx ON knowledge_article_tickets (tenant_id);
CREATE INDEX knowledge_article_tickets_ticket_idx ON knowledge_article_tickets (ticket_id);

DO $$
DECLARE t text;
BEGIN
    -- Shared rows (no Company) are visible to every scope.
    FOREACH t IN ARRAY ARRAY['knowledge_folders', 'knowledge_articles', 'knowledge_article_versions',
                             'knowledge_acl', 'knowledge_access_log'] LOOP
        EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY', t);
        EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY', t);
        EXECUTE format('CREATE POLICY %I ON %I USING (tenant_id IS NULL OR cooper_tenant_visible(tenant_id))
                        WITH CHECK (tenant_id IS NULL OR cooper_tenant_visible(tenant_id))', t || '_isolation', t);
    END LOOP;
    ALTER TABLE knowledge_article_tickets ENABLE ROW LEVEL SECURITY;
    ALTER TABLE knowledge_article_tickets FORCE ROW LEVEL SECURITY;
    CREATE POLICY knowledge_article_tickets_isolation ON knowledge_article_tickets
        USING (cooper_tenant_visible(tenant_id)) WITH CHECK (cooper_tenant_visible(tenant_id));
END $$;

-- Review Date reminders for Owners (FR-KNW-007).
ALTER TABLE notifications DROP CONSTRAINT notifications_kind_check;
ALTER TABLE notifications ADD CONSTRAINT notifications_kind_check
    CHECK (kind IN ('ticket_assigned', 'mentioned', 'sla_warning', 'sla_breached', 'reply_received', 'mailbox_failing',
                    'change_submitted', 'change_approved', 'change_rejected', 'change_scheduled', 'change_completed',
                    'article_review_due'));
ALTER TABLE notifications ADD COLUMN article_id uuid REFERENCES knowledge_articles (id);
