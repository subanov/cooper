//! Ticket Links (FR-TKT-028): related, duplicate and parent/child links between tickets
//! of one Company. Informational: a link never changes either ticket's state.

use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use cooper_kernel::{Actor, TenantId, TicketId};
use serde_json::{Value, json};
use uuid::Uuid;

use super::{Audited, TicketError};

/// How two tickets are linked, as stored. The source/target direction matters for
/// [`LinkKind::Duplicate`] and [`LinkKind::Parent`] only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LinkKind {
    /// Symmetric: the tickets are about related things.
    Related,
    /// The source duplicates the target (the master).
    Duplicate,
    /// The source is the parent of the target.
    Parent,
}

impl LinkKind {
    /// Stable storage name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Related => "related",
            Self::Duplicate => "duplicate",
            Self::Parent => "parent",
        }
    }
}

impl FromStr for LinkKind {
    type Err = TicketError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "related" => Ok(Self::Related),
            "duplicate" => Ok(Self::Duplicate),
            "parent" => Ok(Self::Parent),
            _ => Err(TicketError::invalid("kind", "unknown link kind")),
        }
    }
}

/// A link as seen from one ticket (what the Analyst picks and what the API shows).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LinkRelation {
    /// Related to the other ticket.
    Related,
    /// This ticket duplicates the other (the master).
    DuplicateOf,
    /// The other ticket duplicates this one.
    DuplicatedBy,
    /// This ticket is the parent of the other.
    ParentOf,
    /// This ticket is a child of the other.
    ChildOf,
}

impl LinkRelation {
    /// Stable API name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Related => "related",
            Self::DuplicateOf => "duplicate_of",
            Self::DuplicatedBy => "duplicated_by",
            Self::ParentOf => "parent_of",
            Self::ChildOf => "child_of",
        }
    }

    /// The stored kind and whether the acting ticket is the stored *target*.
    #[must_use]
    pub const fn stored(self) -> (LinkKind, bool) {
        match self {
            Self::Related => (LinkKind::Related, false),
            Self::DuplicateOf => (LinkKind::Duplicate, false),
            Self::DuplicatedBy => (LinkKind::Duplicate, true),
            Self::ParentOf => (LinkKind::Parent, false),
            Self::ChildOf => (LinkKind::Parent, true),
        }
    }

    /// The relation of a stored link from the point of view of `from`.
    #[must_use]
    pub fn of(link: &TicketLink, from: TicketId) -> Self {
        let outgoing = link.source == from;
        match (link.kind, outgoing) {
            (LinkKind::Related, _) => Self::Related,
            (LinkKind::Duplicate, true) => Self::DuplicateOf,
            (LinkKind::Duplicate, false) => Self::DuplicatedBy,
            (LinkKind::Parent, true) => Self::ParentOf,
            (LinkKind::Parent, false) => Self::ChildOf,
        }
    }
}

impl FromStr for LinkRelation {
    type Err = TicketError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "related" => Ok(Self::Related),
            "duplicate_of" => Ok(Self::DuplicateOf),
            "duplicated_by" => Ok(Self::DuplicatedBy),
            "parent_of" => Ok(Self::ParentOf),
            "child_of" => Ok(Self::ChildOf),
            _ => Err(TicketError::invalid(
                "relation",
                "must be related, duplicate_of, duplicated_by, parent_of or child_of",
            )),
        }
    }
}

impl fmt::Display for LinkRelation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A stored link between two tickets of one Company.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketLink {
    id: Uuid,
    tenant_id: TenantId,
    source: TicketId,
    target: TicketId,
    kind: LinkKind,
    created_by: Actor,
    created_at: DateTime<Utc>,
}

impl TicketLink {
    /// Links `from` to `other` with `relation` as seen from `from`.
    ///
    /// # Errors
    ///
    /// A ticket linked to itself.
    pub fn new(
        tenant_id: TenantId,
        from: TicketId,
        other: TicketId,
        relation: LinkRelation,
        created_by: Actor,
        now: DateTime<Utc>,
    ) -> Result<Self, TicketError> {
        if from == other {
            return Err(TicketError::invalid(
                "ticket_id",
                "a ticket cannot be linked to itself",
            ));
        }
        let (kind, reversed) = relation.stored();
        let (source, target) = if reversed {
            (other, from)
        } else {
            (from, other)
        };
        Ok(Self {
            id: Uuid::now_v7(),
            tenant_id,
            source,
            target,
            kind,
            created_by,
            created_at: now,
        })
    }

