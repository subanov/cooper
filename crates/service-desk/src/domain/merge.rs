//! Merge (FR-TKT-026) and Split (FR-TKT-027) records: exactly what moved between
//! tickets, so that either can be undone. Moving the rows and changing both tickets
//! happens in one transaction (ADR-0024).

use chrono::{DateTime, Utc};
use cooper_kernel::{Actor, TenantId, TicketId};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use super::{Audited, TicketError};

/// The conversation rows that moved from one ticket to another. Records describing
/// what happened to a ticket (its audit trail, SLA events, links) never move.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MovedRows {
    /// Emails (with their threading headers).
    pub email_ids: Vec<Uuid>,
    /// Notes.
    pub note_ids: Vec<Uuid>,
    /// Time Entries.
    pub time_entry_ids: Vec<Uuid>,
    /// Attachments.
    pub attachment_ids: Vec<Uuid>,
}

impl MovedRows {
    /// Number of rows.
    #[must_use]
    pub fn len(&self) -> usize {
        self.email_ids.len()
            + self.note_ids.len()
            + self.time_entry_ids.len()
            + self.attachment_ids.len()
    }

    /// Whether nothing moved.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// What the merged-away ticket looked like before, so undo restores it exactly instead
/// of guessing "Open".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceState {
    /// Status.
    pub status_id: Uuid,
    /// Closed time.
    pub closed_at: Option<DateTime<Utc>>,
    /// Resolution Code.
    pub resolution_code_id: Option<Uuid>,
    /// Closure Category.
    pub closure_category_id: Option<Uuid>,
}

/// Who undid a merge or split, and when.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Undone {
    /// Who.
    pub by: Actor,
    /// When.
    pub at: DateTime<Utc>,
}

/// One merge of a source ticket into a target (the survivor).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketMerge {
    /// ID.
    pub id: Uuid,
    /// Company.
    pub tenant_id: TenantId,
    /// The merged-away ticket.
    pub source: TicketId,
    /// The survivor.
    pub target: TicketId,
    /// What moved to the target.
    pub moved: MovedRows,
    /// The `duplicate` link the merge created, if it created one.
    pub link_id: Option<Uuid>,
    /// The source before the merge.
    pub source_before: SourceState,
    /// Who merged.
    pub merged_by: Actor,
    /// When.
    pub merged_at: DateTime<Utc>,
    /// Set once undone.
    pub undone: Option<Undone>,
}

impl TicketMerge {
    /// Marks the merge undone.
    ///
    /// # Errors
    ///
    /// Already undone.
    pub fn undo(&mut self, by: Actor, now: DateTime<Utc>) -> Result<(), TicketError> {
        if self.undone.is_some() {
            return Err(TicketError::InvalidState("the merge was already undone"));
        }
        self.undone = Some(Undone { by, at: now });
        Ok(())
    }
}

impl Audited for TicketMerge {
    fn audit_type(&self) -> &'static str {
        "ticket_merge"
    }
    fn audit_id(&self) -> Uuid {
        self.id
    }
    fn audit_tenant(&self) -> Option<TenantId> {
        Some(self.tenant_id)
    }
    fn audit_fields(&self) -> Vec<(&'static str, Value)> {
        vec![
            ("source_ticket_id", json!(self.source)),
            ("target_ticket_id", json!(self.target)),
            ("moved", json!(self.moved)),
            ("undone_at", json!(self.undone.map(|u| u.at))),
        ]
    }
}

/// One split of emails and Notes from a ticket onto a new one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketSplit {
    /// ID.
    pub id: Uuid,
    /// Company.
    pub tenant_id: TenantId,
    /// The ticket the rows came from.
    pub source: TicketId,
    /// The ticket the split created.
    pub new_ticket: TicketId,
    /// What moved.
    pub moved: MovedRows,
    /// The internal Note left on the source saying where the rows went.
    pub marker_note_id: Option<Uuid>,
    /// The `related` link between the two tickets.
    pub link_id: Option<Uuid>,
    /// Who split.
    pub split_by: Actor,
    /// When.
    pub split_at: DateTime<Utc>,
    /// Set once undone.
    pub undone: Option<Undone>,
}

impl TicketSplit {
    /// Marks the split undone.
    ///
    /// # Errors
    ///
    /// Already undone.
    pub fn undo(&mut self, by: Actor, now: DateTime<Utc>) -> Result<(), TicketError> {
        if self.undone.is_some() {
            return Err(TicketError::InvalidState("the split was already undone"));
        }
        self.undone = Some(Undone { by, at: now });
        Ok(())
    }
}

impl Audited for TicketSplit {
    fn audit_type(&self) -> &'static str {
        "ticket_split"
    }
    fn audit_id(&self) -> Uuid {
        self.id
    }
    fn audit_tenant(&self) -> Option<TenantId> {
        Some(self.tenant_id)
    }
    fn audit_fields(&self) -> Vec<(&'static str, Value)> {
        vec![
            ("source_ticket_id", json!(self.source)),
            ("new_ticket_id", json!(self.new_ticket)),
            ("moved", json!(self.moved)),
            ("undone_at", json!(self.undone.map(|u| u.at))),
        ]
    }
}

/// How long a heartbeat keeps an Analyst present on a ticket (FR-TKT-025). Closed tabs
/// and lost connections disappear on their own after this.
pub const PRESENCE_STALE_SECONDS: i64 = 30;

/// One Analyst who has a ticket open (FR-TKT-025).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TicketPresence {
    /// Who.
    pub analyst_id: cooper_kernel::AnalystId,
    /// Whether they are writing a reply or Note.
    pub is_composing: bool,
    /// Last heartbeat.
    pub last_seen: DateTime<Utc>,
}

impl TicketPresence {
    /// Whether the heartbeat is recent enough at `now`.
    #[must_use]
    pub fn is_live(&self, now: DateTime<Utc>) -> bool {
        now - self.last_seen <= chrono::Duration::seconds(PRESENCE_STALE_SECONDS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_merge_or_split_is_undone_once() {
        let mut merge = TicketMerge {
            id: Uuid::now_v7(),
            tenant_id: TenantId::new(),
            source: TicketId::new(),
            target: TicketId::new(),
            moved: MovedRows {
                note_ids: vec![Uuid::now_v7()],
                ..MovedRows::default()
            },
            link_id: None,
            source_before: SourceState {
                status_id: Uuid::now_v7(),
                closed_at: None,
                resolution_code_id: None,
                closure_category_id: None,
            },
            merged_by: Actor::Automation,
            merged_at: Utc::now(),
            undone: None,
        };
        assert_eq!(merge.moved.len(), 1);
        merge.undo(Actor::Automation, Utc::now()).unwrap();
        assert!(merge.undo(Actor::Automation, Utc::now()).is_err());
    }

    #[test]
    fn presence_goes_stale() {
        let now = Utc::now();
        let p = TicketPresence {
            analyst_id: cooper_kernel::AnalystId::new(),
            is_composing: false,
            last_seen: now - chrono::Duration::seconds(PRESENCE_STALE_SECONDS),
        };
        assert!(p.is_live(now));
        assert!(!p.is_live(now + chrono::Duration::seconds(1)));
    }
}
