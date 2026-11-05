//! Domain events of the service desk (ADR-0005). The SLA clock and notifications react
//! to them.

use chrono::{DateTime, Utc};
use cooper_kernel::{AnalystId, DomainEvent, TeamId, TenantId, TicketId};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

macro_rules! ticket_event {
    ($(#[$meta:meta])* $name:ident, $type:literal { $($(#[$fmeta:meta])* $field:ident : $ty:ty),* $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
        pub struct $name {
            /// The ticket.
            pub ticket_id: TicketId,
            /// Its Company.
            pub tenant_id: TenantId,
            $($(#[$fmeta])* pub $field: $ty,)*
        }

        impl DomainEvent for $name {
            const EVENT_TYPE: &'static str = $type;
            const AGGREGATE_TYPE: &'static str = "ticket";
            fn aggregate_id(&self) -> Uuid {
                self.ticket_id.as_uuid()
            }
            fn tenant_id(&self) -> Option<TenantId> {
                Some(self.tenant_id)
            }
        }
    };
}

ticket_event!(
    /// A ticket was created.
    TicketCreated, "ticket.created" {
        /// Ticket Number.
        number: String,
        /// Priority.
        priority_id: Uuid,
        /// Status.
        status_id: Uuid,
        /// Creation time.
        created_at: DateTime<Utc>,
    }
);

ticket_event!(
    /// The Queue or Assignee changed.
    TicketAssigned, "ticket.assigned" {
        /// New Queue.
        team_id: Option<TeamId>,
        /// New Assignee.
        analyst_id: Option<AnalystId>,
        /// Previous Assignee.
        previous_analyst_id: Option<AnalystId>,
    }
);

ticket_event!(
    /// The status changed between open statuses.
    TicketStatusChanged, "ticket.status_changed" {
        /// Previous status.
        from_status_id: Uuid,
        /// New status.
        to_status_id: Uuid,
    }
);

ticket_event!(
    /// The ticket was closed.
    TicketClosed, "ticket.closed" {
        /// Closing status.
        status_id: Uuid,
        /// Resolution Code.
        resolution_code_id: Uuid,
        /// When.
        closed_at: DateTime<Utc>,
    }
);

ticket_event!(
    /// A closed ticket was reopened.
    TicketReopened, "ticket.reopened" {
        /// New status.
        status_id: Uuid,
    }
);

ticket_event!(
    /// The Priority changed (SLA targets are recalculated, FR-SLA-001).
    TicketPriorityChanged, "ticket.priority_changed" {
        /// Previous Priority.
        from_priority_id: Uuid,
        /// New Priority.
        to_priority_id: Uuid,
    }
);

ticket_event!(
    /// An SLA target reached its warning point (FR-SLA-005). Raised once per target.
    SlaWarningRaised, "ticket.sla_warning" {
        /// Response or resolution.
        target: crate::domain::SlaTarget,
        /// When the target is due.
        due_at: DateTime<Utc>,
    }
);

ticket_event!(
    /// An SLA target was missed (FR-SLA-005). Raised once per target.
    SlaBreached, "ticket.sla_breached" {
        /// Response or resolution.
        target: crate::domain::SlaTarget,
        /// When the target was due.
        due_at: DateTime<Utc>,
    }
);
