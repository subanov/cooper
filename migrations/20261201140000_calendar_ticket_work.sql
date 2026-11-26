-- Generated Calendar Events for scheduled work on tickets (FR-CAL-002, FR-TKT-031).
ALTER TABLE calendar_events DROP CONSTRAINT calendar_events_source_kind_check;
ALTER TABLE calendar_events ADD CONSTRAINT calendar_events_source_kind_check
    CHECK (source_kind IN ('contract', 'asset', 'ticket_work'));
ALTER TABLE calendar_events DROP CONSTRAINT calendar_events_source_date_check;
ALTER TABLE calendar_events ADD CONSTRAINT calendar_events_source_date_check
    CHECK (source_date IN ('notice_date', 'end_date', 'warranty_expires_on', 'lease_expires_on', 'work_starts_on', 'work_ends_on'));
ALTER TABLE calendar_sources DROP CONSTRAINT calendar_sources_source_kind_check;
ALTER TABLE calendar_sources ADD CONSTRAINT calendar_sources_source_kind_check
    CHECK (source_kind IN ('contract', 'asset', 'ticket_work'));
