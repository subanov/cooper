//! Email intake (FR-EML-003 to FR-EML-005, FR-EML-011) and Mailbox administration
//! (FR-EML-001, FR-EML-010).

use cooper_kernel::{
    Actor, AuditAction, AuditRecord, EventEnvelope, Principal, Sensitivity, TicketId,
};
use uuid::Uuid;

use super::{AppError, CreateTicket, ServiceDesk, capabilities, require};
use crate::domain::{
    Direction, InboundEmail, LookupKind, Mailbox, MailboxPollFailed, MailboxSettings,
    TicketDetails, TicketEmail, TicketEmailReceived, audit_record, parse_sdref,
};

/// What happened to an inbound email.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntakeOutcome {
    /// A new ticket was created.
    Created(TicketId),
    /// The email was added to an existing ticket.
    Appended(TicketId),
    /// Rejected by the Whitelist or an unknown-sender policy (and logged).
    Rejected(String),
    /// Automated mail (loop protection): logged, never a ticket.
    Ignored(String),
    /// Already imported (same `Message-ID`).
    Duplicate,
}

impl ServiceDesk {
    // ----- Administration ----------------------------------------------------------

    /// Every Mailbox with its health.
    ///
    /// # Errors
    ///
    /// Forbidden or storage failures.
    pub async fn mailboxes(&self, principal: &Principal) -> Result<Vec<Mailbox>, AppError> {
        require(principal, capabilities::CONFIGURE)?;
        Ok(self.ports.mailboxes.mailboxes().await?)
    }

    /// Creates a Mailbox. `secret` is the password (IMAP) or credentials (OAuth),
    /// encrypted at rest (FR-EML-012).
    ///
    /// # Errors
    ///
    /// Forbidden, invalid settings or a duplicate address.
    pub async fn create_mailbox(
        &self,
        principal: &Principal,
        settings: MailboxSettings,
        secret: Option<&str>,
    ) -> Result<Mailbox, AppError> {
        require(principal, capabilities::CONFIGURE)?;
        let now = self.ports.clock.now();
        let mailbox = Mailbox::create(settings, now)?;
        let audit = audit_record(None, &mailbox, AuditAction::Created, principal.actor(), now);
        self.ports.mailboxes.save(&mailbox, secret, &audit).await?;
        Ok(mailbox)
    }

    /// Changes a Mailbox; the secret is kept unless a new one is given.
    ///
    /// # Errors
    ///
    /// Forbidden, not found or invalid settings.
    pub async fn update_mailbox(
        &self,
        principal: &Principal,
        id: Uuid,
        settings: MailboxSettings,
        secret: Option<&str>,
    ) -> Result<Mailbox, AppError> {
        require(principal, capabilities::CONFIGURE)?;
        let mut mailbox = self.mailbox(id).await?;
        let before = mailbox.clone();
        let now = self.ports.clock.now();
        mailbox.update(settings, now)?;
        let mut audit = audit_record(
            Some(&before),
            &mailbox,
            AuditAction::Updated,
            principal.actor(),
            now,
        );
        if secret.is_some() {
            audit = audit.change("secret", Some(&"(unchanged)"), Some(&"(replaced)"));
        }
        if audit.is_meaningful() {
            self.ports.mailboxes.save(&mailbox, secret, &audit).await?;
        }
        Ok(mailbox)
    }

    /// Recent Rejected Emails of a Mailbox.
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn rejected_emails(
        &self,
        principal: &Principal,
        id: Uuid,
    ) -> Result<Vec<(String, String, String, chrono::DateTime<chrono::Utc>)>, AppError> {
        require(principal, capabilities::CONFIGURE)?;
        self.mailbox(id).await?;
        Ok(self.ports.mailboxes.rejected(id, 100).await?)
    }

    async fn mailbox(&self, id: Uuid) -> Result<Mailbox, AppError> {
        self.ports
            .mailboxes
            .mailboxes()
            .await?
            .into_iter()
            .find(|m| m.id() == id)
            .ok_or(AppError::NotFound("Mailbox"))
    }