    /// Rebuilds a link from storage.
    #[must_use]
    pub const fn restore(
        id: Uuid,
        tenant_id: TenantId,
        source: TicketId,
        target: TicketId,
        kind: LinkKind,
        created_by: Actor,
        created_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            tenant_id,
            source,
            target,
            kind,
            created_by,
            created_at,
        }
    }

    /// ID.
    #[must_use]
    pub const fn id(&self) -> Uuid {
        self.id
    }
    /// Company.
    #[must_use]
    pub const fn tenant_id(&self) -> TenantId {
        self.tenant_id
    }
    /// Stored source.
    #[must_use]
    pub const fn source(&self) -> TicketId {
        self.source
    }
    /// Stored target.
    #[must_use]
    pub const fn target(&self) -> TicketId {
        self.target
    }
    /// Stored kind.
    #[must_use]
    pub const fn kind(&self) -> LinkKind {
        self.kind
    }
    /// Who linked the tickets.
    #[must_use]
    pub const fn created_by(&self) -> Actor {
        self.created_by
    }
    /// When.
    #[must_use]
    pub const fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }
    /// The ticket at the other end, seen from `from`.
    #[must_use]
    pub fn other(&self, from: TicketId) -> TicketId {
        if self.source == from {
            self.target
        } else {
            self.source
        }
    }
    /// Whether `ticket` is one of the ends.
    #[must_use]
    pub fn involves(&self, ticket: TicketId) -> bool {
        self.source == ticket || self.target == ticket
    }
}

/// Fails if adding `link` would make a parent/child cycle. `ancestors_of_parent` are
/// the stored parent chain of the new link's parent (its parent, grandparent, …).
///
/// # Errors
///
/// [`TicketError::Invalid`] when the child is already an ancestor of the parent.
pub fn check_no_cycle(
    link: &TicketLink,
    ancestors_of_parent: &[TicketId],
) -> Result<(), TicketError> {
    if link.kind == LinkKind::Parent && ancestors_of_parent.contains(&link.target) {
        return Err(TicketError::invalid(
            "relation",
            "would make a ticket its own ancestor",
        ));
    }
    Ok(())
}

impl Audited for TicketLink {
    fn audit_type(&self) -> &'static str {
        "ticket_link"
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
            ("kind", json!(self.kind.as_str())),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(relation: LinkRelation) -> (TicketId, TicketId, TicketLink) {
        let (a, b) = (TicketId::new(), TicketId::new());
        let l = TicketLink::new(
            TenantId::new(),
            a,
            b,
            relation,
            Actor::Automation,
            Utc::now(),
        )
        .unwrap();
        (a, b, l)
    }

    #[test]
    fn child_of_is_stored_as_the_inverse_parent_link() {
        let (a, b, l) = link(LinkRelation::ChildOf);
        assert_eq!((l.source(), l.target(), l.kind()), (b, a, LinkKind::Parent));
        assert_eq!(LinkRelation::of(&l, a), LinkRelation::ChildOf);
        assert_eq!(LinkRelation::of(&l, b), LinkRelation::ParentOf);
        assert_eq!(l.other(a), b);
    }

    #[test]
    fn every_relation_reads_back_from_both_ends() {
        for (relation, inverse) in [
            (LinkRelation::Related, LinkRelation::Related),
            (LinkRelation::DuplicateOf, LinkRelation::DuplicatedBy),
            (LinkRelation::DuplicatedBy, LinkRelation::DuplicateOf),
            (LinkRelation::ParentOf, LinkRelation::ChildOf),
            (LinkRelation::ChildOf, LinkRelation::ParentOf),
        ] {
            let (a, b, l) = link(relation);
            assert_eq!(LinkRelation::of(&l, a), relation);
            assert_eq!(LinkRelation::of(&l, b), inverse);
            assert_eq!(relation.as_str().parse::<LinkRelation>(), Ok(relation));
        }
    }

    #[test]
    fn self_links_and_cycles_are_refused() {
        let a = TicketId::new();
        assert!(
            TicketLink::new(
                TenantId::new(),
                a,
                a,
                LinkRelation::Related,
                Actor::Automation,
                Utc::now()
            )
            .is_err()
        );
        // a is parent of b; b's ancestors include a's child? Make b the parent of a's parent.
        let (parent, child, l) = link(LinkRelation::ParentOf);
        assert!(check_no_cycle(&l, &[]).is_ok());
        assert!(
            check_no_cycle(&l, &[TicketId::new(), child]).is_err(),
            "the child is already above the parent"
        );
        let _ = parent;
    }
}
