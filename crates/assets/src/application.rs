//! Assets use cases (ADR-0034).

use std::sync::Arc;

use chrono::{DateTime, NaiveDate, Utc};
use cooper_kernel::{
    Actor, AuditAction, AuditRecord, Clock, DomainEvent, EventEnvelope, Forbidden, Principal,
    TenantId,
};
use thiserror::Error;
use uuid::Uuid;

use crate::domain::{
    Asset, AssetCheckedIn, AssetCheckedOut, AssetCheckout, AssetDetails, AssetError, AssetHolder,
    AssetId, AssetRetired, Lookup, LookupInput, LookupKind, References, location_subtree,
};
use crate::ports::{
    AssetLink, AssetQuery, AssetRepository, AssetSummary, Directory, LinkKind, RepoError,
};

/// Capabilities Assets checks (FR-IAM-010).
pub mod capabilities {
    /// See Assets.
    pub const READ: &str = "assets.read";
    /// Register Assets.
    pub const CREATE: &str = "assets.create";
    /// Edit, Check Out / Check In, Retire and link Assets.
    pub const UPDATE: &str = "assets.update";
    /// Configure Asset Types, Asset Statuses and Locations.
    pub const CONFIGURE: &str = "assets.configure";

    /// Every Capability declared by Assets.
    pub const ALL: &[&str] = &[READ, CREATE, UPDATE, CONFIGURE];
}

