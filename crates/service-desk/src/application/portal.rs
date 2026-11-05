//! Portal use cases for Requesters (FR-PRT-002): the Portal Catalogue and raising a
//! ticket from it. The Requester and Company always come from the principal, never
//! from the request.

use cooper_kernel::{Actor, Principal, RequesterId, Sensitivity, TenantId, TenantScope};
use uuid::Uuid;

use super::{AppError, CreateTicket, ServiceDesk};
use crate::domain::{
    Category, LookupKind, Ticket, TicketDetails, TicketError, portal_catalogue, portal_ticket_type,
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
}

/// The Requester and Company of a Portal principal.
fn portal_requester(principal: &Principal) -> Result<(RequesterId, TenantId), AppError> {
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
                },
            )
            .await?;
        tracing::info!(ticket = ticket.number(), "ticket raised on the Portal");
        Ok(ticket)
    }
}
