-- Forms on the Self-Service Portal, approval, Submission Actions, Submissions, votes and
-- Drafts (FR-FRM-002 to FR-FRM-005; ADR-0040).

-- Portal settings, approval rule and Submission Actions are the Form's current
-- configuration (not versioned); a Submission records the version it was filled in against.
ALTER TABLE forms
    ADD COLUMN portal_visible boolean NOT NULL DEFAULT false,
    ADD COLUMN approval_mode text CHECK (approval_mode IN ('analysts', 'team')),
    ADD COLUMN approval_team_id uuid,
    ADD COLUMN approval_required_approvals integer CHECK (approval_required_approvals BETWEEN 1 AND 50),
    ADD COLUMN approval_required_rejections integer CHECK (approval_required_rejections BETWEEN 1 AND 50),
    ADD COLUMN approval_exclude_requester boolean,
    ADD COLUMN ticket_category_id uuid,
    ADD COLUMN ticket_subject text CHECK (length(btrim(ticket_subject)) BETWEEN 1 AND 500),
    ADD COLUMN ticket_description text CHECK (length(ticket_description) <= 10000),
    ADD COLUMN task_title text CHECK (length(btrim(task_title)) BETWEEN 1 AND 255),
    ADD COLUMN task_description text CHECK (length(task_description) <= 10000),
    ADD CONSTRAINT forms_approval_complete CHECK (
        (approval_mode IS NULL) = (approval_required_approvals IS NULL)
        AND (approval_mode IS NULL) = (approval_required_rejections IS NULL)
        AND (approval_mode IS NULL) = (approval_exclude_requester IS NULL)
        AND (approval_mode <> 'team' OR approval_team_id IS NOT NULL)
    ),
    ADD CONSTRAINT forms_ticket_complete CHECK ((ticket_category_id IS NULL) = (ticket_subject IS NULL)),
    ADD CONSTRAINT forms_task_complete CHECK ((task_title IS NULL) = (task_description IS NULL));

-- Who may see a Portal-visible Form (none: every Requester of the Company).
CREATE TABLE form_audiences (
    form_id     uuid NOT NULL REFERENCES forms (id),
    tenant_id   uuid NOT NULL REFERENCES tenants (id),
    kind        text NOT NULL CHECK (kind IN ('requester', 'group', 'department')),
    audience_id uuid NOT NULL,
    PRIMARY KEY (form_id, kind, audience_id)
);

-- Analysts named by an approval rule (when it names Analysts rather than a Team).
CREATE TABLE form_approvers (
    form_id    uuid NOT NULL REFERENCES forms (id),
    tenant_id  uuid NOT NULL REFERENCES tenants (id),
    analyst_id uuid NOT NULL,
    PRIMARY KEY (form_id, analyst_id)
);

CREATE TABLE form_submissions (
    id            uuid PRIMARY KEY,
    tenant_id     uuid        NOT NULL REFERENCES tenants (id),
    form_id       uuid        NOT NULL REFERENCES forms (id),
    form_version  integer     NOT NULL,
    requester_id  uuid        NOT NULL REFERENCES requesters (id),
    answers       jsonb       NOT NULL DEFAULT '{}'::jsonb,
    status        text        NOT NULL CHECK (status IN ('pending_approval', 'returned', 'approved', 'completed')),
    round         integer     NOT NULL DEFAULT 1 CHECK (round >= 1),
    ticket_id     uuid REFERENCES tickets (id),
    task_id       uuid REFERENCES tasks (id),
    created_at    timestamptz NOT NULL,
    updated_at    timestamptz NOT NULL,
    FOREIGN KEY (form_id, form_version) REFERENCES form_versions (form_id, version_no)
);
CREATE INDEX form_submissions_tenant_idx ON form_submissions (tenant_id);
CREATE INDEX form_submissions_form_idx ON form_submissions (form_id, created_at DESC, id DESC);
CREATE INDEX form_submissions_requester_idx ON form_submissions (requester_id, created_at DESC, id DESC);

-- One row per vote; votes are never changed.
CREATE TABLE form_submission_votes (
    submission_id uuid        NOT NULL REFERENCES form_submissions (id),
    tenant_id     uuid        NOT NULL REFERENCES tenants (id),
    round         integer     NOT NULL CHECK (round >= 1),
    analyst_id    uuid        NOT NULL,
    decision      text        NOT NULL CHECK (decision IN ('approve', 'reject')),
    comment       text        NOT NULL DEFAULT '' CHECK (length(comment) <= 2000),
    cast_at       timestamptz NOT NULL,
    PRIMARY KEY (submission_id, round, analyst_id),
    CHECK (decision = 'approve' OR length(btrim(comment)) > 0)
);
CREATE INDEX form_submission_votes_tenant_idx ON form_submission_votes (tenant_id);

-- A Requester's unsubmitted answers to one Form.
CREATE TABLE form_drafts (
    tenant_id    uuid        NOT NULL REFERENCES tenants (id),
    form_id      uuid        NOT NULL REFERENCES forms (id),
    requester_id uuid        NOT NULL REFERENCES requesters (id),
    answers      jsonb       NOT NULL DEFAULT '{}'::jsonb,
    updated_at   timestamptz NOT NULL,
    PRIMARY KEY (form_id, requester_id)
);
CREATE INDEX form_drafts_tenant_idx ON form_drafts (tenant_id);

DO $$
DECLARE t text;
BEGIN
    FOREACH t IN ARRAY ARRAY['form_audiences', 'form_approvers', 'form_submissions', 'form_submission_votes', 'form_drafts'] LOOP
        EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY', t);
        EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY', t);
        EXECUTE format('CREATE POLICY %I ON %I USING (cooper_tenant_visible(tenant_id)) WITH CHECK (cooper_tenant_visible(tenant_id))',
                       t || '_isolation', t);
    END LOOP;
END $$;
