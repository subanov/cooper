//! Builds audit records by comparing aggregate snapshots (FR-GEN-001).

use chrono::{DateTime, Utc};
use cooper_kernel::{Actor, AuditAction, AuditRecord, TenantId};
use serde_json::Value;
use uuid::Uuid;

/// An aggregate whose fields are audited.
pub trait Audited {
    /// Aggregate type name stored in the audit log, for example `analyst`.
    const AGGREGATE_TYPE: &'static str;

    /// The aggregate's ID.
    fn audit_id(&self) -> Uuid;

    /// The Company the record belongs to, if any.
    fn audit_tenant(&self) -> Option<TenantId>;

    /// Audited fields as `(name, value)`. Secrets (password hashes, tokens) must not be
    /// included.
    fn audit_fields(&self) -> Vec<(&'static str, Value)>;
}

/// Builds the audit record for a change from `before` to `after`. With no `before` the
/// action is `created` and every field is recorded as new.
#[must_use]
pub fn audit_record<A: Audited>(
    before: Option<&A>,
    after: &A,
    action: AuditAction,
    actor: Actor,
    now: DateTime<Utc>,
) -> AuditRecord {
    let mut record = AuditRecord::new(
        after.audit_tenant(),
        A::AGGREGATE_TYPE,
        after.audit_id(),
        action,
        actor,
        now,
    );
    let old_fields = before.map(Audited::audit_fields).unwrap_or_default();
    for (field, new) in after.audit_fields() {
        let old = old_fields
            .iter()
            .find(|(name, _)| *name == field)
            .map(|(_, v)| v);
        record = record.change(field, old, Some(&new));
    }
    record
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    struct Thing {
        id: Uuid,
        name: &'static str,
        size: u32,
    }

    impl Audited for Thing {
        const AGGREGATE_TYPE: &'static str = "thing";
        fn audit_id(&self) -> Uuid {
            self.id
        }
        fn audit_tenant(&self) -> Option<TenantId> {
            None
        }
        fn audit_fields(&self) -> Vec<(&'static str, Value)> {
            vec![("name", self.name.into()), ("size", self.size.into())]
        }
    }

    #[test]
    fn records_only_changed_fields() {
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let id = Uuid::now_v7();
        let before = Thing {
            id,
            name: "a",
            size: 1,
        };
        let after = Thing {
            id,
            name: "a",
            size: 2,
        };
        let rec = audit_record(
            Some(&before),
            &after,
            AuditAction::Updated,
            Actor::Automation,
            now,
        );
        assert_eq!(rec.changes.len(), 1);
        assert_eq!(rec.changes[0].field, "size");

        let created = audit_record(None, &after, AuditAction::Created, Actor::Automation, now);
        assert_eq!(created.changes.len(), 2);
    }
}
