//! In-memory storage for application and handler tests.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::{TimeZone, Utc};
use cooper_kernel::{
    Actor, AnalystId, AuditRecord, Capability, FixedClock, Principal, TenantId, TenantScope,
};
use uuid::Uuid;

use crate::application::{CalendarPorts, CalendarService};
use crate::domain::{
    CalendarCategory, CalendarCategoryId, CalendarEvent, CalendarEventId, SourceKind,
};
use crate::ports::{
    CalendarRepository, EventQuery, GeneratedState, GeneratedWrite, RepoError, Watermark,
};

#[derive(Default)]
pub struct Data {
    pub categories: Vec<CalendarCategory>,
    pub events: Vec<CalendarEvent>,
    pub sources: Vec<((TenantId, SourceKind, Uuid), Watermark)>,
    pub audits: Vec<AuditRecord>,
}

#[derive(Default, Clone)]
pub struct FakeStore(pub Arc<Mutex<Data>>);

impl FakeStore {
    pub fn data(&self) -> std::sync::MutexGuard<'_, Data> {
        self.0.lock().unwrap()
    }
}

#[async_trait]
impl CalendarRepository for FakeStore {
    async fn categories(&self) -> Result<Vec<CalendarCategory>, RepoError> {
        Ok(self.data().categories.clone())
    }

    async fn category(
        &self,
        id: CalendarCategoryId,
    ) -> Result<Option<CalendarCategory>, RepoError> {
        Ok(self
            .data()
            .categories
            .iter()
            .find(|c| c.id() == id)
            .cloned())
    }

    async fn save_category(
        &self,
        c: &CalendarCategory,
        audit: &AuditRecord,
    ) -> Result<(), RepoError> {
        let mut d = self.data();
        if d.categories
            .iter()
            .any(|o| o.id() != c.id() && o.name().eq_ignore_ascii_case(c.name()))
        {
            return Err(RepoError::Conflict("calendar_categories_name_key".into()));
        }
        d.categories.retain(|o| o.id() != c.id());
        d.categories.push(c.clone());
        d.audits.push(audit.clone());
        Ok(())
    }

    async fn ensure_system_category(
        &self,
        candidate: &CalendarCategory,
        audit: &AuditRecord,
    ) -> Result<CalendarCategory, RepoError> {
        let mut d = self.data();
        if let Some(c) = d.categories.iter().find(|c| c.key() == candidate.key()) {
            return Ok(c.clone());
        }
        d.categories.push(candidate.clone());
        d.audits.push(audit.clone());
        Ok(candidate.clone())
    }

    async fn events(
        &self,
        scope: &TenantScope,
        q: &EventQuery,
    ) -> Result<Vec<CalendarEvent>, RepoError> {
        Ok(self
            .data()
            .events
            .iter()
            .filter(|e| scope.allows(e.tenant_id()))
            .filter(|e| q.tenant_id.is_none_or(|t| t == e.tenant_id()))
            .filter(|e| q.category_id.is_none_or(|c| c == e.details().category_id))
            .filter(|e| e.details().timing.overlaps(&q.range))
            .take(q.limit as usize)
            .cloned()
            .collect())
    }

    async fn event(
        &self,
        scope: &TenantScope,
        id: CalendarEventId,
    ) -> Result<Option<CalendarEvent>, RepoError> {
        Ok(self
            .data()
            .events
            .iter()
            .find(|e| e.id() == id && scope.allows(e.tenant_id()))
            .cloned())
    }

    async fn insert_event(&self, e: &CalendarEvent, audit: &AuditRecord) -> Result<(), RepoError> {
        let mut d = self.data();
        d.events.push(e.clone());
        d.audits.push(audit.clone());
        Ok(())
    }

    async fn update_event(&self, e: &CalendarEvent, audit: &AuditRecord) -> Result<(), RepoError> {
        let mut d = self.data();
        let slot = d
            .events
            .iter_mut()
            .find(|o| o.id() == e.id())
            .ok_or(RepoError::NotFound)?;
        *slot = e.clone();
        d.audits.push(audit.clone());
        Ok(())
    }

    async fn delete_event(&self, e: &CalendarEvent, audit: &AuditRecord) -> Result<(), RepoError> {
        let mut d = self.data();
        d.events.retain(|o| o.id() != e.id());
        d.audits.push(audit.clone());
        Ok(())
    }

    async fn generated(
        &self,
        tenant: TenantId,
        kind: SourceKind,
        source_id: Uuid,
    ) -> Result<GeneratedState, RepoError> {
        let d = self.data();
        Ok(GeneratedState {
            watermark: d
                .sources
                .iter()
                .find(|(k, _)| *k == (tenant, kind, source_id))
                .map(|(_, w)| *w),
            events: d
                .events
                .iter()
                .filter(|e| {
                    e.tenant_id() == tenant
                        && e.source()
                            .is_some_and(|s| s.kind == kind && s.id == source_id)
                })
                .cloned()
                .collect(),
        })
    }

    async fn apply_generated(&self, w: GeneratedWrite<'_>) -> Result<(), RepoError> {
        let mut d = self.data();
        let key = (w.tenant_id, w.kind, w.source_id);
        let current = d.sources.iter().find(|(k, _)| *k == key).map(|(_, v)| *v);
        if current != w.expected {
            return Err(RepoError::Conflict("moved".into()));
        }
        d.sources.retain(|(k, _)| *k != key);
        d.sources.push((key, w.watermark));
        for e in w.deletes {
            d.events.retain(|o| o.id() != e.id());
        }
        for e in w.updates {
            if let Some(slot) = d.events.iter_mut().find(|o| o.id() == e.id()) {
                *slot = e.clone();
            }
        }
        d.events.extend(w.inserts.iter().cloned());
        d.audits.extend(w.audits.iter().cloned());
        Ok(())
    }
}

pub fn service(store: &FakeStore) -> CalendarService {
    CalendarService::new(CalendarPorts {
        store: Arc::new(store.clone()),
        clock: Arc::new(FixedClock::new(
            Utc.with_ymd_and_hms(2026, 11, 11, 9, 0, 0).unwrap(),
        )),
    })
}

pub fn analyst(caps: &[&'static str], scope: TenantScope) -> Principal {
    Principal::new(
        Actor::Analyst(AnalystId::new()),
        false,
        caps.iter()
            .filter_map(|c| Capability::from_static(c))
            .collect(),
        scope,
    )
}
