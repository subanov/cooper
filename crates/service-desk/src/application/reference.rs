//! Reference data use cases (FR-TKT-004 to FR-TKT-007, FR-SYS-001).

use std::collections::BTreeSet;

use cooper_kernel::{AuditAction, Principal};
use uuid::Uuid;

use super::{AppError, ServiceDesk, capabilities, require};
use crate::domain::{Category, CategoryInput, Lookup, LookupFlags, LookupKind, audit_record};

/// Editable fields of a reference-data row.
#[derive(Debug, Clone)]
pub struct LookupInput {
    /// Name.
    pub name: String,
    /// Behaviour flags.
    pub flags: LookupFlags,
    /// Display order.
    pub sort_order: i32,
    /// Active flag.
    pub is_active: bool,
}

impl ServiceDesk {
    /// Rows of a kind, in display order. Any staff member may read them.
    ///
    /// # Errors
    ///
    /// Forbidden or storage failures.
    pub async fn lookups(
        &self,
        principal: &Principal,
        kind: LookupKind,
    ) -> Result<Vec<Lookup>, AppError> {
        require(principal, capabilities::READ)?;
        Ok(self.ports.reference.lookups(kind).await?)
    }

    pub(super) async fn lookup(&self, kind: LookupKind, id: Uuid) -> Result<Lookup, AppError> {
        self.ports
            .reference
            .lookups(kind)
            .await?
            .into_iter()
            .find(|l| l.id() == id)
            .ok_or(AppError::NotFound(kind.label()))
    }

    pub(super) async fn default_lookup(&self, kind: LookupKind) -> Result<Lookup, AppError> {
        self.ports
            .reference
            .lookups(kind)
            .await?
            .into_iter()
            .find(|l| l.flags().is_default && l.is_active())
            .ok_or(AppError::NotFound(kind.label()))
    }

    /// Creates a row.
    ///
    /// # Errors
    ///
    /// Forbidden, invalid input or a duplicate name.
    pub async fn create_lookup(
        &self,
        principal: &Principal,
        kind: LookupKind,
        input: LookupInput,
    ) -> Result<Lookup, AppError> {
        require(principal, capabilities::CONFIGURE)?;
        let now = self.ports.clock.now();
        let lookup = Lookup::create(kind, &input.name, input.flags, input.sort_order, now)?;
        let audit = audit_record(None, &lookup, AuditAction::Created, principal.actor(), now);
        self.ports
            .reference
            .save_lookup(&lookup, &audit, true)
            .await?;
        Ok(lookup)
    }

    /// Changes a row. Making it the default takes the flag from the previous default.
    /// The default cannot simply be switched off: make another row the default instead.
    ///
    /// # Errors
    ///
    /// Forbidden, not found, invalid input or a duplicate name.
    pub async fn update_lookup(
        &self,
        principal: &Principal,
        kind: LookupKind,
        id: Uuid,
        input: LookupInput,
    ) -> Result<Lookup, AppError> {
        require(principal, capabilities::CONFIGURE)?;
        let mut lookup = self.lookup(kind, id).await?;
        if lookup.flags().is_default && !input.flags.is_default {
            return Err(crate::domain::TicketError::invalid(
                "is_default",
                "choose another default instead",
            )
            .into());
        }
        let before = lookup.clone();
        let now = self.ports.clock.now();
        lookup.update(
            &input.name,
            input.flags,
            input.sort_order,
            input.is_active,
            now,
        )?;
        let audit = audit_record(
            Some(&before),
            &lookup,
            AuditAction::Updated,
            principal.actor(),
            now,
        );
        if audit.is_meaningful() {
            self.ports
                .reference
                .save_lookup(&lookup, &audit, false)
                .await?;
        }
        Ok(lookup)
    }

    /// Every Category.
    ///
    /// # Errors
    ///
    /// Forbidden or storage failures.
    pub async fn categories(&self, principal: &Principal) -> Result<Vec<Category>, AppError> {
        require(principal, capabilities::READ)?;
        Ok(self.ports.reference.categories().await?)
    }

    async fn category(&self, id: Uuid) -> Result<Category, AppError> {
        self.ports
            .reference
            .categories()
            .await?
            .into_iter()
            .find(|c| c.id() == id)
            .ok_or(AppError::NotFound("Category"))
    }

    async fn check_types(&self, type_ids: &BTreeSet<Uuid>) -> Result<(), AppError> {
        let known: BTreeSet<Uuid> = self
            .ports
            .reference
            .lookups(LookupKind::Type)
            .await?
            .iter()
            .map(Lookup::id)
            .collect();
        if type_ids.is_subset(&known) {
            Ok(())
        } else {
            Err(
                crate::domain::TicketError::invalid("type_ids", "contains an unknown Ticket Type")
                    .into(),
            )
        }
    }

    /// Creates a Category under `parent_id` (or at the top).
    ///
    /// # Errors
    ///
    /// Forbidden, unknown parent or type, too deep, or a duplicate sibling name.
    pub async fn create_category(
        &self,
        principal: &Principal,
        parent_id: Option<Uuid>,
        input: CategoryInput,
    ) -> Result<Category, AppError> {
        require(principal, capabilities::CONFIGURE)?;
        self.check_types(&input.type_ids).await?;
        let parent = match parent_id {
            Some(id) => Some(self.category(id).await?),
            None => None,
        };
        let now = self.ports.clock.now();
        let category = Category::create(parent.as_ref(), input, now)?;
        let audit = audit_record(
            None,
            &category,
            AuditAction::Created,
            principal.actor(),
            now,
        );
        self.ports
            .reference
            .save_category(&category, &audit, true)
            .await?;
        Ok(category)
    }

    /// Changes a Category.
    ///
    /// # Errors
    ///
    /// Forbidden, not found, unknown type or a duplicate sibling name.
    pub async fn update_category(
        &self,
        principal: &Principal,
        id: Uuid,
        input: CategoryInput,
    ) -> Result<Category, AppError> {
        require(principal, capabilities::CONFIGURE)?;
        self.check_types(&input.type_ids).await?;
        let mut category = self.category(id).await?;
        let before = category.clone();
        let now = self.ports.clock.now();
        category.update(input, now)?;
        let audit = audit_record(
            Some(&before),
            &category,
            AuditAction::Updated,
            principal.actor(),
            now,
        );
        if audit.is_meaningful() {
            self.ports
                .reference
                .save_category(&category, &audit, false)
                .await?;
        }
        Ok(category)
    }
}
