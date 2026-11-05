# 0012. Administration context: Module switches, typed System Settings, job schedules and a database-backed system log

- **Status:** Accepted
- **Date:** 2026-11-06
- **Issue:** #27, #75, #76, #77
- **Requirements:** FR-GEN-005, FR-SYS-001, FR-SYS-002, FR-SYS-003
- **Related:** [ADR-0009](0009-shared-crates-and-dependency-baseline.md),
  [ADR-0011](0011-identity-context-sessions-and-rbac.md)

## Context

Administrators must be able to turn Modules off, change System Settings, change
Background Job schedules, see job run history and read warnings and errors without
shell access to the servers. FreeITSM keeps settings as untyped key/value rows read
throughout the PHP code, and its "system log" mixes application events with PHP errors.

## Decision

A new `cooper-administration` crate (`crates/administration`) owns these use cases.

- **Modules** are a fixed set in the kernel (`cooper_kernel::Module`). The `modules`
  table only stores switches an Administrator changed; a missing row means "on".
  `Tickets` cannot be turned off because every other Module builds on it.
  The server registers which `/api/v1` path prefixes belong to which Module; the
  `module_gate` middleware answers `404` for a disabled Module's routes (FR-GEN-005).
  The gate caches switches for 5 seconds, so a change made on one instance reaches the
  others within that time. It fails open on a storage error: switches are about
  navigation and scope, not security (Capabilities and tenant scope still apply).
- **System Settings** are **typed**: each context declares the settings it reads as
  `SettingDefinition` constants (key, description, kind and bounds, default). Only values
  an Administrator changed are stored (`system_settings`), validated against the
  definition; a stored value that no longer fits is ignored in favour of the default.
  Other contexts read values through `AdministrationService::setting_value`.
- **Background Jobs** stay owned by `cooper-platform` (ADR-0009); the platform exposes
  `jobs::admin` functions to list jobs and runs, change a schedule (on the caller's
  transaction, so the change is audited atomically) and request a run. "Run now" works
  even for jobs whose schedule is `off`.
- **System log**: a `tracing` layer in `cooper-platform` copies every `WARN` and `ERROR`
  event (with its fields and the HTTP request ID) into a bounded channel; a writer task
  stores them in `system_log_entries` in batches. Events are dropped (and the drop is
  counted and logged) rather than slowing requests when the channel is full. The
  `system-log-prune` job keeps 30 days by default. Log levels written to stdout still
  follow `RUST_LOG`.
- Capabilities: `system.manage` (Modules, settings, jobs) and `system.view_log`. Changes
  to Modules, settings and schedules are audited; records keyed by name get a
  deterministic UUID v5 as audit aggregate ID.

## Consequences

- Each new context adds its Capabilities, settings and Module route prefixes to the
  server's catalogues (`capability_catalogue`, `setting_catalogue`, `module_routes`).
- Messages logged at `WARN`/`ERROR` become visible to Administrators, so they must not
  contain secrets (they never should).
- `uuid` gains the `v5` feature.

### Deviations from FreeITSM

- Settings are typed and validated; unknown keys are rejected.
- The Tickets Module cannot be turned off.
