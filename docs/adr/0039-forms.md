# 0039. Forms: per-Company, versioned Forms with typed Form Fields

- **Status:** Accepted
- **Date:** 2026-11-29
- **Issue:** #189 (#190 to #193 follow in the same context)
- **Requirements:** FR-FRM-001 (FR-FRM-002 to FR-FRM-005 to follow)

## Context

FreeITSM builds Forms in its admin UI from typed fields and runs them from the Portal.
Forms are versioned there, and a submission should keep the version it was filled in
against. Forms are a new bounded context (`cooper-forms`) behind the **Forms** Module
(`/forms`).

## Decision

- **Forms belong to a Company** (`tenant_id`, Row-Level Security), like Suppliers,
  Contracts and Assets. Deviation from FreeITSM: single-tenant there, so a Form is shared
  by every Company.
- **Names are unique per Company** (case-insensitive). FreeITSM does not enforce this.
- **Form Fields are typed**: `text`, `long_text`, `number`, `date`, `boolean` and
  `choice`. Keys are stable identifiers (lower-case, at most 50 characters), unique within a
  version. Only a choice field has options (1 to 50, unique case-insensitively). Layout
  width is `full` or `half`. A version has 1 to 100 fields.
- **Versions are immutable.** Changing the fields publishes version `n + 1` and makes it
  current; earlier versions stay as they were. Changing the name, description or active flag
  does not publish a version. Submissions (FR-FRM-003, later) will reference the version they
  were filled in against.
- **Storage**: `forms`, `form_versions` and `form_fields` (one row per field, with
  `position`), with CHECK constraints for the closed sets. Saving a Form writes new versions
  and fields with `ON CONFLICT DO NOTHING`, so stored versions cannot change.
- **Capabilities**: `forms.read` and `forms.write`. Requesters never use the staff API.
- **Audit**: every create and edit writes an audit entry; a new version records the field
  list in its audit entry.

## Consequences

- Portal visibility and audiences (FR-FRM-002), Submission Actions (FR-FRM-003), approval
  rounds (FR-FRM-004) and Drafts (FR-FRM-005) build on this model. They need Submissions,
  which are not part of this ADR.
- The Forms Module switch hides `/api/v1/forms` when turned off (FR-GEN-005).
