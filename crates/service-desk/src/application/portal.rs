//! Portal use cases for Requesters: the Portal Catalogue and raising a ticket from it
//! (FR-PRT-002); my tickets, the public timeline, replies and attachments (FR-PRT-003). The Requester and Company always come from the principal, never
//! from the request.

use std::collections::{BTreeSet, HashMap};

use chrono::{DateTime, Utc};
use cooper_kernel::{
    Actor, AuditAction, Cursor, EventEnvelope, Limit, Page, Principal, RequesterId, Sensitivity,
    TenantId, TenantScope, TicketId,
};
use uuid::Uuid;

use super::tickets::sort_value;
use super::{AppError, CreateTicket, ServiceDesk, StoredFile};
use crate::domain::{
    AttachmentInfo, Category, Direction, ListFilter, LookupKind, Note, PortalReplyDraft, SortField,
    Ticket, TicketDetails, TicketEmail, TicketEmailReceived, TicketError, TicketListQuery,
    Visibility, audit_record, portal_catalogue, portal_ticket_type,
};

/// A ticket raised on the Portal.
#[derive(Debug, Clone)]
pub struct PortalTicketInput {
    /// The chosen Portal Catalogue entry. Required whenever the Catalogue is not empty.
    pub category_id: Option<Uuid>,
    /// Subject.
    pub subject: String,
    /// Description.
    pub description: String,
    /// Values of Ticket Fields asked of the Requester, by key (FR-TKT-029).
    pub fields: std::collections::BTreeMap<String, serde_json::Value>,
}

/// The Requester and Company of a Portal principal.
pub(super) fn portal_requester(principal: &Principal) -> Result<(RequesterId, TenantId), AppError> {
    let Actor::Requester(requester) = principal.actor() else {
        return Err(cooper_kernel::Forbidden("requester".into()).into());
    };
    match principal.scope() {
        TenantScope::Tenants(ids) if ids.len() == 1 => ids
            .iter()
            .next()
            .map(|tenant| (requester, *tenant))
            .ok_or_else(|| cooper_kernel::Forbidden("requester".into()).into()),
        _ => Err(cooper_kernel::Forbidden("requester".into()).into()),
    }
}

impl ServiceDesk {
    /// The Portal Catalogue for a signed-in Requester (FR-PRT-002), in tree order.
    ///
    /// # Errors
    ///
    /// Not a Portal principal, or storage failures.
    pub async fn portal_catalogue(&self, principal: &Principal) -> Result<Vec<Category>, AppError> {
        portal_requester(principal)?;
        let categories = self.ports.reference.categories().await?;
        Ok(portal_catalogue(&categories).into_iter().cloned().collect())
    }

    /// Raises a ticket for the signed-in Requester from the Portal Catalogue
    /// (FR-PRT-002). The ticket gets the Portal Origin, the Category's Ticket Type (when
    /// it is limited to Types) and the default Priority; nobody is assigned.
    ///
    /// # Errors
    ///
    /// Not a Portal principal; a Category outside the Catalogue, or none while the
    /// Catalogue has entries; invalid subject.
    pub async fn raise_portal_ticket(
        &self,
        principal: &Principal,
        input: PortalTicketInput,
    ) -> Result<Ticket, AppError> {
        let (requester, tenant) = portal_requester(principal)?;
        let categories = self.ports.reference.categories().await?;
        let catalogue = portal_catalogue(&categories);
        let category = match input.category_id {
            Some(id) => Some(*catalogue.iter().find(|c| c.id() == id).ok_or_else(|| {
                TicketError::invalid("category_id", "is not in the Portal Catalogue")
            })?),
            None if catalogue.is_empty() => None,
            None => {
                return Err(TicketError::invalid(
                    "category_id",
                    "choose an entry from the Portal Catalogue",
                )
                .into());
            }
        };
        let type_id = match category {
            Some(c) if !c.type_ids().is_empty() => {
                let types = self.ports.reference.lookups(LookupKind::Type).await?;
                Some(portal_ticket_type(c, &types).ok_or_else(|| {
                    TicketError::invalid("category_id", "has no active Ticket Type")
                })?)
            }
            _ => None,
        };
        let origin_id = self
            .ports
            .reference
            .lookups(LookupKind::Origin)
            .await?
            .into_iter()
            .find(|o| o.key() == Some("portal") && o.is_active())
            .map(|o| o.id());
        let ticket = self
            .open_ticket(
                principal,
                CreateTicket {
                    tenant_id: tenant,
                    details: TicketDetails {
                        subject: input.subject,
                        description: input.description,
                        requester_id: Some(requester),
                        category_id: category.map(Category::id),
                        type_id,
                        origin_id,
                        sensitivity: Sensitivity::Normal,
                        ..TicketDetails::default()
                    },
                    sensitivity_given: false,
                    team_id: None,
                    analyst_id: None,
                    channel: crate::domain::Channel::Portal,
                    mailbox_id: None,
                    fields: input.fields,
                },
            )
            .await?;
        tracing::info!(ticket = ticket.number(), "ticket raised on the Portal");
        Ok(ticket)
    }
}

