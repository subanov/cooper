-- Contracts: Suppliers, Contacts and Contracts (FR-CON-001 to FR-CON-004; ADR-0035).

-- Configurable lookups, instance-wide like the other reference data. No seed values:
-- Administrators create them (Vocabulary).
CREATE TABLE supplier_types (
    id         uuid PRIMARY KEY,
    name       text        NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 50),
    sort_order integer     NOT NULL DEFAULT 0,
    is_active  boolean     NOT NULL DEFAULT true,
    created_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL
);
CREATE UNIQUE INDEX supplier_types_name_key ON supplier_types (lower(name));

CREATE TABLE supplier_statuses (
    id         uuid PRIMARY KEY,
    name       text        NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 50),
    sort_order integer     NOT NULL DEFAULT 0,
    is_active  boolean     NOT NULL DEFAULT true,
    created_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL
);
CREATE UNIQUE INDEX supplier_statuses_name_key ON supplier_statuses (lower(name));

CREATE TABLE contract_statuses (
    id         uuid PRIMARY KEY,
    name       text        NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 50),
    -- Behaviour flag: a Contract in a closed status (terminated, expired, …) is no longer
    -- active: no Calendar Events and no reminders.
    is_closed  boolean     NOT NULL DEFAULT false,
    sort_order integer     NOT NULL DEFAULT 0,
    is_active  boolean     NOT NULL DEFAULT true,
    created_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL
);
CREATE UNIQUE INDEX contract_statuses_name_key ON contract_statuses (lower(name));

CREATE TABLE payment_schedules (
    id         uuid PRIMARY KEY,
    name       text        NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 50),
    sort_order integer     NOT NULL DEFAULT 0,
    is_active  boolean     NOT NULL DEFAULT true,
    created_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL
);
CREATE UNIQUE INDEX payment_schedules_name_key ON payment_schedules (lower(name));

-- External vendors of a Company (FR-CON-001).
CREATE TABLE suppliers (
    id                  uuid PRIMARY KEY,
    tenant_id           uuid        NOT NULL REFERENCES tenants (id),
    legal_name          text        NOT NULL CHECK (length(btrim(legal_name)) BETWEEN 1 AND 255),
    trading_name        text        CHECK (length(btrim(trading_name)) BETWEEN 1 AND 255),
    registration_number text        CHECK (length(btrim(registration_number)) BETWEEN 1 AND 50),
    vat_number          text        CHECK (length(btrim(vat_number)) BETWEEN 1 AND 50),
    supplier_type_id    uuid REFERENCES supplier_types (id),
    supplier_status_id  uuid REFERENCES supplier_statuses (id),
    notes               text        NOT NULL DEFAULT '',
    is_active           boolean     NOT NULL DEFAULT true,
    created_by_kind     text        NOT NULL,
    created_by_id       uuid,
    created_at          timestamptz NOT NULL,
    updated_at          timestamptz NOT NULL
);
CREATE UNIQUE INDEX suppliers_legal_name_key ON suppliers (tenant_id, lower(legal_name));

-- People at a Supplier (Vocabulary: Contact; never Requesters).
CREATE TABLE supplier_contacts (
    id          uuid PRIMARY KEY,
    tenant_id   uuid        NOT NULL REFERENCES tenants (id),
    supplier_id uuid        NOT NULL REFERENCES suppliers (id),
    full_name   text        NOT NULL CHECK (length(btrim(full_name)) BETWEEN 1 AND 255),
    job_title   text        CHECK (length(job_title) <= 255),
    email       text        CHECK (length(email) <= 254),
    phone       text        CHECK (length(phone) <= 50),
    mobile      text        CHECK (length(mobile) <= 50),
    notes       text        NOT NULL DEFAULT '',
    is_active   boolean     NOT NULL DEFAULT true,
    created_at  timestamptz NOT NULL,
    updated_at  timestamptz NOT NULL
);
CREATE INDEX supplier_contacts_supplier_idx ON supplier_contacts (supplier_id);

