//! Checklist Templates and Checklists (FR-TSK-008, FR-TKT-032). Pure Rust: no I/O.

use std::str::FromStr;

use chrono::{DateTime, Utc};
use cooper_kernel::{Actor, TenantId};
use thiserror::Error;
use uuid::Uuid;

/// A checklist problem.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ChecklistError {
    /// A value failed validation.
    #[error("{field}: {reason}")]
    Invalid {
        /// The field.
        field: &'static str,
        /// Why.
        reason: String,
    },
    /// The action is not allowed now.
    #[error("{0}")]
    InvalidState(String),
}

fn invalid(field: &'static str, reason: impl Into<String>) -> ChecklistError {
    ChecklistError::Invalid { field, reason: reason.into() }
}

/// Most items in one version.
pub const MAX_ITEMS: usize = 100;
/// Longest item text, in characters.
pub const MAX_ITEM_LEN: usize = 500;
/// Longest answer, in characters.
pub const MAX_ANSWER_LEN: usize = 2000;

/// Whether open mandatory items stop closure (fixed set).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClosureMode {
    /// Closure is not affected.
    Advisory,
    /// Open mandatory items block closure.
    Blocking,
}

impl ClosureMode {
    /// The stored name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Advisory => "advisory",
            Self::Blocking => "blocking",
        }
    }
}

impl FromStr for ClosureMode {
    type Err = ChecklistError;

    fn from_str(s: &str) -> Result<Self, ChecklistError> {
        match s {
            "advisory" => Ok(Self::Advisory),
            "blocking" => Ok(Self::Blocking),
            other => Err(invalid("closure_mode", format!("{other:?} is not advisory or blocking"))),
        }
    }
}

/// What a Checklist is attached to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TargetKind {
    /// A ticket.
    Ticket,
    /// A task.
    Task,
}

impl TargetKind {
    /// The stored name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ticket => "ticket",
            Self::Task => "task",
        }
    }
}

impl FromStr for TargetKind {
    type Err = ChecklistError;

    fn from_str(s: &str) -> Result<Self, ChecklistError> {
        match s {
            "ticket" => Ok(Self::Ticket),
            "task" => Ok(Self::Task),
            other => Err(invalid("target", format!("{other:?} is not a ticket or task"))),
        }
    }
}

/// An item as an Administrator enters it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemInput {
    /// The text of the item.
    pub text: String,
    /// Must be done before the Checklist can close a blocking ticket.
    pub mandatory: bool,
    /// Needs an answer when done.
    pub requires_input: bool,
}

fn validate_items(items: Vec<ItemInput>) -> Result<Vec<ItemInput>, ChecklistError> {
    if items.is_empty() || items.len() > MAX_ITEMS {
        return Err(invalid("items", "a checklist needs 1 to 100 items"));
    }
    items
        .into_iter()
        .map(|i| {
            let text = i.text.trim().to_owned();
            if text.is_empty() || text.chars().count() > MAX_ITEM_LEN {
                return Err(invalid("items", "each item needs 1 to 500 characters"));
            }
            Ok(ItemInput { text, ..i })
        })
        .collect()
}

fn validate_name(field: &'static str, value: &str, max: usize) -> Result<String, ChecklistError> {
    let v = value.trim().to_owned();
    if v.is_empty() || v.chars().count() > max {
        return Err(invalid(field, format!("must be 1 to {max} characters")));
    }
    Ok(v)
}

/// One published version of a template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateVersion {
    /// Version number, from 1.
    pub number: u32,
    /// Closure mode of this version.
    pub closure_mode: ClosureMode,
    /// Items in order.
    pub items: Vec<ItemInput>,
    /// Who published it.
    pub created_by: Actor,
    /// When.
    pub created_at: DateTime<Utc>,
}

