//! Ticket Fields (FR-TKT-029) and Ticket Type Rules (FR-TKT-011): their administration
//! and the Ticket Field values of a ticket.

use std::collections::BTreeMap;

use cooper_kernel::{AuditAction, AuditRecord, Principal, TicketId};
use serde_json::Value;
use uuid::Uuid;

use super::{AppError, ServiceDesk, capabilities, require};
use crate::domain::{
    LookupKind, TicketError, TicketField, TicketFieldInput, TicketTypeRule, TicketTypeRuleInput,
    audit_record, parse_values,
};

/// A Ticket Field with its value on one ticket.
#[derive(Debug, Clone)]
pub struct FieldValue {
    /// The field.
    pub field: TicketField,
    /// Its value; `None` when empty.
    pub value: Option<Value>,
}

impl crate::domain::Audited for TicketField {
    fn audit_type(&self) -> &'static str {
        "ticket_field"
    }
    fn audit_id(&self) -> Uuid {
        self.id()
    }
    fn audit_tenant(&self) -> Option<cooper_kernel::TenantId> {
        None
    }
    fn audit_fields(&self) -> Vec<(&'static str, Value)> {
        let i = self.input();
        vec![
            ("key", serde_json::json!(i.key)),
            ("label", serde_json::json!(i.label)),
            ("field_type", serde_json::json!(i.field_type.as_str())),
            ("options", serde_json::json!(i.options)),
            ("type_ids", serde_json::json!(i.type_ids)),
            (
                "required_on_create",
                serde_json::json!(i.required_on_create),
            ),
            ("required_on_close", serde_json::json!(i.required_on_close)),
            ("ask_requester", serde_json::json!(i.ask_requester)),
            ("is_active", serde_json::json!(i.is_active)),
        ]
    }
}

impl crate::domain::Audited for TicketTypeRule {
    fn audit_type(&self) -> &'static str {
        "ticket_type_rule"
    }
    fn audit_id(&self) -> Uuid {
        self.id()
    }
    fn audit_tenant(&self) -> Option<cooper_kernel::TenantId> {
        None
    }
    fn audit_fields(&self) -> Vec<(&'static str, Value)> {
        let i = self.input();
        vec![
            ("name", serde_json::json!(i.name)),
            ("ticket_type_id", serde_json::json!(i.ticket_type_id)),
            ("conditions", serde_json::json!(i.conditions)),
            ("sort_order", serde_json::json!(i.sort_order)),
            ("is_active", serde_json::json!(i.is_active)),
        ]
    }
}

impl ServiceDesk {
    /// Every Ticket Field, active and retired (needs `tickets.read`).
    ///
    /// # Errors
    ///
    /// Forbidden.
    pub async fn ticket_fields(&self, principal: &Principal) -> Result<Vec<TicketField>, AppError> {
        require(principal, capabilities::READ)?;
        Ok(self.ports.reference.ticket_fields().await?)
    }

    /// The active Ticket Fields asked of Requesters on the Portal (FR-TKT-029).
    ///
    /// # Errors
    ///
    /// Storage failures.
    pub async fn portal_ticket_fields(
        &self,
        principal: &Principal,
    ) -> Result<Vec<TicketField>, AppError> {
        super::portal::portal_requester(principal)?;
        Ok(self
            .ports
            .reference
            .ticket_fields()
            .await?
            .into_iter()
            .filter(|f| f.input().is_active && f.input().ask_requester)
            .collect())
    }

    async fn check_field_types(&self, type_ids: &[Uuid]) -> Result<(), AppError> {
        let types = self.ports.reference.lookups(LookupKind::Type).await?;
        for id in type_ids {
            if !types.iter().any(|t| t.id() == *id) {
                return Err(
                    TicketError::invalid("type_ids", "refers to an unknown Ticket Type").into(),
                );
            }
        }
        Ok(())
    }

    /// Defines a Ticket Field (needs `tickets.configure`).
    ///
    /// # Errors
    ///
    /// Forbidden, invalid input, unknown Ticket Types or a duplicate key.
    pub async fn create_ticket_field(
        &self,
        principal: &Principal,
        input: TicketFieldInput,
    ) -> Result<TicketField, AppError> {
        require(principal, capabilities::CONFIGURE)?;
        self.check_field_types(&input.type_ids).await?;
        let now = self.ports.clock.now();
        let field = TicketField::define(input, now)?;
        if self
            .ports
            .reference
            .ticket_fields()
            .await?
            .iter()
            .any(|f| f.key() == field.key())
        {
            return Err(AppError::Conflict(
                "a Ticket Field with this key exists".into(),
            ));
        }
        let audit = audit_record(None, &field, AuditAction::Created, principal.actor(), now);
        self.ports
            .reference
            .save_ticket_field(&field, &audit)
            .await?;
        Ok(field)
    }

    /// Changes a Ticket Field; its key and type never change (needs `tickets.configure`).
    ///
    /// # Errors
    ///
    /// Forbidden, not found, or invalid input.
    pub async fn update_ticket_field(
        &self,
        principal: &Principal,
        id: Uuid,
        input: TicketFieldInput,
    ) -> Result<TicketField, AppError> {
        require(principal, capabilities::CONFIGURE)?;
        self.check_field_types(&input.type_ids).await?;
        let mut field = self
            .ports
            .reference
            .ticket_fields()
            .await?
            .into_iter()
            .find(|f| f.id() == id)
            .ok_or(AppError::NotFound("Ticket Field"))?;
        let before = field.clone();
        let now = self.ports.clock.now();
        field.update(input, now)?;
        let audit = audit_record(
            Some(&before),
            &field,
            AuditAction::Updated,
            principal.actor(),
            now,
        );
        self.ports
            .reference
            .save_ticket_field(&field, &audit)
            .await?;
        Ok(field)
    }

