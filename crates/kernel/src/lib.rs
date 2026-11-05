//! Cooper shared kernel.
//!
//! The small set of types every bounded context shares: typed identifiers, the
//! [`TenantId`] of a Company, the [`Actor`] who performed an action, the [`Clock`]
//! abstraction, the [`DomainEvent`] contract, cursor [`pagination`] and
//! [`AuditRecord`]s, and the authenticated [`Principal`] with its [`Capability`]s.
//!
//! The kernel contains **no business rules** and performs **no I/O**. It must stay
//! usable from every context's `domain` module (AGENTS.md §3).

pub mod actor;
pub mod audit;
pub mod clock;
pub mod event;
pub mod id;
pub mod pagination;
pub mod principal;
pub mod sensitivity;
pub mod tenancy;

pub use actor::{Actor, ActorKind, ParseActorError};
pub use audit::{AuditAction, AuditChange, AuditRecord};
pub use clock::{Clock, FixedClock, SystemClock};
pub use event::{DomainEvent, EventEnvelope, EventId};
pub use id::{AnalystId, ApiKeyId, DepartmentId, ParseIdError, RequesterId, TeamId, TenantId};
pub use pagination::{Cursor, CursorError, Limit, LimitError, Page};
pub use principal::{Capability, Forbidden, ParseCapabilityError, Principal};
pub use sensitivity::{ParseSensitivityError, Sensitivity};
pub use tenancy::TenantScope;