/// A Checklist Template: its category and role, and every published version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    /// ID.
    pub id: Uuid,
    /// Company.
    pub tenant_id: TenantId,
    /// Name (unique per Company).
    pub name: String,
    /// Category, for finding templates.
    pub category: String,
    /// The role it is meant for, such as Onboarding.
    pub role: String,
    /// The current version.
    pub current: TemplateVersion,
    /// Earlier versions, oldest first.
    pub previous: Vec<TemplateVersion>,
    /// When it was created.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

impl Template {
    /// Creates a template with its first version.
    ///
    /// # Errors
    ///
    /// Invalid name, category, role or items.
    #[allow(clippy::too_many_arguments)] // a template is named by three labels, a mode, items, author and time
    pub fn create(
        tenant_id: TenantId,
        name: &str,
        category: &str,
        role: &str,
        mode: ClosureMode,
        items: Vec<ItemInput>,
        by: Actor,
        now: DateTime<Utc>,
    ) -> Result<Self, ChecklistError> {
        Ok(Self {
            id: Uuid::now_v7(),
            tenant_id,
            name: validate_name("name", name, 150)?,
            category: validate_name("category", category, 100)?,
            role: validate_name("role", role, 100)?,
            current: TemplateVersion { number: 1, closure_mode: mode, items: validate_items(items)?, created_by: by, created_at: now },
            previous: Vec::new(),
            created_at: now,
            updated_at: now,
        })
    }

    /// Publishes a new version; earlier versions stay as they were.
    ///
    /// # Errors
    ///
    /// Invalid items; nothing changes then.
    pub fn publish(&mut self, mode: ClosureMode, items: Vec<ItemInput>, by: Actor, now: DateTime<Utc>) -> Result<u32, ChecklistError> {
        let number = self.current.number + 1;
        let next = TemplateVersion { number, closure_mode: mode, items: validate_items(items)?, created_by: by, created_at: now };
        let old = std::mem::replace(&mut self.current, next);
        self.previous.push(old);
        self.updated_at = now;
        Ok(number)
    }

    /// All versions, oldest first.
    pub fn versions(&self) -> impl Iterator<Item = &TemplateVersion> {
        self.previous.iter().chain(std::iter::once(&self.current))
    }
}

/// One item of a Checklist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChecklistItem {
    /// ID.
    pub id: Uuid,
    /// Position in the list, from 0.
    pub position: u32,
    /// Text.
    pub text: String,
    /// Mandatory.
    pub mandatory: bool,
    /// Needs an answer when done.
    pub requires_input: bool,
    /// Whether it is done.
    pub done: bool,
    /// The answer, if given.
    pub answer: Option<String>,
    /// When it was last completed.
    pub done_at: Option<DateTime<Utc>>,
    /// Who completed it last.
    pub done_by: Option<Actor>,
}

/// A Checklist on a ticket or task: a copy of one template version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checklist {
    /// ID.
    pub id: Uuid,
    /// Company.
    pub tenant_id: TenantId,
    /// What it is on.
    pub target: TargetKind,
    /// The ticket or task.
    pub target_id: Uuid,
    /// The template it came from.
    pub template_id: Uuid,
    /// The template version it copies.
    pub template_version: u32,
    /// Name (the template's name).
    pub name: String,
    /// Closure mode (the template version's).
    pub closure_mode: ClosureMode,
    /// Items.
    pub items: Vec<ChecklistItem>,
    /// When it was attached.
    pub created_at: DateTime<Utc>,
}

impl Checklist {
    /// Makes a Checklist on a target from the current version of a template.
    #[must_use]
    pub fn from_template(template: &Template, target: TargetKind, target_id: Uuid, now: DateTime<Utc>) -> Self {
        let items = template
            .current
            .items
            .iter()
            .enumerate()
            .map(|(i, item)| ChecklistItem {
                id: Uuid::now_v7(),
                position: u32::try_from(i).unwrap_or(u32::MAX),
                text: item.text.clone(),
                mandatory: item.mandatory,
                requires_input: item.requires_input,
                done: false,
                answer: None,
                done_at: None,
                done_by: None,
            })
            .collect();
        Self {
            id: Uuid::now_v7(),
            tenant_id: template.tenant_id,
            target,
            target_id,
            template_id: template.id,
            template_version: template.current.number,
            name: template.name.clone(),
            closure_mode: template.current.closure_mode,
            items,
            created_at: now,
        }
    }

