# 0041. Asset extras: custom fields, software inventory, licences and CSV import

- **Status:** Accepted
- **Date:** 2026-11-29
- **Issue:** #168, #172, #173, #175, #176 (with #174 and #188 for the calendar side)
- **Requirements:** FR-AST-003, FR-AST-007, FR-AST-008, FR-AST-013, FR-AST-014, FR-CAL-002

## Context

FreeITSM keeps custom asset fields, an inventory fed by agents and scripts, software and
licences, and an import from spreadsheets. Each needs a model that stays correct under
multiple Companies and concurrent writers.

## Decision

- **Custom fields (FR-AST-003).** Field Sets are instance-wide reference data, assigned to
  Asset Types (many to many). A field has a key unique across all Field Sets, a type
  (text, number, date, boolean, choice), required, unique, searchable and list flags. Values
  live in `asset_custom_values`, one row per Asset and field. Values are checked on write:
  unknown keys and inactive fields are refused, required fields must be filled in, and a
  unique field may not repeat another Asset's value (case-insensitive, checked in the
  application under the Company's tenancy). Key, type and options are fixed once created, so
  stored values stay meaningful. Deviation from FreeITSM: Cooper checks types on write; the
  "searchable" flag is stored for search, not yet a list filter.
- **Software (FR-AST-013).** Applications are distinct per Company by name and publisher
  (case-insensitive). Installations are per Asset with the version last reported. A new
  report replaces the installations: software that is no longer reported is removed.
- **Inventory push (FR-AST-008).** `POST /inventory/push` needs `assets.inventory` (an API
  Key or an Analyst). It finds an existing, active Asset by service tag, then hostname, then
  asset tag; an unknown or ambiguous identifier is refused, so the push never creates
  Assets. The hardware, disks and network adapters are kept as one JSON snapshot per Asset
  (`asset_inventory`), since they are read as a whole.
- **Licences (FR-AST-014).** A licence has a type, quantity, renewal date, notice period,
  cost (exact, with currency) and status. Its key is sealed with the keyring (ADR-0013,
  purpose `licences.licence_key:<id>`), never returned, and only its last four characters are
  shown. Applications are linked to a licence; compliance counts the distinct active Assets
  with any of them installed against the quantity: `compliant`, `over_used`, or
  `no_applications` when nothing is linked.
- **CSV import (FR-AST-007).** The file is parsed and mapped to fields first; a preview
  (`dry_run`) returns the same per-row outcome as a real run, with nothing written. Assets
  are matched by asset tag, service tag, then hostname. `fill` only sets empty fields,
  `overwrite` sets every field the file gives. New Assets need an Asset Type. Assets of that
  type that the file does not mention are `ignore`d, `flag`ged, or `deactivate`d (retired
  with the reason "Missing from an Asset import"; a checked-out Asset is reported as failed).
  Writes go through the Assets service, so its Capabilities and audit apply. The import is
  limited to 2000 rows. The outcome log is returned to the caller and not stored.
- **Calendar (FR-CAL-002).** The Calendar already consumes `asset.dates_changed` and
  `contract.dates_changed` (ADR-0036), so warranty and lease dates appear as generated
  events. Change windows, planned maintenance, task due dates and ticket scheduled work are
  not generated yet: their sources do not publish the needed events.
- **Notifications (FR-AST-012).** Not built: Assets have no owner to notify. The calendar
  events are the reminder for now.

## Consequences

- Inventory and licence keys are sensitive: the key is sealed at rest and never read back;
  the push endpoint needs its own Capability.
- The import is synchronous and bounded; very large files need a background job later.
