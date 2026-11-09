//! Ticket statistics for reports (FR-RPT-001, FR-RPT-002, FR-RPT-007, ADR-0032).
//!
//! The Reporting context composes these figures; the counting stays here, next to the
//! tickets, and applies the caller's Companies and the Confidential rule (FR-TKT-014)
//! exactly as ticket lists do.

use chrono::NaiveDate;
use chrono_tz::Tz;
use cooper_kernel::{Principal, TenantScope};

use super::{AppError, ServiceDesk, capabilities, require};
use crate::domain::{
    SlaTally, StatisticsFilter, TicketGrouping, TicketTally, TimeGrouping, TimeTally,
};

/// The caller's scope, narrowed to one Company when the filter names one.
fn narrowed(principal: &Principal, filter: &StatisticsFilter) -> Result<TenantScope, AppError> {
    match filter.tenant {
        None => Ok(principal.scope().clone()),
        Some(t) if principal.scope().allows(t) => Ok(TenantScope::single(t)),
        Some(_) => Err(AppError::NotFound("Company")),
    }
}

impl ServiceDesk {
    /// Tickets the caller may see, counted per group (needs `tickets.read`). Date groups
    /// are calendar days or months in `tz`.
    ///
    /// # Errors
    ///
    /// Forbidden, or a Company outside the caller's scope.
    pub async fn ticket_tally(
        &self,
        principal: &Principal,
        group: TicketGrouping,
        filter: &StatisticsFilter,
        tz: Tz,
    ) -> Result<Vec<TicketTally>, AppError> {
        require(principal, capabilities::READ)?;
        let scope = narrowed(principal, filter)?;
        let visibility = self.visibility(principal, false).await?;
        Ok(self
            .ports
            .statistics
            .tally(&scope, &visibility, group, filter, tz)
            .await?)
    }

    /// SLA results of the tickets the caller may see, per group (needs `tickets.read`).
    /// Each ticket counts in the state its own page shows now (FR-SLA-007).
    ///
    /// # Errors
    ///
    /// Forbidden, or a Company outside the caller's scope.
    pub async fn sla_tally(
        &self,
        principal: &Principal,
        group: TicketGrouping,
        filter: &StatisticsFilter,
        tz: Tz,
    ) -> Result<Vec<SlaTally>, AppError> {
        require(principal, capabilities::READ)?;
        let scope = narrowed(principal, filter)?;
        let visibility = self.visibility(principal, false).await?;
        let now = self.ports.clock.now();
        Ok(self
            .ports
            .statistics
            .sla_tally(&scope, &visibility, group, filter, tz, now)
            .await?)
    }

    /// Time Entries on tickets the caller may see, with a work date in `from..=to`, per
    /// group (needs `tickets.read`).
    ///
    /// # Errors
    ///
    /// Forbidden, or a Company outside the caller's scope.
    pub async fn time_tally(
        &self,
        principal: &Principal,
        group: TimeGrouping,
        filter: &StatisticsFilter,
        from: NaiveDate,
        to: NaiveDate,
    ) -> Result<Vec<TimeTally>, AppError> {
        require(principal, capabilities::READ)?;
        let scope = narrowed(principal, filter)?;
        let visibility = self.visibility(principal, false).await?;
        Ok(self
            .ports
            .statistics
            .time_tally(&scope, &visibility, group, from, to)
            .await?)
    }
}