    /// Ticket Type Rules in evaluation order (needs `tickets.configure`).
    ///
    /// # Errors
    ///
    /// Forbidden.
    pub async fn type_rules(&self, principal: &Principal) -> Result<Vec<TicketTypeRule>, AppError> {
        require(principal, capabilities::CONFIGURE)?;
        Ok(self.ports.reference.type_rules().await?)
    }

    /// Creates a Ticket Type Rule (needs `tickets.configure`).
    ///
    /// # Errors
    ///
    /// Forbidden, invalid input or an unknown Ticket Type.
    pub async fn create_type_rule(
        &self,
        principal: &Principal,
        input: TicketTypeRuleInput,
    ) -> Result<TicketTypeRule, AppError> {
        require(principal, capabilities::CONFIGURE)?;
        self.check_field_types(&[input.ticket_type_id]).await?;
        let now = self.ports.clock.now();
        let rule = TicketTypeRule::create(input, now)?;
        let audit = audit_record(None, &rule, AuditAction::Created, principal.actor(), now);
        self.ports.reference.save_type_rule(&rule, &audit).await?;
        Ok(rule)
    }

    async fn type_rule(&self, id: Uuid) -> Result<TicketTypeRule, AppError> {
        self.ports
            .reference
            .type_rules()
            .await?
            .into_iter()
            .find(|r| r.id() == id)
            .ok_or(AppError::NotFound("Ticket Type Rule"))
    }

    /// Changes a Ticket Type Rule (needs `tickets.configure`).
    ///
    /// # Errors
    ///
    /// Forbidden, not found, invalid input or an unknown Ticket Type.
    pub async fn update_type_rule(
        &self,
        principal: &Principal,
        id: Uuid,
        input: TicketTypeRuleInput,
    ) -> Result<TicketTypeRule, AppError> {
        require(principal, capabilities::CONFIGURE)?;
        self.check_field_types(&[input.ticket_type_id]).await?;
        let mut rule = self.type_rule(id).await?;
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
        self.ports.reference.save_type_rule(&rule, &audit).await?;
        Ok(rule)
    }

    /// Deletes a Ticket Type Rule (needs `tickets.configure`).
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn delete_type_rule(&self, principal: &Principal, id: Uuid) -> Result<(), AppError> {
        require(principal, capabilities::CONFIGURE)?;
        let rule = self.type_rule(id).await?;
        let now = self.ports.clock.now();
        let audit = AuditRecord::new(
            None,
            "ticket_type_rule",
            id,
            AuditAction::Deleted,
            principal.actor(),
            now,
        );
        self.ports.reference.delete_type_rule(&rule, &audit).await?;
        Ok(())
    }

    /// The Ticket Fields of a ticket with their values: every field that applies to its
    /// type, plus retired or inapplicable fields that still hold a value.
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn ticket_field_values(
        &self,
        principal: &Principal,
        id: TicketId,
    ) -> Result<Vec<FieldValue>, AppError> {
        require(principal, capabilities::READ)?;
        let ticket = self.load(principal, id, true).await?;
        let values = self
            .ports
            .relations
            .field_values(principal.scope(), id)
            .await?;
        Ok(self
            .ports
            .reference
            .ticket_fields()
            .await?
            .into_iter()
            .filter(|f| f.applies_to(ticket.details().type_id) || values.contains_key(&f.id()))
            .map(|field| FieldValue {
                value: values.get(&field.id()).cloned(),
                field,
            })
            .collect())
    }

    /// Sets or clears (`null`) Ticket Field values by key (needs `tickets.update`).
    ///
    /// # Errors
    ///
    /// Forbidden, not found, a deleted ticket, unknown keys or invalid values.
    pub async fn set_ticket_fields(
        &self,
        principal: &Principal,
        id: TicketId,
        submitted: &BTreeMap<String, Value>,
    ) -> Result<Vec<FieldValue>, AppError> {
        require(principal, capabilities::UPDATE)?;
        let ticket = self.load(principal, id, false).await?;
        let fields = self.ports.reference.ticket_fields().await?;
        let changes = parse_values(&fields, ticket.details().type_id, submitted, false)?;
        let old = self
            .ports
            .relations
            .field_values(principal.scope(), id)
            .await?;
        let now = self.ports.clock.now();
        let mut audit = AuditRecord::new(
            Some(ticket.tenant_id()),
            "ticket",
            id.as_uuid(),
            AuditAction::Updated,
            principal.actor(),
            now,
        );
        for (field_id, value) in &changes {
            let key = fields
                .iter()
                .find(|f| f.id() == *field_id)
                .map_or_else(String::new, |f| f.key().to_owned());
            audit = audit.change(format!("fields.{key}"), old.get(field_id), value.as_ref());
        }
        if audit.is_meaningful() {
            let mut touched = ticket.clone();
            touched.touch(now);
            self.ports
                .relations
                .set_field_values(&touched, &changes, &audit)
                .await?;
        }
        self.ticket_field_values(principal, id).await
    }
}