/// A ticket on the Requester's "my tickets" list.
#[derive(Debug, Clone)]
pub struct PortalTicket {
    /// The ticket.
    pub ticket: Ticket,
    /// Name of its Status.
    pub status_name: String,
}

/// One public item on a ticket's Portal timeline (FR-PRT-003): never internal notes,
/// time or field history.
#[derive(Debug, Clone)]
pub enum PortalTimelineEntry {
    /// A public Note from the service desk.
    Note {
        /// Text.
        body: String,
        /// When.
        at: DateTime<Utc>,
    },
    /// A message: an emailed or Portal reply.
    Message {
        /// Whether the Requester side wrote it (inbound), rather than the service desk.
        from_requester: bool,
        /// Text.
        body: String,
        /// When.
        at: DateTime<Utc>,
    },
}

impl PortalTimelineEntry {
    /// When it happened.
    #[must_use]
    pub const fn at(&self) -> DateTime<Utc> {
        match self {
            Self::Note { at, .. } | Self::Message { at, .. } => *at,
        }
    }
}

/// What a Requester may see: their own tickets, Confidential ones included (they are
/// about them), never deleted ones.
fn requester_visibility() -> Visibility {
    Visibility {
        all_confidential: true,
        confidential_departments: BTreeSet::new(),
        include_deleted: false,
    }
}

impl ServiceDesk {
    /// Loads a ticket the signed-in Requester may read: their own, or one of a Requester
    /// they cover as a Manager (FR-PRT-004). Anything else is "not found".
    async fn readable_ticket(
        &self,
        principal: &Principal,
        id: TicketId,
    ) -> Result<Ticket, AppError> {
        if let Ok(t) = self.own_ticket(principal, id).await {
            return Ok(t);
        }
        let (_, tenant) = portal_requester(principal)?;
        let managed = self.ports.directory.managed_scope(principal).await?;
        self.ports
            .tickets
            .get(principal.scope(), &requester_visibility(), id)
            .await?
            .filter(|t| {
                t.tenant_id() == tenant
                    && t.details()
                        .requester_id
                        .is_some_and(|r| managed.requester_ids.contains(&r))
                    && (managed.include_confidential
                        || t.details().sensitivity == Sensitivity::Normal)
            })
            .ok_or(AppError::NotFound("Ticket"))
    }

    /// Tickets of the Requesters the signed-in Requester covers as a Manager, newest
    /// first (FR-PRT-004). Empty unless Managers are switched on. Read-only: replies and
    /// attachments stay with each Requester's own tickets.
    ///
    /// # Errors
    ///
    /// Not a Portal principal or an invalid cursor.
    pub async fn portal_managed_tickets(
        &self,
        principal: &Principal,
        open: Option<bool>,
        limit: Limit,
        cursor: Option<&Cursor>,
    ) -> Result<Page<PortalTicket>, AppError> {
        let (_, tenant) = portal_requester(principal)?;
        let managed = self.ports.directory.managed_scope(principal).await?;
        if managed.requester_ids.is_empty() {
            return Ok(Page {
                items: Vec::new(),
                next_cursor: None,
            });
        }
        let mut filter = ListFilter {
            requester_ids: managed.requester_ids,
            open,
            sensitivity: (!managed.include_confidential).then_some(Sensitivity::Normal),
            ..ListFilter::default()
        };
        filter.tenant_ids.insert(tenant);
        self.portal_page(principal, filter, limit, cursor).await
    }