    /// Marks an item done. An item that requires input needs a non-blank answer.
    ///
    /// # Errors
    ///
    /// An unknown item, a missing answer, or an answer that is too long.
    pub fn complete_item(&mut self, item: Uuid, answer: Option<&str>, by: Actor, now: DateTime<Utc>) -> Result<(), ChecklistError> {
        let answer = answer.map(str::trim).filter(|a| !a.is_empty()).map(str::to_owned);
        if answer.as_deref().is_some_and(|a| a.chars().count() > MAX_ANSWER_LEN) {
            return Err(invalid("answer", "must be at most 2000 characters"));
        }
        let it = self
            .items
            .iter_mut()
            .find(|i| i.id == item)
            .ok_or_else(|| ChecklistError::InvalidState("that item is not on this checklist".into()))?;
        if it.requires_input && answer.is_none() {
            return Err(invalid("answer", "is required for this item"));
        }
        it.done = true;
        it.answer = answer;
        it.done_at = Some(now);
        it.done_by = Some(by);
        Ok(())
    }

    /// Marks an item not done again. Its answer is kept for the history.
    ///
    /// # Errors
    ///
    /// An unknown item.
    pub fn reopen_item(&mut self, item: Uuid) -> Result<(), ChecklistError> {
        let it = self
            .items
            .iter_mut()
            .find(|i| i.id == item)
            .ok_or_else(|| ChecklistError::InvalidState("that item is not on this checklist".into()))?;
        it.done = false;
        Ok(())
    }

    /// Mandatory items not done yet.
    #[must_use]
    pub fn open_mandatory(&self) -> Vec<&ChecklistItem> {
        self.items.iter().filter(|i| i.mandatory && !i.done).collect()
    }

    /// Whether this Checklist stops its ticket from closing.
    #[must_use]
    pub fn blocks_closure(&self) -> bool {
        self.closure_mode == ClosureMode::Blocking && !self.open_mandatory().is_empty()
    }
}

