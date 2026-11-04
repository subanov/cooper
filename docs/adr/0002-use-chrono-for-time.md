# 0002. Use `chrono` and `chrono-tz` for time

- **Status:** Accepted
- **Date:** 2026-11-04
- **Issue:** #2
- **Requirements:** FR-SLA-002, FR-SLA-003, FR-TSK-006, FR-SYS-002

## Context

AGENTS.md requires one time crate across the workspace: `time` or `chrono`. Time is
central to Cooper:

- SLA due times count only working minutes in an SLA Calendar's **time zone**, skip
  Holidays, and must handle daylight-saving transitions (FR-SLA-002).
- Task recurrence ("2nd Tuesday of each month") needs calendar arithmetic (FR-TSK-006).
- Background job schedules run "daily at 03:00" (FR-SYS-002).
- The database stores `TIMESTAMPTZ` in UTC (AGENTS.md §5).

FreeITSM stores `DATETIME` without a time zone and mixes a UTC database session with a
Europe/London PHP default. Cooper must not repeat that.

## Options considered

1. **`time` 0.3.** Small and well maintained, and `sqlx` supports it. Its IANA time zone
   support needs third-party crates (`time-tz`, `tzdb`) that are less widely used, and
   `OffsetDateTime` carries only a fixed offset, so DST-aware arithmetic has to be built
   by hand.
2. **`chrono` 0.4 + `chrono-tz`.** The most widely used option. `sqlx`, `serde` and
   `proptest` support it. `chrono-tz` compiles the IANA database in and gives
   `DateTime<Tz>` with correct DST behaviour, including ambiguous and skipped local
   times through `LocalResult`. Its historical soundness issue (`localtime_r`) was fixed
   in 0.4.20+.
3. **`jiff`.** An excellent modern design with built-in time zones, but it is outside
   the AGENTS.md shortlist and is younger. It would need its own ADR to adopt.

## Decision

Use **`chrono`** (default features off; `clock`, `serde` and `std` on) workspace-wide.
Add **`chrono-tz`** when the SLA Calendar work starts.

- Instants are `DateTime<Utc>`. Domain code never calls `Utc::now()`; it takes a
  `&dyn cooper_kernel::Clock` or a `DateTime<Utc>` (AGENTS.md §3.3).
- Local-time concepts (SLA working hours, daily schedules, Holidays) use `NaiveTime` and
  `NaiveDate` plus an explicit `chrono_tz::Tz`. They are never converted implicitly.
- The database uses `timestamptz` for instants, `time` and `date` for local values, and
  stores time zone names as IANA strings.

## Consequences

- AGENTS.md references to `OffsetDateTime` now read `DateTime<Utc>`.
- SLA arithmetic must be property-tested across DST transitions. `chrono-tz` makes those
  transitions explicit, which is what we want.
- `chrono-tz` embeds the time zone database, so updates arrive through Dependabot crate
  bumps rather than the operating system.