    /// Loads a ticket of the signed-in Requester; any other ticket is "not found".
    async fn own_ticket(&self, principal: &Principal, id: TicketId) -> Result<Ticket, AppError> {
        let (requester, tenant) = portal_requester(principal)?;
        self.ports
            .tickets
            .get(principal.scope(), &requester_visibility(), id)
            .await?
            .filter(|t| t.tenant_id() == tenant && t.details().requester_id == Some(requester))
            .ok_or(AppError::NotFound("Ticket"))
    }

    async fn status_names(&self) -> Result<HashMap<Uuid, String>, AppError> {
        Ok(self
            .ports
            .reference
            .lookups(LookupKind::Status)
            .await?
            .into_iter()
            .map(|l| (l.id(), l.name().to_owned()))
            .collect())
    }

    /// The signed-in Requester's tickets, newest first (FR-PRT-003). `open` limits the
    /// list to open (`true`) or closed (`false`) tickets.
    ///
    /// # Errors
    ///
    /// Not a Portal principal or an invalid cursor.
    pub async fn portal_tickets(
        &self,
        principal: &Principal,
        open: Option<bool>,
        limit: Limit,
        cursor: Option<&Cursor>,
    ) -> Result<Page<PortalTicket>, AppError> {
        let (requester, tenant) = portal_requester(principal)?;
        let mut filter = ListFilter {
            requester_id: Some(requester),
            open,
            ..ListFilter::default()
        };
        filter.tenant_ids.insert(tenant);
        self.portal_page(principal, filter, limit, cursor).await
    }

    async fn portal_page(
        &self,
        principal: &Principal,
        filter: ListFilter,
        limit: Limit,
        cursor: Option<&Cursor>,
    ) -> Result<Page<PortalTicket>, AppError> {
        let query = TicketListQuery {
            filter,
            sort: SortField::CreatedAt,
            descending: true,
            after: cursor.map(Cursor::decode::<(String, Uuid)>).transpose()?,
            limit,
            now: Some(self.ports.clock.now()),
        };
        let rows = self
            .ports
            .tickets
            .list(principal.scope(), &requester_visibility(), &query)
            .await?;
        let page = Page::from_rows(rows, limit, |t| {
            Cursor::encode(&(sort_value(t, SortField::CreatedAt), t.id().as_uuid()))
        })?;
        let names = self.status_names().await?;
        Ok(Page {
            items: page
                .items
                .into_iter()
                .map(|ticket| PortalTicket {
                    status_name: names.get(&ticket.status_id()).cloned().unwrap_or_default(),
                    ticket,
                })
                .collect(),
            next_cursor: page.next_cursor,
        })
    }

    /// One of the signed-in Requester's tickets.
    ///
    /// # Errors
    ///
    /// Not a Portal principal, or not the Requester's ticket.
    pub async fn portal_ticket(
        &self,
        principal: &Principal,
        id: TicketId,
    ) -> Result<PortalTicket, AppError> {
        let ticket = self.readable_ticket(principal, id).await?;
        let status_name = self
            .status_names()
            .await?
            .remove(&ticket.status_id())
            .unwrap_or_default();
        Ok(PortalTicket {
            ticket,
            status_name,
        })
    }

    /// The public timeline of one of the Requester's tickets, oldest first: public
    /// Notes and messages (FR-PRT-003).
    ///
    /// # Errors
    ///
    /// Not a Portal principal, or not the Requester's ticket.
    pub async fn portal_timeline(
        &self,
        principal: &Principal,
        id: TicketId,
    ) -> Result<Vec<PortalTimelineEntry>, AppError> {
        self.readable_ticket(principal, id).await?;
        let scope = principal.scope();
        let mut entries: Vec<PortalTimelineEntry> = self
            .ports
            .tickets
            .notes(scope, id)
            .await?
            .into_iter()
            .filter(Note::is_public)
            .map(|n| PortalTimelineEntry::Note {
                body: n.body().to_owned(),
                at: n.created_at(),
            })
            .collect();
        entries.extend(
            self.ports
                .content
                .emails(scope, id)
                .await?
                .into_iter()
                .map(|e| PortalTimelineEntry::Message {
                    from_requester: e.direction == Direction::Inbound,
                    body: e.text_body,
                    at: e.occurred_at,
                }),
        );
        entries.sort_by_key(PortalTimelineEntry::at);
        Ok(entries)
    }