/// The open mandatory items, as "checklist: item" texts, of the blocking Checklists in
/// `checklists`. Empty means closure may go ahead.
#[must_use]
pub fn blocking_items(checklists: &[Checklist]) -> Vec<String> {
    checklists
        .iter()
        .filter(|c| c.closure_mode == ClosureMode::Blocking)
        .flat_map(|c| c.open_mandatory().into_iter().map(move |i| format!("{}: {}", c.name, i.text)))
        .collect()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)] // test code

    use super::*;
    use chrono::TimeZone;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 29, 9, 0, 0).unwrap()
    }

    fn item(text: &str, mandatory: bool, requires_input: bool) -> ItemInput {
        ItemInput { text: text.into(), mandatory, requires_input }
    }

    fn template(mode: ClosureMode, items: Vec<ItemInput>) -> Template {
        Template::create(TenantId::new(), "Leaver", "HR", "Onboarding", mode, items, Actor::Automation, now()).unwrap()
    }

    #[test]
    fn a_template_needs_a_name_category_role_and_items() {
        let ok = vec![item("Collect laptop", true, false)];
        assert!(Template::create(TenantId::new(), " ", "HR", "x", ClosureMode::Advisory, ok.clone(), Actor::Automation, now()).is_err());
        assert!(Template::create(TenantId::new(), "a", "", "x", ClosureMode::Advisory, ok.clone(), Actor::Automation, now()).is_err());
        assert!(Template::create(TenantId::new(), "a", "b", "", ClosureMode::Advisory, ok, Actor::Automation, now()).is_err());
        assert!(Template::create(TenantId::new(), "a", "b", "c", ClosureMode::Advisory, vec![], Actor::Automation, now()).is_err());
        let blank = vec![item("   ", false, false)];
        assert!(Template::create(TenantId::new(), "a", "b", "c", ClosureMode::Advisory, blank, Actor::Automation, now()).is_err());
    }

    #[test]
    fn publishing_keeps_earlier_versions_and_items_are_trimmed() {
        let mut t = template(ClosureMode::Advisory, vec![item(" Collect laptop ", true, false)]);
        assert_eq!(t.current.items[0].text, "Collect laptop");
        let number = t.publish(ClosureMode::Blocking, vec![item("Revoke badge", true, false), item("Sign form", false, true)], Actor::Automation, now()).unwrap();
        assert_eq!(number, 2);
        let modes: Vec<ClosureMode> = t.versions().map(|v| v.closure_mode).collect();
        assert_eq!(modes, [ClosureMode::Advisory, ClosureMode::Blocking]);
        assert!(t.publish(ClosureMode::Blocking, vec![], Actor::Automation, now()).is_err());
        assert_eq!(t.current.number, 2, "a rejected version changes nothing");
    }

    #[test]
    fn a_checklist_copies_the_template_version_it_was_made_from() {
        let mut t = template(ClosureMode::Blocking, vec![item("Revoke badge", true, false)]);
        let list = Checklist::from_template(&t, TargetKind::Ticket, Uuid::now_v7(), now());
        t.publish(ClosureMode::Advisory, vec![item("Something else", false, false)], Actor::Automation, now()).unwrap();
        assert_eq!(list.template_version, 1);
        assert_eq!(list.closure_mode, ClosureMode::Blocking);
        assert_eq!(list.items.len(), 1);
        assert_eq!(list.items[0].text, "Revoke badge");
    }

    #[test]
    fn items_that_require_input_need_an_answer_before_they_are_done() {
        let t = template(ClosureMode::Advisory, vec![item("Sign form", true, true)]);
        let mut list = Checklist::from_template(&t, TargetKind::Task, Uuid::now_v7(), now());
        let id = list.items[0].id;
        assert!(list.complete_item(id, None, Actor::Automation, now()).is_err());
        assert!(list.complete_item(id, Some("   "), Actor::Automation, now()).is_err());
        list.complete_item(id, Some(" signed "), Actor::Automation, now()).unwrap();
        assert!(list.items[0].done);
        assert_eq!(list.items[0].answer.as_deref(), Some("signed"));
        assert!(list.complete_item(Uuid::now_v7(), Some("x"), Actor::Automation, now()).is_err());
    }

    #[test]
    fn only_blocking_checklists_with_open_mandatory_items_block_closure() {
        let advisory = template(ClosureMode::Advisory, vec![item("Optional step", true, false)]);
        let blocking = template(ClosureMode::Blocking, vec![item("Revoke badge", true, false), item("Tidy desk", false, false)]);
        let a = Checklist::from_template(&advisory, TargetKind::Ticket, Uuid::now_v7(), now());
        let mut b = Checklist::from_template(&blocking, TargetKind::Ticket, Uuid::now_v7(), now());
        assert!(!a.blocks_closure(), "advisory never blocks");
        assert!(b.blocks_closure());
        assert_eq!(blocking_items(&[a.clone(), b.clone()]), ["Leaver: Revoke badge"]);
        let badge = b.items[0].id;
        b.complete_item(badge, None, Actor::Automation, now()).unwrap();
        assert!(!b.blocks_closure(), "optional items never block");
        b.reopen_item(badge).unwrap();
        assert!(b.blocks_closure(), "reopening blocks again");
    }

    #[test]
    fn target_kinds_and_closure_modes_parse_from_their_names() {
        assert_eq!("ticket".parse::<TargetKind>().unwrap(), TargetKind::Ticket);
        assert!("project".parse::<TargetKind>().is_err());
        assert_eq!("blocking".parse::<ClosureMode>().unwrap(), ClosureMode::Blocking);
        assert!("maybe".parse::<ClosureMode>().is_err());
    }
}