    /// Records a poll result (FR-EML-010). The third failure in a row raises
    /// `mailbox.poll_failed`, which notifies Administrators.
    ///
    /// # Errors
    ///
    /// Storage failures.
    pub async fn record_poll(
        &self,
        mailbox: &mut Mailbox,
        error: Option<&str>,
    ) -> Result<(), AppError> {
        let now = self.ports.clock.now();
        let mut events = Vec::new();
        match error {
            None => mailbox.poll_succeeded(now),
            Some(e) => {
                tracing::warn!(mailbox = %mailbox.settings().address, error = %e, "mailbox poll failed");
                if mailbox.poll_failed(e, now) {
                    let event = MailboxPollFailed {
                        mailbox_id: mailbox.id(),
                        name: mailbox.settings().name.clone(),
                        error: e.chars().take(500).collect(),
                        failures: mailbox.health().consecutive_failures,
                    };
                    events.push(
                        EventEnvelope::wrap(&event, Actor::Automation, now)
                            .map_err(|_| AppError::Internal)?,
                    );
                }
            }
        }
        Ok(self.ports.mailboxes.record_health(mailbox, &events).await?)
    }

    /// The decrypted secret of a Mailbox, for its client.
    ///
    /// # Errors
    ///
    /// Storage or decryption failures.
    pub async fn mailbox_secret(&self, id: Uuid) -> Result<Option<String>, AppError> {
        Ok(self.ports.mailboxes.secret(id).await?)
    }

    // ----- Intake ------------------------------------------------------------------------

