//! SLA configuration and the SLA clock of tickets (FR-SLA-001 to FR-SLA-007).

use cooper_kernel::{AuditAction, Principal};
use uuid::Uuid;

use super::{AppError, ServiceDesk, capabilities, require};
use crate::domain::{
    LookupKind, SlaCalendar, SlaCalendarInput, SlaNotificationRule, SlaRuleInput, SlaTargets,
    Ticket, TicketError, WorkingTime, audit_record,
};

impl ServiceDesk {
    /// Every SLA Calendar.
    ///
    /// # Errors
    ///
    /// Forbidden or storage failures.
    pub async fn sla_calendars(&self, principal: &Principal) -> Result<Vec<SlaCalendar>, AppError> {
        require(principal, capabilities::READ)?;
        Ok(self.ports.sla.calendars().await?)
    }

    /// Creates an SLA Calendar.
    ///
    /// # Errors
    ///
    /// Forbidden, invalid hours or a duplicate name.
    pub async fn create_sla_calendar(
        &self,
        principal: &Principal,
        input: SlaCalendarInput,
    ) -> Result<SlaCalendar, AppError> {
        require(principal, capabilities::CONFIGURE)?;
        let now = self.ports.clock.now();
        let calendar = SlaCalendar::create(input, now)?;
        let audit = audit_record(
            None,
            &calendar,
            AuditAction::Created,
            principal.actor(),
            now,
        );
        self.ports.sla.save_calendar(&calendar, &audit).await?;
        Ok(calendar)
    }

    /// Changes an SLA Calendar. Open tickets keep the due times already computed.
    ///
    /// # Errors
    ///
    /// Forbidden, not found, invalid hours or a duplicate name.
    pub async fn update_sla_calendar(
        &self,
        principal: &Principal,
        id: Uuid,
        input: SlaCalendarInput,
    ) -> Result<SlaCalendar, AppError> {
        require(principal, capabilities::CONFIGURE)?;
        let mut calendar = self
            .calendar(Some(id))
            .await?
            .ok_or(AppError::NotFound("SLA Calendar"))?;
        let before = calendar.clone();
        let now = self.ports.clock.now();
        calendar.update(input, now)?;
        let audit = audit_record(
            Some(&before),
            &calendar,
            AuditAction::Updated,
            principal.actor(),
            now,
        );
        if audit.is_meaningful() {
            self.ports.sla.save_calendar(&calendar, &audit).await?;
        }
        Ok(calendar)
    }

    /// Every SLA Notification Rule.
    ///
    /// # Errors
    ///
    /// Forbidden or storage failures.
    pub async fn sla_rules(
        &self,
        principal: &Principal,
    ) -> Result<Vec<SlaNotificationRule>, AppError> {
        require(principal, capabilities::CONFIGURE)?;
        Ok(self.ports.sla.rules().await?)
    }

    /// Creates an SLA Notification Rule.
    ///
    /// # Errors
    ///
    /// Forbidden, invalid rule or a duplicate name.
    pub async fn create_sla_rule(
        &self,
        principal: &Principal,
        input: SlaRuleInput,
    ) -> Result<SlaNotificationRule, AppError> {
        require(principal, capabilities::CONFIGURE)?;
        let now = self.ports.clock.now();
        let rule = SlaNotificationRule::create(input, now)?;
        let audit = audit_record(None, &rule, AuditAction::Created, principal.actor(), now);
        self.ports.sla.save_rule(&rule, &audit).await?;
        Ok(rule)
    }

    /// Changes an SLA Notification Rule.
    ///
    /// # Errors
    ///
    /// Forbidden, not found or invalid rule.
    pub async fn update_sla_rule(
        &self,
        principal: &Principal,
        id: Uuid,
        input: SlaRuleInput,
    ) -> Result<SlaNotificationRule, AppError> {
        require(principal, capabilities::CONFIGURE)?;
        let mut rule = self
            .ports
            .sla
            .rules()
            .await?
            .into_iter()
            .find(|r| r.id() == id)
            .ok_or(AppError::NotFound("SLA Notification Rule"))?;
        let before = rule.clone();
        let now = self.ports.clock.now();
        rule.update(input, now)?;
        let audit = audit_record(
            Some(&before),
            &rule,
            AuditAction::Updated,
            principal.actor(),
            now,
        );
        if audit.is_meaningful() {
            self.ports.sla.save_rule(&rule, &audit).await?;
        }
        Ok(rule)
    }

    /// Analysts to notify for an SLA trigger on a target, per the active SLA Notification
    /// Rules (FR-SLA-006).
    ///
    /// # Errors
    ///
    /// Storage failures.
    pub async fn sla_recipients(
        &self,
        trigger: crate::domain::SlaTrigger,
        target: crate::domain::SlaTarget,
        assignee: Option<cooper_kernel::AnalystId>,
        team_members: &std::collections::BTreeSet<cooper_kernel::AnalystId>,
    ) -> Result<std::collections::BTreeSet<cooper_kernel::AnalystId>, AppError> {
        Ok(self
            .ports
            .sla
            .rules()
            .await?
            .iter()
            .flat_map(|r| r.recipients(trigger, target, assignee, team_members))
            .collect())
    }

    /// A calendar by ID, or the default calendar for `None`.
    pub(super) async fn calendar(&self, id: Option<Uuid>) -> Result<Option<SlaCalendar>, AppError> {
        let calendars = self.ports.sla.calendars().await?;
        Ok(match id {
            Some(id) => calendars.into_iter().find(|c| c.id() == id),
            None => calendars.into_iter().find(SlaCalendar::is_default),
        })
    }

    /// The SLA targets of a Priority and the calendar they count against.
    pub(super) async fn sla_targets(
        &self,
        priority_id: Uuid,
    ) -> Result<(SlaTargets, Option<SlaCalendar>), AppError> {
        let priority = self.lookup(LookupKind::Priority, priority_id).await?;
        let flags = priority.flags();
        let calendar = self.calendar(flags.sla_calendar_id).await?;
        if flags.sla_calendar_id.is_some() && calendar.is_none() {
            return Err(TicketError::invalid(
                "sla_calendar_id",
                "the Priority's SLA Calendar does not exist",
            )
            .into());
        }
        Ok((
            SlaTargets {
                calendar_id: calendar.as_ref().map(SlaCalendar::id),
                response_minutes: flags.response_minutes,
                resolution_minutes: flags.resolution_minutes,
            },
            calendar,
        ))
    }

    /// (Re)starts the SLA clock of a ticket from its Priority.
    pub(super) async fn start_sla(&self, ticket: &mut Ticket) -> Result<(), AppError> {
        let (targets, calendar) = self.sla_targets(ticket.details().priority_id).await?;
        let percent = self.ports.settings.settings().await.sla_warning_percent;
        ticket.apply_sla(targets, WorkingTime(calendar.as_ref()), percent);
        Ok(())
    }

    /// Pauses or resumes the SLA clock for a ticket entering a status.
    pub(super) async fn follow_status(
        &self,
        ticket: &mut Ticket,
        pauses: bool,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), AppError> {
        if pauses {
            ticket.pause_sla(now);
        } else if ticket.sla().paused_at.is_some() {
            let calendar = self.calendar(ticket.sla().targets.calendar_id).await?;
            let percent = self.ports.settings.settings().await.sla_warning_percent;
            ticket.resume_sla(WorkingTime(calendar.as_ref()), percent, now);
        }
        Ok(())
    }
}