-- Agreements with a Supplier or with the Company as customer (FR-CON-002, FR-CON-004).
CREATE TABLE contracts (
    id                        uuid PRIMARY KEY,
    tenant_id                 uuid        NOT NULL REFERENCES tenants (id),
    contract_number           text        NOT NULL CHECK (length(btrim(contract_number)) BETWEEN 1 AND 50),
    title                     text        NOT NULL CHECK (length(btrim(title)) BETWEEN 1 AND 255),
    description               text        NOT NULL DEFAULT '',
    -- Who the Contract is with: a Supplier, or the Company itself as customer (optionally
    -- with a Requester as its contact).
    party_kind                text        NOT NULL CHECK (party_kind IN ('supplier', 'customer')),
    supplier_id               uuid REFERENCES suppliers (id),
    requester_id              uuid REFERENCES requesters (id),
    owner_id                  uuid REFERENCES analysts (id),
    contract_status_id        uuid REFERENCES contract_statuses (id),
    start_date                date,
    end_date                  date,
    notice_period_amount      integer     CHECK (notice_period_amount BETWEEN 0 AND 3650),
    notice_period_unit        text        CHECK (notice_period_unit IN ('days', 'months')),
    -- Calculated by Cooper (Contract::notice_date: end date minus notice period) and
    -- stored for reminders, filters and sorting. FreeITSM: contracts.notice_date (typed in).
    notice_date               date,
    value                     numeric(14, 2) CHECK (value >= 0),
    currency                  text        CHECK (currency ~ '^[A-Z]{3}$'),
    payment_schedule_id       uuid REFERENCES payment_schedules (id),
    cost_centre_id            uuid REFERENCES cost_centres (id),
    dms_link                  text        CHECK (length(dms_link) <= 2000),
    personal_data_transferred boolean     NOT NULL DEFAULT false,
    dpia_required             boolean     NOT NULL DEFAULT false,
    dpia_completed_on         date,
    dpia_link                 text        CHECK (length(dpia_link) <= 2000),
    created_by_kind           text        NOT NULL,
    created_by_id             uuid,
    created_at                timestamptz NOT NULL,
    updated_at                timestamptz NOT NULL,
    deleted_at                timestamptz,
    CHECK ((party_kind = 'supplier') = (supplier_id IS NOT NULL)),
    CHECK (party_kind = 'customer' OR requester_id IS NULL),
    CHECK (start_date IS NULL OR end_date IS NULL OR start_date <= end_date),
    CHECK ((notice_period_amount IS NULL) = (notice_period_unit IS NULL)),
    CHECK (notice_date IS NULL OR (end_date IS NOT NULL AND notice_period_amount IS NOT NULL)),
    CHECK ((value IS NULL) = (currency IS NULL)),
    CHECK (dpia_required OR (dpia_completed_on IS NULL AND dpia_link IS NULL))
);
-- Contract numbers are unique per Company among Contracts that are not deleted.
CREATE UNIQUE INDEX contracts_number_key ON contracts (tenant_id, lower(contract_number)) WHERE deleted_at IS NULL;
CREATE INDEX contracts_supplier_idx ON contracts (supplier_id);
CREATE INDEX contracts_status_idx ON contracts (contract_status_id);
CREATE INDEX contracts_notice_date_idx ON contracts (notice_date) WHERE deleted_at IS NULL;
CREATE INDEX contracts_end_date_idx ON contracts (end_date) WHERE deleted_at IS NULL;

-- Reminders sent to Contract owners (FR-CON-003): at most one per Contract, date kind
-- and date.
CREATE TABLE contract_reminders (
    tenant_id     uuid        NOT NULL REFERENCES tenants (id),
    contract_id   uuid        NOT NULL REFERENCES contracts (id),
    kind          text        NOT NULL CHECK (kind IN ('notice_date', 'end_date')),
    reminded_date date        NOT NULL,
    owner_id      uuid        NOT NULL REFERENCES analysts (id),
    sent_at       timestamptz NOT NULL,
    PRIMARY KEY (contract_id, kind, reminded_date)
);

DO $$
DECLARE t text;
BEGIN
    FOREACH t IN ARRAY ARRAY['suppliers', 'supplier_contacts', 'contracts', 'contract_reminders'] LOOP
        EXECUTE format('CREATE INDEX %I ON %I (tenant_id)', t || '_tenant_idx', t);
        EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY', t);
        EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY', t);
        EXECUTE format('CREATE POLICY %I ON %I USING (cooper_tenant_visible(tenant_id)) WITH CHECK (cooper_tenant_visible(tenant_id))',
                       t || '_isolation', t);
    END LOOP;
END $$;

-- Contract owners are reminded of Notice Dates and end dates (FR-CON-003).
ALTER TABLE notifications DROP CONSTRAINT notifications_kind_check;
ALTER TABLE notifications ADD CONSTRAINT notifications_kind_check
    CHECK (kind IN ('ticket_assigned', 'mentioned', 'sla_warning', 'sla_breached', 'reply_received', 'mailbox_failing',
                    'change_submitted', 'change_approved', 'change_rejected', 'change_scheduled', 'change_completed',
                    'article_review_due', 'task_assigned', 'checks_unrecorded',
                    'contract_notice_due', 'contract_end_due'));
ALTER TABLE notifications ADD COLUMN contract_id uuid REFERENCES contracts (id);