    /// Imports one inbound email from `mailbox`: loop protection, duplicates, Whitelist,
    /// threading by SDREF and `In-Reply-To`/`References`, Requester and Company matching,
    /// then append to the ticket (reopening it when closed) or create a new one.
    ///
    /// # Errors
    ///
    /// Storage failures. Business outcomes are in [`IntakeOutcome`].
    pub async fn import_email(
        &self,
        mailbox: &Mailbox,
        email: InboundEmail,
    ) -> Result<IntakeOutcome, AppError> {
        let now = self.ports.clock.now();
        let settings = self.ports.settings.settings().await;
        let own = vec![
            mailbox.settings().address.clone(),
            address_of(&settings.from_address),
        ];
        if let Some(reason) = email.automated_reason(&own) {
            self.ports
                .mailboxes
                .log_rejected(mailbox.id(), &email, reason, now)
                .await?;
            return Ok(IntakeOutcome::Ignored(reason.to_owned()));
        }
        if self.ports.content.inbound_exists(&email.message_id).await? {
            return Ok(IntakeOutcome::Duplicate);
        }
        if !mailbox.admits(&email.from_address) {
            self.ports
                .mailboxes
                .log_rejected(
                    mailbox.id(),
                    &email,
                    "not allowed by the Mailbox Whitelist",
                    now,
                )
                .await?;
            return Ok(IntakeOutcome::Rejected(
                "not allowed by the Mailbox Whitelist".into(),
            ));
        }
        let system = Principal::automation();

        // Threading: SDREF first, then the email headers.
        let mut ticket = None;
        if let Some(number) = parse_sdref(&email.subject)
            && let Ok(t) = self.ticket_by_number(&system, &number).await
        {
            ticket = Some(t);
        }
        if ticket.is_none() {
            let mut ids: Vec<String> = email.references.clone();
            ids.extend(email.in_reply_to.clone());
            if !ids.is_empty()
                && let Some(id) = self.ports.content.ticket_for_messages(&ids).await?
            {
                ticket = self.ticket(&system, id).await.ok().map(|d| d.ticket);
            }
        }

        // Requester and Company (FR-EML-004).
        let known = self
            .ports
            .directory
            .requesters_by_email(&email.from_address)
            .await?;
        let domain = email
            .from_address
            .rsplit('@')
            .next()
            .unwrap_or_default()
            .to_owned();
        let company = match &ticket {
            Some(t) => t.tenant_id(),
            None => match self.ports.directory.company_for_domain(&domain).await? {
                Some(c) => c,
                None => known
                    .iter()
                    .find(|(_, _, active)| *active)
                    .map_or(mailbox.settings().tenant_id, |(_, t, _)| *t),
            },
        };
        let requester = match known.iter().find(|(_, t, _)| *t == company) {
            Some((id, _, _)) => *id,
            None if mailbox.settings().create_unknown_requesters => {
                let name = email
                    .from_name
                    .clone()
                    .filter(|n| !n.trim().is_empty())
                    .unwrap_or_else(|| {
                        email
                            .from_address
                            .split('@')
                            .next()
                            .unwrap_or("Requester")
                            .to_owned()
                    });
                self.ports
                    .directory
                    .create_requester(company, &name, &email.from_address)
                    .await?
            }
            None => {
                self.ports
                    .mailboxes
                    .log_rejected(mailbox.id(), &email, "unknown sender", now)
                    .await?;
                return Ok(IntakeOutcome::Rejected("unknown sender".into()));
            }
        };

        let (ticket, created) = match ticket {
            Some(t) => (t, false),
            None => {
                let origin = match mailbox.settings().origin_id {
                    Some(o) => Some(o),
                    None => self
                        .ports
                        .reference
                        .lookups(LookupKind::Origin)
                        .await?
                        .into_iter()
                        .find(|o| o.key() == Some("email"))
                        .map(|o| o.id()),
                };
                let details = TicketDetails {
                    subject: if email.subject.trim().is_empty() {
                        "(no subject)".into()
                    } else {
                        email.subject.chars().take(500).collect()
                    },
                    description: email.text_body.clone(),
                    requester_id: Some(requester),
                    department_id: mailbox.settings().department_id,
                    origin_id: origin,
                    sensitivity: Sensitivity::Normal,
                    ..TicketDetails::default()
                };
                let t = self
                    .create_ticket(
                        &system,
                        CreateTicket {
                            tenant_id: company,
                            details,
                            sensitivity_given: false,
                            team_id: mailbox.settings().team_id,
                            analyst_id: None,
                        },
                    )
                    .await?;
                (t, true)
            }
        };

        let record = TicketEmail {
            id: Uuid::now_v7(),
            tenant_id: ticket.tenant_id(),
            ticket_id: ticket.id(),
            direction: Direction::Inbound,
            from: email.from_address.clone(),
            to: email.to.clone(),
            cc: email.cc.clone(),
            subject: email.subject.clone(),
            text_body: email.text_body.clone(),
            message_id: email.message_id.clone(),
            author_id: None,
            occurred_at: now,
            mailbox_id: Some(mailbox.id()),
            requester_id: Some(requester),
            in_reply_to: email.in_reply_to.clone(),
        };
        let mut reopened = false;
        if !created && ticket.closed_at().is_some() {
            let open = self.default_lookup(LookupKind::Status).await?;
            self.change_status(&system, ticket.id(), open.id(), None, None)
                .await?;
            reopened = true;
        }
        let mut events = Vec::new();
        if !created {
            let e = TicketEmailReceived {
                ticket_id: ticket.id(),
                tenant_id: ticket.tenant_id(),
                email_id: record.id,
                from: record.from.clone(),
                reopened,
            };
            events.push(
                EventEnvelope::wrap(&e, Actor::Automation, now).map_err(|_| AppError::Internal)?,
            );
        }
        let audit = AuditRecord::new(
            Some(record.tenant_id),
            "ticket_email",
            record.id,
            AuditAction::Created,
            Actor::Automation,
            now,
        )
        .change("ticket_id", None, Some(&record.ticket_id))
        .change("from", None, Some(&record.from));
        self.ports
            .content
            .add_email(&record, &audit, &events)
            .await?;
        tracing::info!(
            ticket = ticket.number(),
            created,
            reopened,
            "email imported"
        );
        Ok(if created {
            IntakeOutcome::Created(ticket.id())
        } else {
            IntakeOutcome::Appended(ticket.id())
        })
    }
}

/// The address part of `Name <addr>`.
fn address_of(from: &str) -> String {
    from.rsplit('<')
        .next()
        .unwrap_or(from)
        .trim_end_matches('>')
        .trim()
        .to_lowercase()
}