/// A use case failed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AppError {
    /// A business rule was violated.
    #[error(transparent)]
    Rule(#[from] AssetError),
    /// The caller lacks a Capability.
    #[error(transparent)]
    Forbidden(#[from] Forbidden),
    /// Not found or not visible.
    #[error("{0} not found")]
    NotFound(&'static str),
    /// A uniqueness rule was violated or the record changed meanwhile.
    #[error("{0}")]
    Conflict(String),
    /// Unexpected failure; logged, not returned.
    #[error("internal error")]
    Internal,
}

impl From<RepoError> for AppError {
    fn from(err: RepoError) -> Self {
        match err {
            RepoError::Conflict(c) if c == "assets_asset_tag_key" => {
                Self::Conflict("an Asset with this Asset Tag already exists in the Company".into())
            }
            RepoError::Conflict(c) if c.ends_with("_name_key") => {
                Self::Conflict("a row with this name already exists".into())
            }
            RepoError::Conflict(c) if c == "asset_links_pkey" => {
                Self::Conflict("already linked".into())
            }
            RepoError::Conflict(c) if c == "asset_checkouts_one_open" => {
                Self::Conflict("the Asset is already checked out".into())
            }
            RepoError::Conflict(_) => {
                Self::Conflict("the record conflicts with an existing one".into())
            }
            RepoError::Concurrent(what) => Self::Conflict(what),
            RepoError::MissingReference(c) => Self::Rule(AssetError::invalid(
                "reference",
                format!("refers to a record that does not exist ({c})"),
            )),
            RepoError::NotFound => Self::NotFound("record"),
            RepoError::Storage(message) => {
                tracing::error!(error = %message, "assets storage error");
                Self::Internal
            }
        }
    }
}

/// Dependencies.
#[derive(Clone)]
pub struct AssetPorts {
    /// Storage.
    pub assets: Arc<dyn AssetRepository>,
    /// Requesters, Analysts, Cost Centres, Suppliers, tickets and Contracts.
    pub directory: Arc<dyn Directory>,
    /// Time.
    pub clock: Arc<dyn Clock>,
}

/// A Check Out request.
#[derive(Debug, Clone)]
pub struct CheckOutRequest {
    /// Who receives the Asset.
    pub holder: AssetHolder,
    /// Note.
    pub note: String,
    /// Expected return.
    pub due_back_on: Option<NaiveDate>,
}

/// A Retire request.
#[derive(Debug, Clone)]
pub struct RetireRequest {
    /// The day the Asset left the estate (default today).
    pub retired_on: Option<NaiveDate>,
    /// Why.
    pub reason: String,
    /// An `is_retired` Asset Status to move to.
    pub asset_status_id: Option<Uuid>,
}

/// Assets use cases.
#[derive(Clone)]
pub struct AssetService {
    ports: AssetPorts,
}

fn require(principal: &Principal, capability: &str) -> Result<(), AppError> {
    if matches!(principal.actor(), Actor::Requester(_)) {
        return Err(Forbidden("staff".into()).into());
    }
    Ok(principal.require(capability)?)
}

fn event<E: DomainEvent>(
    e: &E,
    actor: Actor,
    now: DateTime<Utc>,
) -> Result<EventEnvelope, AppError> {
    EventEnvelope::wrap(e, actor, now).map_err(|err| {
        tracing::error!(error = %err, "cannot serialize domain event");
        AppError::Internal
    })
}

fn asset_audit(
    before: Option<&Asset>,
    after: &Asset,
    actor: Actor,
    now: DateTime<Utc>,
) -> AuditRecord {
    let action = if before.is_some() {
        AuditAction::Updated
    } else {
        AuditAction::Created
    };
    let old = before.map(Asset::audit_fields).unwrap_or_default();
    after.audit_fields().into_iter().fold(
        AuditRecord::new(
            Some(after.tenant_id()),
            "asset",
            after.id().as_uuid(),
            action,
            actor,
            now,
        ),
        |r, (f, v)| {
            let old = old.iter().find(|(n, _)| *n == f).map(|(_, v)| v);
            if before.is_none() && v.is_null() {
                r
            } else {
                r.change(f, old, Some(&v))
            }
        },
    )
}

fn note(asset: &Asset, actor: Actor, now: DateTime<Utc>) -> AuditRecord {
    AuditRecord::new(
        Some(asset.tenant_id()),
        "asset",
        asset.id().as_uuid(),
        AuditAction::Updated,
        actor,
        now,
    )
}

impl AssetService {
    /// Creates the service.
    #[must_use]
    pub const fn new(ports: AssetPorts) -> Self {
        Self { ports }
    }

    // --- Reference data (FR-AST-002) -----------------------------------------------------

    /// Reference data of a kind in display order (needs `assets.read`).
    ///
    /// # Errors
    ///
    /// Forbidden.
    pub async fn lookups(
        &self,
        principal: &Principal,
        kind: LookupKind,
    ) -> Result<Vec<Lookup>, AppError> {
        require(principal, capabilities::READ)?;
        let mut rows = self.ports.assets.lookups(kind).await?;
        rows.sort_by(|a, b| (a.sort_order, &a.name).cmp(&(b.sort_order, &b.name)));
        Ok(rows)
    }

    async fn lookup(&self, kind: LookupKind, id: Uuid) -> Result<Lookup, AppError> {
        self.ports
            .assets
            .lookups(kind)
            .await?
            .into_iter()
            .find(|l| l.id == id)
            .ok_or(AppError::NotFound(kind.label()))
    }

    /// Creates an Asset Type, Asset Status or Location (needs `assets.configure`).
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
        let all = self.ports.assets.lookups(kind).await?;
        let l = Lookup::create(kind, input, &all, now)?;
        let audit = AuditRecord::new(
            None,
            kind.aggregate_type(),
            l.id,
            AuditAction::Created,
            principal.actor(),
            now,
        )
        .change("name", None, Some(&l.name))
        .change("parent_id", None, l.parent_id.as_ref())
        .change("is_retired", None, Some(&l.is_retired))
        .change("is_active", None, Some(&l.is_active));
        self.ports.assets.save_lookup(&l, &audit).await?;
        Ok(l)
    }

    /// Changes an Asset Type, Asset Status or Location (needs `assets.configure`).
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
        let all = self.ports.assets.lookups(kind).await?;
        let mut l = all
            .iter()
            .find(|l| l.id == id)
            .cloned()
            .ok_or(AppError::NotFound(kind.label()))?;
        let before = l.clone();
        let now = self.ports.clock.now();
        l.apply(input, &all, now)?;
        let audit = AuditRecord::new(
            None,
            kind.aggregate_type(),
            id,
            AuditAction::Updated,
            principal.actor(),
            now,
        )
        .change("name", Some(&before.name), Some(&l.name))
        .change("parent_id", before.parent_id.as_ref(), l.parent_id.as_ref())
        .change("is_retired", Some(&before.is_retired), Some(&l.is_retired))
        .change("colour", before.colour.as_ref(), l.colour.as_ref())
        .change("sort_order", Some(&before.sort_order), Some(&l.sort_order))
        .change("is_active", Some(&before.is_active), Some(&l.is_active));
        self.ports.assets.save_lookup(&l, &audit).await?;
        Ok(l)
    }

    // --- Assets (FR-AST-001) -------------------------------------------------------------

    /// Resolves the reference data of `d` and checks the other contexts' references.
    async fn resolve(
        &self,
        tenant: TenantId,
        d: &AssetDetails,
        before: Option<&AssetDetails>,
    ) -> Result<(Option<Lookup>, Option<Lookup>, Option<Lookup>), AppError> {
        let find = |kind, id: Option<Uuid>| async move {
            match id {
                Some(id) => Ok::<_, AppError>(
                    self.ports
                        .assets
                        .lookups(kind)
                        .await?
                        .into_iter()
                        .find(|l| l.id == id),
                ),
                None => Ok(None),
            }
        };
        let t = find(LookupKind::Type, d.asset_type_id).await?;
        let s = find(LookupKind::Status, d.asset_status_id).await?;
        let l = find(LookupKind::Location, d.location_id).await?;
        if let Some(c) = d.cost_centre_id
            && before.and_then(|b| b.cost_centre_id) != Some(c)
            && !self.ports.directory.cost_centre_in(tenant, c).await?
        {
            return Err(AssetError::invalid(
                "cost_centre_id",
                "is not a Cost Centre of the Asset's Company",
            )
            .into());
        }
        if let Some(s) = d.supplier_id
            && before.and_then(|b| b.supplier_id) != Some(s)
            && !self.ports.directory.supplier_exists(tenant, s).await?
        {
            return Err(AssetError::invalid("supplier_id", "is not a Supplier").into());
        }
        Ok((t, s, l))
    }

    /// Registers an Asset in a Company the caller can see (needs `assets.create`).
    ///
    /// # Errors
    ///
    /// Forbidden, an invisible Company, invalid input or a duplicate Asset Tag.
    pub async fn create(
        &self,
        principal: &Principal,
        tenant: TenantId,
        details: AssetDetails,
    ) -> Result<Asset, AppError> {
        require(principal, capabilities::CREATE)?;
        if !principal.scope().allows(tenant) {
            return Err(AppError::NotFound("Company"));
        }
        let (t, s, l) = self.resolve(tenant, &details, None).await?;
        let refs = References {
            asset_type: t.as_ref(),
            status: s.as_ref(),
            location: l.as_ref(),
        };
        let now = self.ports.clock.now();
        let asset = Asset::register(tenant, details, refs, principal.actor(), now)?;
        let d = asset.details();
        let events = if d.warranty_expires_on.is_some() || d.lease_expires_on.is_some() {
            vec![event(&asset.dates_changed(), principal.actor(), now)?]
        } else {
            Vec::new()
        };
        let audit = asset_audit(None, &asset, principal.actor(), now);
        self.ports.assets.insert(&asset, &audit, &events).await?;
        Ok(asset)
    }

    /// One Asset (needs `assets.read`).
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn get(&self, principal: &Principal, id: AssetId) -> Result<Asset, AppError> {
        require(principal, capabilities::READ)?;
        self.ports
            .assets
            .get(principal.scope(), id)
            .await?
            .ok_or(AppError::NotFound("Asset"))
    }

    /// Assets, newest first; retired Assets only when asked for (needs `assets.read`).
    /// A Location filter includes the Locations below it.
    ///
    /// # Errors
    ///
    /// Forbidden.
    pub async fn list(
        &self,
        principal: &Principal,
        query: &AssetQuery,
    ) -> Result<Vec<Asset>, AppError> {
        require(principal, capabilities::READ)?;
        let mut q = query.clone();
        if let Some(roots) = &query.locations {
            let all = self.ports.assets.lookups(LookupKind::Location).await?;
            q.locations = Some(
                roots
                    .iter()
                    .flat_map(|r| location_subtree(&all, *r))
                    .collect(),
            );
        }
        Ok(self.ports.assets.list(principal.scope(), &q).await?)
    }

    /// Counts of the caller's estate (or one Company's): active Assets per type and
    /// status, retired Assets apart (needs `assets.read`; FR-AST-005).
    ///
    /// # Errors
    ///
    /// Forbidden, or a Company outside the caller's scope.
    pub async fn summary(
        &self,
        principal: &Principal,
        company: Option<TenantId>,
    ) -> Result<AssetSummary, AppError> {
        require(principal, capabilities::READ)?;
        if company.is_some_and(|c| !principal.scope().allows(c)) {
            return Err(AppError::NotFound("Company"));
        }
        Ok(self
            .ports
            .assets
            .summary(principal.scope(), company)
            .await?)
    }

    /// Edits an Asset (needs `assets.update`).
    ///
    /// # Errors
    ///
    /// Forbidden, not found, invalid input or a duplicate Asset Tag.
    pub async fn edit(
        &self,
        principal: &Principal,
        id: AssetId,
        details: AssetDetails,
    ) -> Result<Asset, AppError> {
        require(principal, capabilities::UPDATE)?;
        let mut asset = self.get(principal, id).await?;
        let before = asset.clone();
        let (t, s, l) = self
            .resolve(asset.tenant_id(), &details, Some(before.details()))
            .await?;
        let refs = References {
            asset_type: t.as_ref(),
            status: s.as_ref(),
            location: l.as_ref(),
        };
        let now = self.ports.clock.now();
        let edited = asset.edit(details, refs, now)?;
        let events = if edited.dates_changed {
            vec![event(&asset.dates_changed(), principal.actor(), now)?]
        } else {
            Vec::new()
        };
        let audit = asset_audit(Some(&before), &asset, principal.actor(), now);
        self.ports.assets.update(&asset, &audit, &events).await?;
        Ok(asset)
    }

    /// Checks an Asset out to a Requester of its Company or an active Analyst (needs
    /// `assets.update`; FR-AST-004).
    ///
    /// # Errors
    ///
    /// Forbidden, not found, an unknown Holder, a retired or already checked-out Asset.
    pub async fn check_out(
        &self,
        principal: &Principal,
        id: AssetId,
        request: CheckOutRequest,
    ) -> Result<(Asset, AssetCheckout), AppError> {
        require(principal, capabilities::UPDATE)?;
        let mut asset = self.get(principal, id).await?;
        let known = match request.holder {
            AssetHolder::Requester(r) => {
                self.ports
                    .directory
                    .requester_in(asset.tenant_id(), r)
                    .await?
            }
            AssetHolder::Analyst(a) => self.ports.directory.analyst_active(a).await?,
        };
        if !known {
            return Err(AssetError::invalid(
                "holder_id",
                "must be an active Requester of the Asset's Company or an active Analyst",
            )
            .into());
        }
        let before = asset.clone();
        let now = self.ports.clock.now();
        let checkout = asset
            .check_out(
                request.holder,
                &request.note,
                request.due_back_on,
                principal.actor(),
                now,
            )?
            .clone();
        let events = vec![event(
            &AssetCheckedOut {
                asset_id: asset.id(),
                tenant_id: asset.tenant_id(),
                checkout_id: checkout.id,
                holder_kind: checkout.holder.kind().to_owned(),
                holder_id: checkout.holder.id(),
                due_back_on: checkout.due_back_on,
            },
            principal.actor(),
            now,
        )?];
        let audit = asset_audit(Some(&before), &asset, principal.actor(), now);
        self.ports
            .assets
            .check_out(&asset, &checkout, &audit, &events)
            .await?;
        Ok((asset, checkout))
    }

    /// Checks an Asset back in (needs `assets.update`; FR-AST-004).
    ///
    /// # Errors
    ///
    /// Forbidden, not found, or not checked out.
    pub async fn check_in(
        &self,
        principal: &Principal,
        id: AssetId,
        note_text: &str,
    ) -> Result<(Asset, AssetCheckout), AppError> {
        require(principal, capabilities::UPDATE)?;
        let mut asset = self.get(principal, id).await?;
        let before = asset.clone();
        let now = self.ports.clock.now();
        let closed = asset.check_in(note_text, principal.actor(), now)?;
        let events = vec![event(
            &AssetCheckedIn {
                asset_id: asset.id(),
                tenant_id: asset.tenant_id(),
                checkout_id: closed.id,
                holder_kind: closed.holder.kind().to_owned(),
                holder_id: closed.holder.id(),
            },
            principal.actor(),
            now,
        )?];
        let audit = asset_audit(Some(&before), &asset, principal.actor(), now);
        self.ports
            .assets
            .check_in(&asset, &closed, &audit, &events)
            .await?;
        Ok((asset, closed))
    }

    /// The Check Out log of an Asset, newest first (needs `assets.read`).
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn checkouts(
        &self,
        principal: &Principal,
        id: AssetId,
    ) -> Result<Vec<AssetCheckout>, AppError> {
        self.get(principal, id).await?;
        Ok(self.ports.assets.checkouts(principal.scope(), id).await?)
    }

    /// Retires an Asset (needs `assets.update`; FR-AST-005).
    ///
    /// # Errors
    ///
    /// Forbidden, not found, already retired, checked out, or invalid input.
    pub async fn retire(
        &self,
        principal: &Principal,
        id: AssetId,
        request: RetireRequest,
    ) -> Result<Asset, AppError> {
        require(principal, capabilities::UPDATE)?;
        let mut asset = self.get(principal, id).await?;
        let status = match request.asset_status_id {
            Some(s) => Some(self.lookup(LookupKind::Status, s).await?),
            None => None,
        };
        let before = asset.clone();
        let now = self.ports.clock.now();
        let retired_on = request.retired_on.unwrap_or_else(|| now.date_naive());
        asset.retire(
            retired_on,
            &request.reason,
            status.as_ref(),
            principal.actor(),
            now,
        )?;
        let reason = asset
            .retirement()
            .map(|r| r.reason.clone())
            .unwrap_or_default();
        let events = vec![
            event(
                &AssetRetired {
                    asset_id: asset.id(),
                    tenant_id: asset.tenant_id(),
                    retired_on,
                    reason,
                },
                principal.actor(),
                now,
            )?,
            event(&asset.dates_changed(), principal.actor(), now)?,
        ];
        let audit = asset_audit(Some(&before), &asset, principal.actor(), now);
        self.ports.assets.retire(&asset, &audit, &events).await?;
        Ok(asset)
    }

    // --- Links and history (FR-AST-006) --------------------------------------------------

    /// Links with the target's number, leaving out records the caller cannot see.
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn links(
        &self,
        principal: &Principal,
        id: AssetId,
    ) -> Result<Vec<(AssetLink, String)>, AppError> {
        let asset = self.get(principal, id).await?;
        let mut out = Vec::new();
        for l in self.ports.assets.links(principal.scope(), id).await? {
            if let Some(label) = self
                .ports
                .directory
                .link_target(principal, asset.tenant_id(), l.kind, l.target_id)
                .await?
            {
                out.push((l, label));
            }
        }
        Ok(out)
    }

    /// Links a ticket or Contract of the Asset's Company (needs `assets.update`).
    ///
    /// # Errors
    ///
    /// Forbidden, not found (or another Company's), or already linked.
    pub async fn link(
        &self,
        principal: &Principal,
        id: AssetId,
        kind: LinkKind,
        target: Uuid,
    ) -> Result<(AssetLink, String), AppError> {
        require(principal, capabilities::UPDATE)?;
        let asset = self.get(principal, id).await?;
        let label = self
            .ports
            .directory
            .link_target(principal, asset.tenant_id(), kind, target)
            .await?
            .ok_or(AppError::NotFound(kind.label()))?;
        let now = self.ports.clock.now();
        let l = AssetLink {
            kind,
            target_id: target,
            linked_by: principal.actor(),
            linked_at: now,
        };
        let audit = note(&asset, principal.actor(), now).change(
            format!("{}_linked", kind.as_str()),
            None,
            Some(&label),
        );
        self.ports.assets.add_link(&asset, &l, &audit).await?;
        Ok((l, label))
    }

    /// Removes a link (needs `assets.update`).
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn unlink(
        &self,
        principal: &Principal,
        id: AssetId,
        kind: LinkKind,
        target: Uuid,
    ) -> Result<(), AppError> {
        require(principal, capabilities::UPDATE)?;
        let asset = self.get(principal, id).await?;
        let audit = note(&asset, principal.actor(), self.ports.clock.now()).change(
            format!("{}_unlinked", kind.as_str()),
            None,
            Some(&target),
        );
        self.ports
            .assets
            .remove_link(&asset, kind, target, &audit)
            .await?;
        Ok(())
    }

    /// The audit trail of an Asset: edits, Check Outs, Check Ins, retirement and links
    /// (needs `assets.read`).
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn history(
        &self,
        principal: &Principal,
        id: AssetId,
    ) -> Result<Vec<cooper_platform::audit::AuditEntry>, AppError> {
        self.get(principal, id).await?;
        Ok(self.ports.assets.history(principal.scope(), id).await?)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeSet, HashMap};
    use std::sync::Mutex;

    use async_trait::async_trait;
    use cooper_kernel::{AnalystId, Capability, FixedClock, RequesterId, TenantScope};

    use super::*;
    use crate::domain::Retirement;

    #[derive(Default)]
    struct Store {
        lookups: Vec<Lookup>,
        assets: HashMap<AssetId, Asset>,
        checkouts: Vec<(AssetId, AssetCheckout)>,
        links: Vec<(AssetId, AssetLink)>,
        audits: Vec<AuditRecord>,
        events: Vec<EventEnvelope>,
    }

    #[derive(Default)]
    struct FakeRepo(Mutex<Store>);

    impl FakeRepo {
        fn with<T>(&self, f: impl FnOnce(&mut Store) -> T) -> T {
            f(&mut self.0.lock().unwrap())
        }
        fn record(&self, audit: &AuditRecord, events: &[EventEnvelope]) {
            self.with(|s| {
                s.audits.push(audit.clone());
                s.events.extend_from_slice(events);
            });
        }
        fn event_types(&self) -> Vec<String> {
            self.with(|s| s.events.iter().map(|e| e.event_type.clone()).collect())
        }
    }

    #[async_trait]
    impl AssetRepository for FakeRepo {
        async fn lookups(&self, kind: LookupKind) -> Result<Vec<Lookup>, RepoError> {
            Ok(self.with(|s| {
                s.lookups
                    .iter()
                    .filter(|l| l.kind == kind)
                    .cloned()
                    .collect()
            }))
        }
        async fn save_lookup(&self, l: &Lookup, _: &AuditRecord) -> Result<(), RepoError> {
            self.with(|s| {
                if s.lookups.iter().any(|o| {
                    o.kind == l.kind && o.id != l.id && o.name.eq_ignore_ascii_case(&l.name)
                }) {
                    return Err(RepoError::Conflict("asset_types_name_key".into()));
                }
                s.lookups.retain(|o| o.id != l.id);
                s.lookups.push(l.clone());
                Ok(())
            })
        }
        async fn insert(
            &self,
            a: &Asset,
            audit: &AuditRecord,
            events: &[EventEnvelope],
        ) -> Result<(), RepoError> {
            self.with(|s| {
                let tag = a.details().asset_tag.as_deref().map(str::to_lowercase);
                if tag.is_some()
                    && s.assets.values().any(|o| {
                        o.tenant_id() == a.tenant_id()
                            && o.details().asset_tag.as_deref().map(str::to_lowercase) == tag
                    })
                {
                    return Err(RepoError::Conflict("assets_asset_tag_key".into()));
                }
                s.assets.insert(a.id(), a.clone());
                Ok(())
            })?;
            self.record(audit, events);
            Ok(())
        }
        async fn update(
            &self,
            a: &Asset,
            audit: &AuditRecord,
            events: &[EventEnvelope],
        ) -> Result<(), RepoError> {
            self.with(|s| s.assets.insert(a.id(), a.clone()));
            self.record(audit, events);
            Ok(())
        }
        async fn check_out(
            &self,
            a: &Asset,
            c: &AssetCheckout,
            audit: &AuditRecord,
            events: &[EventEnvelope],
        ) -> Result<(), RepoError> {
            self.with(|s| s.checkouts.push((a.id(), c.clone())));
            self.update(a, audit, events).await
        }
        async fn check_in(
            &self,
            a: &Asset,
            c: &AssetCheckout,
            audit: &AuditRecord,
            events: &[EventEnvelope],
        ) -> Result<(), RepoError> {
            self.with(|s| {
                for (_, o) in &mut s.checkouts {
                    if o.id == c.id {
                        *o = c.clone();
                    }
                }
            });
            self.update(a, audit, events).await
        }
        async fn retire(
            &self,
            a: &Asset,
            audit: &AuditRecord,
            events: &[EventEnvelope],
        ) -> Result<(), RepoError> {
            self.update(a, audit, events).await
        }
        async fn get(&self, scope: &TenantScope, id: AssetId) -> Result<Option<Asset>, RepoError> {
            Ok(self.with(|s| {
                s.assets
                    .get(&id)
                    .filter(|a| scope.allows(a.tenant_id()))
                    .cloned()
            }))
        }
        async fn list(&self, scope: &TenantScope, q: &AssetQuery) -> Result<Vec<Asset>, RepoError> {
            Ok(self.with(|s| {
                let mut v: Vec<Asset> = s
                    .assets
                    .values()
                    .filter(|a| scope.allows(a.tenant_id()))
                    .filter(|a| q.include_retired || a.is_active())
                    .filter(|a| {
                        q.locations.as_ref().is_none_or(|ls| {
                            a.details().location_id.is_some_and(|l| ls.contains(&l))
                        })
                    })
                    .cloned()
                    .collect();
                v.sort_by_key(|a| std::cmp::Reverse(a.id()));
                v
            }))
        }
        async fn summary(
            &self,
            scope: &TenantScope,
            _: Option<TenantId>,
        ) -> Result<AssetSummary, RepoError> {
            Ok(self.with(|s| {
                let visible: Vec<_> = s
                    .assets
                    .values()
                    .filter(|a| scope.allows(a.tenant_id()))
                    .collect();
                AssetSummary {
                    active: visible.iter().filter(|a| a.is_active()).count() as u64,
                    retired: visible.iter().filter(|a| !a.is_active()).count() as u64,
                    ..AssetSummary::default()
                }
            }))
        }
        async fn checkouts(
            &self,
            _: &TenantScope,
            id: AssetId,
        ) -> Result<Vec<AssetCheckout>, RepoError> {
            Ok(self.with(|s| {
                s.checkouts
                    .iter()
                    .rev()
                    .filter(|(a, _)| *a == id)
                    .map(|(_, c)| c.clone())
                    .collect()
            }))
        }
        async fn links(&self, _: &TenantScope, id: AssetId) -> Result<Vec<AssetLink>, RepoError> {
            Ok(self.with(|s| {
                s.links
                    .iter()
                    .filter(|(a, _)| *a == id)
                    .map(|(_, l)| l.clone())
                    .collect()
            }))
        }
        async fn add_link(
            &self,
            a: &Asset,
            l: &AssetLink,
            audit: &AuditRecord,
        ) -> Result<(), RepoError> {
            self.with(|s| {
                if s.links
                    .iter()
                    .any(|(x, o)| *x == a.id() && o.kind == l.kind && o.target_id == l.target_id)
                {
                    return Err(RepoError::Conflict("asset_links_pkey".into()));
                }
                s.links.push((a.id(), l.clone()));
                Ok(())
            })?;
            self.record(audit, &[]);
            Ok(())
        }
        async fn remove_link(
            &self,
            a: &Asset,
            kind: LinkKind,
            target: Uuid,
            audit: &AuditRecord,
        ) -> Result<(), RepoError> {
            let n = self.with(|s| {
                let before = s.links.len();
                s.links
                    .retain(|(x, o)| !(*x == a.id() && o.kind == kind && o.target_id == target));
                before - s.links.len()
            });
            if n == 0 {
                return Err(RepoError::NotFound);
            }
            self.record(audit, &[]);
            Ok(())
        }
        async fn history(
            &self,
            _: &TenantScope,
            _: AssetId,
        ) -> Result<Vec<cooper_platform::audit::AuditEntry>, RepoError> {
            Ok(Vec::new())
        }
    }

    struct FakeDirectory {
        tenant: TenantId,
        requester: RequesterId,
        analyst: AnalystId,
        cost_centre: Uuid,
        ticket: Uuid,
    }

    #[async_trait]
    impl Directory for FakeDirectory {
        async fn requester_in(&self, t: TenantId, id: RequesterId) -> Result<bool, RepoError> {
            Ok(t == self.tenant && id == self.requester)
        }
        async fn analyst_active(&self, id: AnalystId) -> Result<bool, RepoError> {
            Ok(id == self.analyst)
        }
        async fn cost_centre_in(&self, t: TenantId, id: Uuid) -> Result<bool, RepoError> {
            Ok(t == self.tenant && id == self.cost_centre)
        }
        async fn supplier_exists(&self, _: TenantId, _: Uuid) -> Result<bool, RepoError> {
            Ok(true)
        }
        async fn link_target(
            &self,
            _: &Principal,
            t: TenantId,
            kind: LinkKind,
            id: Uuid,
        ) -> Result<Option<String>, RepoError> {
            Ok(
                (t == self.tenant && kind == LinkKind::Ticket && id == self.ticket)
                    .then(|| "DEF-00001".to_owned()),
            )
        }
    }

    struct World {
        service: AssetService,
        repo: Arc<FakeRepo>,
        dir: Arc<FakeDirectory>,
        clock: Arc<FixedClock>,
    }

    fn world() -> World {
        let repo = Arc::new(FakeRepo::default());
        let dir = Arc::new(FakeDirectory {
            tenant: TenantId::new(),
            requester: RequesterId::new(),
            analyst: AnalystId::new(),
            cost_centre: Uuid::now_v7(),
            ticket: Uuid::now_v7(),
        });
        let clock = Arc::new(FixedClock::new("2026-11-11T09:00:00Z".parse().unwrap()));
        let service = AssetService::new(AssetPorts {
            assets: repo.clone(),
            directory: dir.clone(),
            clock: clock.clone(),
        });
        World {
            service,
            repo,
            dir,
            clock,
        }
    }

    fn analyst(caps: &[&'static str], scope: TenantScope) -> Principal {
        let caps: BTreeSet<Capability> = caps
            .iter()
            .filter_map(|c| Capability::from_static(c))
            .collect();
        Principal::new(Actor::Analyst(AnalystId::new()), false, caps, scope)
    }

    fn staff(w: &World) -> Principal {
        analyst(capabilities::ALL, TenantScope::single(w.dir.tenant))
    }

    fn tagged(tag: &str) -> AssetDetails {
        AssetDetails {
            asset_tag: Some(tag.into()),
            ..AssetDetails::default()
        }
    }

    #[tokio::test]
    async fn asset_tags_are_unique_per_company_and_capabilities_are_checked() {
        let w = world();
        let p = staff(&w);
        w.service
            .create(&p, w.dir.tenant, tagged("A-1"))
            .await
            .unwrap();
        let dup = w.service.create(&p, w.dir.tenant, tagged("a-1")).await;
        assert!(matches!(dup, Err(AppError::Conflict(_))), "{dup:?}");
        let reader = analyst(&[capabilities::READ], TenantScope::single(w.dir.tenant));
        assert!(matches!(
            w.service.create(&reader, w.dir.tenant, tagged("B")).await,
            Err(AppError::Forbidden(_))
        ));
        let elsewhere = analyst(capabilities::ALL, TenantScope::single(TenantId::new()));
        assert!(matches!(
            w.service
                .create(&elsewhere, w.dir.tenant, tagged("C"))
                .await,
            Err(AppError::NotFound(_))
        ));
        let requester = Principal::new(
            Actor::Requester(RequesterId::new()),
            false,
            BTreeSet::new(),
            TenantScope::All,
        );
        assert!(matches!(
            w.service.lookups(&requester, LookupKind::Type).await,
            Err(AppError::Forbidden(_))
        ));
    }

    #[tokio::test]
    async fn references_to_other_contexts_are_checked() {
        let w = world();
        let p = staff(&w);
        let bad = AssetDetails {
            cost_centre_id: Some(Uuid::now_v7()),
            ..tagged("A")
        };
        assert!(matches!(
            w.service.create(&p, w.dir.tenant, bad).await,
            Err(AppError::Rule(AssetError::Invalid {
                field: "cost_centre_id",
                ..
            }))
        ));
        let ok = AssetDetails {
            cost_centre_id: Some(w.dir.cost_centre),
            supplier_id: Some(Uuid::now_v7()),
            ..tagged("A")
        };
        w.service.create(&p, w.dir.tenant, ok).await.unwrap();
        let unknown_type = AssetDetails {
            asset_type_id: Some(Uuid::now_v7()),
            ..tagged("B")
        };
        assert!(
            w.service
                .create(&p, w.dir.tenant, unknown_type)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn expiry_dates_publish_dates_changed_and_audit_the_change() {
        let w = world();
        let p = staff(&w);
        let a = w
            .service
            .create(&p, w.dir.tenant, tagged("A"))
            .await
            .unwrap();
        assert!(w.repo.event_types().is_empty(), "no dates, no event");
        let with_warranty = AssetDetails {
            warranty_expires_on: Some("2028-01-31".parse().unwrap()),
            ..a.details().clone()
        };
        w.service
            .edit(&p, a.id(), with_warranty.clone())
            .await
            .unwrap();
        assert_eq!(w.repo.event_types(), ["asset.dates_changed"]);
        w.service.edit(&p, a.id(), with_warranty).await.unwrap();
        assert_eq!(w.repo.event_types().len(), 1, "unchanged dates, no event");
        let audit = w.repo.with(|s| s.audits[1].clone());
        assert_eq!(audit.action, AuditAction::Updated);
        assert_eq!(audit.changes[0].field, "warranty_expires_on");
        let created = w
            .service
            .create(
                &p,
                w.dir.tenant,
                AssetDetails {
                    lease_expires_on: Some("2027-01-01".parse().unwrap()),
                    ..tagged("B")
                },
            )
            .await
            .unwrap();
        let ev = w.repo.with(|s| s.events.last().cloned().unwrap());
        assert_eq!(ev.event_type, "asset.dates_changed");
        assert_eq!(ev.tenant_id, Some(w.dir.tenant));
        assert_eq!(ev.payload["asset_id"], serde_json::json!(created.id()));
        assert_eq!(ev.payload["lease_expires_on"], "2027-01-01");
        assert_eq!(ev.payload["active"], true);
    }

    #[tokio::test]
    async fn check_out_and_in_to_known_holders_with_a_log() {
        let w = world();
        let p = staff(&w);
        let a = w
            .service
            .create(&p, w.dir.tenant, tagged("A"))
            .await
            .unwrap();
        let stranger = CheckOutRequest {
            holder: AssetHolder::Requester(RequesterId::new()),
            note: String::new(),
            due_back_on: None,
        };
        assert!(w.service.check_out(&p, a.id(), stranger).await.is_err());
        let to_rita = CheckOutRequest {
            holder: AssetHolder::Requester(w.dir.requester),
            note: "new starter".into(),
            due_back_on: None,
        };
        let (held, c) = w
            .service
            .check_out(&p, a.id(), to_rita.clone())
            .await
            .unwrap();
        assert_eq!(held.holder(), Some(AssetHolder::Requester(w.dir.requester)));
        assert!(matches!(
            w.service.check_out(&p, a.id(), to_rita).await,
            Err(AppError::Rule(AssetError::InvalidState(_)))
        ));
        w.clock.advance(chrono::Duration::days(30));
        let (back, closed) = w.service.check_in(&p, a.id(), "returned").await.unwrap();
        assert_eq!(closed.id, c.id);
        assert!(back.holder().is_none());
        let to_ann = CheckOutRequest {
            holder: AssetHolder::Analyst(w.dir.analyst),
            note: String::new(),
            due_back_on: None,
        };
        w.service.check_out(&p, a.id(), to_ann).await.unwrap();
        let log = w.service.checkouts(&p, a.id()).await.unwrap();
        assert_eq!(log.len(), 2);
        assert_eq!(log[1].checkin_note, "returned");
        assert!(log[0].checked_in_at.is_none());
        assert_eq!(
            w.repo.event_types(),
            ["asset.checked_out", "asset.checked_in", "asset.checked_out"]
        );
        let audit = w.repo.with(|s| s.audits[1].clone());
        assert_eq!(audit.changes[0].field, "holder");
    }

    #[tokio::test]
    async fn retiring_excludes_the_asset_and_publishes_events() {
        let w = world();
        let p = staff(&w);
        let gone = w
            .service
            .create_lookup(
                &p,
                LookupKind::Status,
                LookupInput {
                    name: "Disposed".into(),
                    is_retired: true,
                    is_active: true,
                    ..LookupInput::default()
                },
            )
            .await
            .unwrap();
        let a = w
            .service
            .create(&p, w.dir.tenant, tagged("A"))
            .await
            .unwrap();
        w.service
            .create(&p, w.dir.tenant, tagged("B"))
            .await
            .unwrap();
        let r = w
            .service
            .retire(
                &p,
                a.id(),
                RetireRequest {
                    retired_on: None,
                    reason: "End of life".into(),
                    asset_status_id: Some(gone.id),
                },
            )
            .await
            .unwrap();
        assert_eq!(
            r.retirement(),
            Some(&Retirement {
                retired_on: "2026-11-11".parse().unwrap(),
                reason: "End of life".into(),
                recorded_at: w.clock.now(),
                recorded_by: p.actor(),
            })
        );
        assert_eq!(
            w.repo.event_types(),
            ["asset.retired", "asset.dates_changed"]
        );
        let ev = w.repo.with(|s| s.events[1].clone());
        assert_eq!(ev.payload["active"], false);
        let active = w.service.list(&p, &AssetQuery::default()).await.unwrap();
        assert_eq!(active.len(), 1, "retired Assets are left out by default");
        let all = w
            .service
            .list(
                &p,
                &AssetQuery {
                    include_retired: true,
                    ..AssetQuery::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(all.len(), 2);
        let s = w.service.summary(&p, None).await.unwrap();
        assert_eq!((s.active, s.retired), (1, 1));
        assert!(matches!(
            w.service.summary(&p, Some(TenantId::new())).await,
            Err(AppError::NotFound(_))
        ));
        let again = CheckOutRequest {
            holder: AssetHolder::Analyst(w.dir.analyst),
            note: String::new(),
            due_back_on: None,
        };
        assert!(w.service.check_out(&p, a.id(), again).await.is_err());
    }

    #[tokio::test]
    async fn location_filters_include_sub_locations() {
        let w = world();
        let p = staff(&w);
        let loc = |name: &str, parent: Option<Uuid>| LookupInput {
            name: name.into(),
            parent_id: parent,
            is_active: true,
            ..LookupInput::default()
        };
        let site = w
            .service
            .create_lookup(&p, LookupKind::Location, loc("London", None))
            .await
            .unwrap();
        let room = w
            .service
            .create_lookup(&p, LookupKind::Location, loc("Room 1", Some(site.id)))
            .await
            .unwrap();
        assert!(
            w.service
                .update_lookup(
                    &p,
                    LookupKind::Location,
                    site.id,
                    loc("London", Some(room.id))
                )
                .await
                .is_err(),
            "no loops"
        );
        let in_room = AssetDetails {
            location_id: Some(room.id),
            ..tagged("A")
        };
        w.service.create(&p, w.dir.tenant, in_room).await.unwrap();
        w.service
            .create(&p, w.dir.tenant, tagged("B"))
            .await
            .unwrap();
        let q = AssetQuery {
            locations: Some(vec![site.id]),
            ..AssetQuery::default()
        };
        assert_eq!(w.service.list(&p, &q).await.unwrap().len(), 1);
        let reader = analyst(&[capabilities::READ], TenantScope::single(w.dir.tenant));
        assert!(matches!(
            w.service
                .create_lookup(&reader, LookupKind::Type, loc("Laptop", None))
                .await,
            Err(AppError::Forbidden(_))
        ));
    }

    #[tokio::test]
    async fn links_need_a_visible_target_of_the_same_company() {
        let w = world();
        let p = staff(&w);
        let a = w
            .service
            .create(&p, w.dir.tenant, tagged("A"))
            .await
            .unwrap();
        assert!(matches!(
            w.service
                .link(&p, a.id(), LinkKind::Ticket, Uuid::now_v7())
                .await,
            Err(AppError::NotFound(_))
        ));
        let (_, label) = w
            .service
            .link(&p, a.id(), LinkKind::Ticket, w.dir.ticket)
            .await
            .unwrap();
        assert_eq!(label, "DEF-00001");
        assert!(matches!(
            w.service
                .link(&p, a.id(), LinkKind::Ticket, w.dir.ticket)
                .await,
            Err(AppError::Conflict(_))
        ));
        assert_eq!(w.service.links(&p, a.id()).await.unwrap().len(), 1);
        w.service
            .unlink(&p, a.id(), LinkKind::Ticket, w.dir.ticket)
            .await
            .unwrap();
        assert!(w.service.links(&p, a.id()).await.unwrap().is_empty());
        let audit = w.repo.with(|s| s.audits.last().cloned().unwrap());
        assert_eq!(audit.changes[0].field, "ticket_unlinked");
    }
}