    /// The Requester replies on the Portal (FR-PRT-003). The reply is recorded like an
    /// emailed reply: it notifies the Assignee and reopens a closed ticket.
    ///
    /// # Errors
    ///
    /// Not a Portal principal, not the Requester's ticket, or an empty body.
    pub async fn portal_reply(
        &self,
        principal: &Principal,
        id: TicketId,
        body: &str,
    ) -> Result<TicketEmail, AppError> {
        let (requester, _) = portal_requester(principal)?;
        let ticket = self.own_ticket(principal, id).await?;
        let contact = self
            .ports
            .directory
            .requester_contact(principal.scope(), requester)
            .await?
            .ok_or(AppError::NotFound("Requester"))?;
        let previous = self.ports.content.emails(principal.scope(), id).await?;
        let settings = self.ports.settings.settings().await;
        let now = self.ports.clock.now();
        let email = TicketEmail::portal_reply(
            &ticket,
            PortalReplyDraft {
                requester_id: requester,
                from: contact.email,
                body: body.to_owned(),
                service_desk: settings.from_address,
                in_reply_to: previous.last().map(|e| e.message_id.clone()),
            },
            now,
        )?;
        let reopened = if ticket.closed_at().is_some() {
            let open = self.default_lookup(LookupKind::Status).await?;
            self.change_status(&Principal::automation(), id, open.id(), None, None)
                .await?;
            true
        } else {
            false
        };
        let event = EventEnvelope::wrap(
            &TicketEmailReceived {
                ticket_id: id,
                tenant_id: ticket.tenant_id(),
                email_id: email.id,
                from: email.from.clone(),
                reopened,
            },
            principal.actor(),
            now,
        )
        .map_err(|_| AppError::Internal)?;
        let audit = audit_record(None, &email, AuditAction::Created, principal.actor(), now);
        self.ports
            .content
            .add_email(&email, &audit, &[event])
            .await?;
        tracing::info!(ticket = ticket.number(), reopened, "Portal reply received");
        Ok(email)
    }

    /// The largest attachment allowed, after checking the ticket is the Requester's.
    ///
    /// # Errors
    ///
    /// Not a Portal principal, or not the Requester's ticket.
    pub async fn portal_attachment_limit(
        &self,
        principal: &Principal,
        id: TicketId,
    ) -> Result<u64, AppError> {
        self.own_ticket(principal, id).await?;
        Ok(self.ports.settings.settings().await.max_attachment_bytes)
    }

    /// Attaches a file the Requester uploaded on the Portal.
    ///
    /// # Errors
    ///
    /// Not a Portal principal, or not the Requester's ticket.
    pub async fn portal_attach(
        &self,
        principal: &Principal,
        id: TicketId,
        file: StoredFile,
    ) -> Result<AttachmentInfo, AppError> {
        let ticket = self.own_ticket(principal, id).await?;
        self.record_attachment(principal, &ticket, file).await
    }

    /// Attachments the Requester may see: their own uploads and files that came in with
    /// emails. Files Analysts attach stay internal.
    ///
    /// # Errors
    ///
    /// Not a Portal principal, or not the Requester's ticket.
    pub async fn portal_attachments(
        &self,
        principal: &Principal,
        id: TicketId,
    ) -> Result<Vec<AttachmentInfo>, AppError> {
        self.own_ticket(principal, id).await?;
        let me = principal.actor();
        Ok(self
            .ports
            .content
            .attachments(principal.scope(), id)
            .await?
            .into_iter()
            .filter(|a| a.created_by == me || a.created_by == Actor::Automation)
            .collect())
    }
}
