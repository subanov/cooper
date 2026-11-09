//! Change Management use cases (ADR-0027).

use std::collections::BTreeSet;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use cooper_kernel::{
    Actor, AnalystId, AuditAction, AuditRecord, Clock, DomainEvent, EventEnvelope, Forbidden,
    Principal, TenantId,
};
use thiserror::Error;
use uuid::Uuid;

use crate::domain::{
    CabApprovalType, CabMember, Change, ChangeApproved, ChangeCompleted, ChangeDetails,
    ChangeError, ChangeRejected, ChangeScheduled, ChangeStage, ChangeSubmitted, ChangeTemplate,
    Deciders, Decision, DiffLine, LONG_TEXT_FIELDS, Lookup, LookupInput, LookupKind, Moved, Pir,
    RiskLevel, RoundOutcome, TemplateContent, Vote, Window, change_number, check_bands, diff_lines,
    overlaps, text,
};
use crate::ports::{
    ChangeAttachment, ChangeLink, ChangeQuery, ChangeRepository, ChecklistItem, Comment, Directory,
    LinkKind, RepoError, TextRevision,
};

/// Capabilities Change Management checks (FR-IAM-010).
pub mod capabilities {
    /// See Changes.
    pub const READ: &str = "changes.read";
    /// Create Changes.
    pub const CREATE: &str = "changes.create";
    /// Edit, move, comment, link, record PIRs.
    pub const UPDATE: &str = "changes.update";
    /// Vote on Changes (as Approver or CAB member).
    pub const APPROVE: &str = "changes.approve";
    /// Configure Types, Statuses, Priorities, Impacts, Categories, Risk Levels and
    /// Templates.
    pub const CONFIGURE: &str = "changes.configure";

    /// Every Capability declared by Change Management.
    pub const ALL: &[&str] = &[READ, CREATE, UPDATE, APPROVE, CONFIGURE];
}

/// A use case failed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AppError {
    /// A business rule was violated.
    #[error(transparent)]
    Rule(#[from] ChangeError),
    /// The caller lacks a Capability.
    #[error(transparent)]
    Forbidden(#[from] Forbidden),
    /// Not found or not visible.
    #[error("{0} not found")]
    NotFound(&'static str),
    /// A uniqueness rule was violated.
    #[error("{0}")]
    Conflict(String),
    /// Unexpected failure; logged, not returned.
    #[error("internal error")]
    Internal,
}

impl From<RepoError> for AppError {
    fn from(err: RepoError) -> Self {
        match err {
            RepoError::Conflict(c) if c.ends_with("_name_key") => {
                Self::Conflict("a row with this name already exists".into())
            }
            RepoError::Conflict(c) if c == "change_approval_votes_once" => {
                Self::Conflict("you already voted in this round".into())
            }
            RepoError::Conflict(c) if c == "change_links_pkey" => {
                Self::Conflict("already linked".into())
            }
            RepoError::Conflict(_) => {
                Self::Conflict("the record conflicts with an existing one".into())
            }
            RepoError::MissingReference(c) => Self::Rule(ChangeError::invalid(
                "reference",
                format!("refers to a record that does not exist ({c})"),
            )),
            RepoError::NotFound => Self::NotFound("record"),
            RepoError::Storage(message) => {
                tracing::error!(error = %message, "change management storage error");
                Self::Internal
            }
        }
    }
}

/// Dependencies.
#[derive(Clone)]
pub struct ChangePorts {
    /// Storage.
    pub changes: Arc<dyn ChangeRepository>,
    /// Teams, tickets and Problems.
    pub directory: Arc<dyn Directory>,
    /// Time.
    pub clock: Arc<dyn Clock>,
}

/// A new Change. Empty fields are filled from the template, when one is given.
#[derive(Debug, Clone, Default)]
pub struct NewChange {
    /// Company.
    pub tenant_id: Option<TenantId>,
    /// Details (nil IDs mean "default" or "from the template").
    pub details: ChangeDetails,
    /// Template to pre-fill from (FR-CHG-010).
    pub template_id: Option<Uuid>,
}

/// The Change Calendar for a period (FR-CHG-012).
#[derive(Debug, Clone)]
pub struct Calendar {
    /// Changes with a window in the period.
    pub changes: Vec<Change>,
    /// Pairs of Changes whose windows overlap.
    pub overlaps: Vec<(Uuid, Uuid)>,
}

/// The Change Management application service.
#[derive(Clone)]
pub struct ChangeService {
    ports: ChangePorts,
}

fn require(principal: &Principal, capability: &str) -> Result<(), AppError> {
    if matches!(principal.actor(), Actor::Requester(_)) {
        return Err(Forbidden("staff".into()).into());
    }
    Ok(principal.require(capability)?)
}

fn analyst(principal: &Principal) -> Option<AnalystId> {
    match principal.actor() {
        Actor::Analyst(a) => Some(a),
        _ => None,
    }
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

fn lookup_audit(
    l: &Lookup,
    before: Option<&Lookup>,
    actor: Actor,
    now: DateTime<Utc>,
) -> AuditRecord {
    let action = if before.is_some() {
        AuditAction::Updated
    } else {
        AuditAction::Created
    };
    let fields = |x: &Lookup| {
        vec![
            ("name", serde_json::json!(x.name)),
            ("stage", serde_json::json!(x.stage.as_str())),
            ("is_closed", serde_json::json!(x.is_closed)),
            ("approval_route", serde_json::json!(x.route.as_str())),
            ("is_standard", serde_json::json!(x.is_standard)),
            ("is_default", serde_json::json!(x.is_default)),
            ("is_active", serde_json::json!(x.is_active)),
        ]
    };
    let old = before.map(fields).unwrap_or_default();
    fields(l).into_iter().fold(
        AuditRecord::new(None, l.kind.table(), l.id, action, actor, now),
        |r, (f, v)| {
            r.change(
                f,
                old.iter().find(|(n, _)| *n == f).map(|(_, v)| v),
                Some(&v),
            )
        },
    )
}

fn change_audit(
    before: Option<&Change>,
    after: &Change,
    actor: Actor,
    now: DateTime<Utc>,
) -> AuditRecord {
    let action = if before.is_some() {
        AuditAction::Updated
    } else {
        AuditAction::Created
    };
    let old = before.map(Change::audit_fields).unwrap_or_default();
    after.audit_fields().into_iter().fold(
        AuditRecord::new(
            Some(after.tenant_id()),
            "change",
            after.id(),
            action,
            actor,
            now,
        ),
        |r, (f, v)| {
            r.change(
                f,
                old.iter().find(|(n, _)| *n == f).map(|(_, v)| v),
                Some(&v),
            )
        },
    )
}

fn revisions(
    before: Option<&ChangeDetails>,
    after: &ChangeDetails,
    author: Actor,
    now: DateTime<Utc>,
) -> Vec<TextRevision> {
    LONG_TEXT_FIELDS
        .iter()
        .filter_map(|f| {
            let new = after.long_text(f)?;
            let old = before.and_then(|b| b.long_text(f)).unwrap_or_default();
            (new != old && !(before.is_none() && new.is_empty())).then(|| TextRevision {
                id: Uuid::now_v7(),
                field: (*f).to_owned(),
                body: new.to_owned(),
                author,
                created_at: now,
            })
        })
        .collect()
}

impl ChangeService {
    /// Creates the service.
    #[must_use]
    pub const fn new(ports: ChangePorts) -> Self {
        Self { ports }
    }

    /// The current time.
    #[must_use]
    pub fn now(&self) -> DateTime<Utc> {
        self.ports.clock.now()
    }

    // --- Reference data ------------------------------------------------------------------

    /// Lookups of a kind in display order.
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
        let mut rows = self.ports.changes.lookups(kind).await?;
        rows.sort_by(|a, b| (a.sort_order, &a.name).cmp(&(b.sort_order, &b.name)));
        Ok(rows)
    }

    async fn lookup(&self, kind: LookupKind, id: Uuid) -> Result<Lookup, AppError> {
        self.ports
            .changes
            .lookups(kind)
            .await?
            .into_iter()
            .find(|l| l.id == id)
            .ok_or(AppError::NotFound(kind.label()))
    }

    async fn active_or_default(
        &self,
        kind: LookupKind,
        id: Uuid,
        field: &'static str,
    ) -> Result<Uuid, AppError> {
        let rows = self.ports.changes.lookups(kind).await?;
        let found = if id.is_nil() {
            rows.iter().find(|l| l.is_default && l.is_active)
        } else {
            rows.iter().find(|l| l.id == id)
        };
        match found {
            Some(l) if l.is_active => Ok(l.id),
            Some(_) => Err(ChangeError::invalid(field, "is not active").into()),
            None if id.is_nil() => {
                Err(ChangeError::invalid(field, "is required (no default is configured)").into())
            }
            None => Err(ChangeError::invalid(field, "does not exist").into()),
        }
    }

    async fn first_status(&self, stage: ChangeStage) -> Result<Lookup, AppError> {
        let mut rows: Vec<Lookup> = self
            .ports
            .changes
            .lookups(LookupKind::Status)
            .await?
            .into_iter()
            .filter(|s| {
                s.stage == stage
                    && s.is_active
                    && (stage != ChangeStage::Draft || s.is_default || !s.is_closed)
            })
            .collect();
        rows.sort_by_key(|s| (!s.is_default, s.sort_order));
        rows.into_iter().next().ok_or_else(|| {
            ChangeError::InvalidState(format!("no active {stage} status is configured")).into()
        })
    }

    /// Creates a lookup (needs `changes.configure`).
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
        let l = Lookup::create(kind, input, now)?;
        self.ports
            .changes
            .save_lookup(&l, &lookup_audit(&l, None, principal.actor(), now))
            .await?;
        Ok(l)
    }

    /// Changes a lookup (needs `changes.configure`). The default cannot be unset
    /// directly, and a status's Stage cannot change (Changes use it).
    ///
    /// # Errors
    ///
    /// Forbidden, not found or invalid input.
    pub async fn update_lookup(
        &self,
        principal: &Principal,
        kind: LookupKind,
        id: Uuid,
        input: LookupInput,
    ) -> Result<Lookup, AppError> {
        require(principal, capabilities::CONFIGURE)?;
        let mut l = self.lookup(kind, id).await?;
        if l.is_default && !input.is_default {
            return Err(
                ChangeError::invalid("is_default", "make another row the default instead").into(),
            );
        }
        if kind == LookupKind::Status && input.stage != l.stage {
            return Err(ChangeError::invalid(
                "stage",
                "cannot change; create a new status instead",
            )
            .into());
        }
        let before = l.clone();
        let now = self.ports.clock.now();
        l.apply(input, now)?;
        self.ports
            .changes
            .save_lookup(&l, &lookup_audit(&l, Some(&before), principal.actor(), now))
            .await?;
        Ok(l)
    }

    /// Risk Level bands (FR-CHG-005).
    ///
    /// # Errors
    ///
    /// Forbidden.
    pub async fn risk_bands(&self, principal: &Principal) -> Result<Vec<RiskLevel>, AppError> {
        require(principal, capabilities::READ)?;
        let mut bands = self.ports.changes.risk_bands().await?;
        bands.sort_by_key(|b| b.min_score);
        Ok(bands)
    }

    /// Replaces the Risk Level bands; they must cover 1–25 exactly once (needs
    /// `changes.configure`).
    ///
    /// # Errors
    ///
    /// Forbidden, or gaps and overlaps.
    pub async fn set_risk_bands(
        &self,
        principal: &Principal,
        mut bands: Vec<RiskLevel>,
    ) -> Result<Vec<RiskLevel>, AppError> {
        require(principal, capabilities::CONFIGURE)?;
        for b in &mut bands {
            b.name = text("name", &b.name, 50, true)?;
        }
        check_bands(&bands)?;
        let now = self.ports.clock.now();
        let describe = |bs: &[RiskLevel]| {
            bs.iter()
                .map(|b| format!("{} {}-{}", b.name, b.min_score, b.max_score))
                .collect::<Vec<_>>()
        };
        let old = self.ports.changes.risk_bands().await?;
        let audit = AuditRecord::new(
            None,
            "change_risk_levels",
            Uuid::nil(),
            AuditAction::Updated,
            principal.actor(),
            now,
        )
        .change("bands", Some(&describe(&old)), Some(&describe(&bands)));
        self.ports.changes.save_risk_bands(&bands, &audit).await?;
        self.risk_bands(principal).await
    }

    // --- Templates (FR-CHG-010) ----------------------------------------------------------

    /// Templates (needs `changes.read`). Configurers see all of them; other Analysts
    /// see the active ones usable by one of their Teams.
    ///
    /// # Errors
    ///
    /// Forbidden.
    pub async fn templates(&self, principal: &Principal) -> Result<Vec<ChangeTemplate>, AppError> {
        require(principal, capabilities::READ)?;
        let all = self.ports.changes.templates().await?;
        if principal.is_admin() || principal.can(capabilities::CONFIGURE) {
            return Ok(all);
        }
        let teams = match analyst(principal) {
            Some(a) => self.ports.directory.teams_of(a).await?,
            None => BTreeSet::new(),
        };
        Ok(all.into_iter().filter(|t| t.usable_by(&teams)).collect())
    }

    /// Creates a template or saves a new version of one (needs `changes.configure`).
    ///
    /// # Errors
    ///
    /// Forbidden, not found, invalid input or a duplicate name.
    pub async fn save_template(
        &self,
        principal: &Principal,
        id: Option<Uuid>,
        mut t: ChangeTemplate,
    ) -> Result<ChangeTemplate, AppError> {
        require(principal, capabilities::CONFIGURE)?;
        t.validate()?;
        let now = self.ports.clock.now();
        let existing = match id {
            Some(id) => Some(
                self.ports
                    .changes
                    .templates()
                    .await?
                    .into_iter()
                    .find(|x| x.id == id)
                    .ok_or(AppError::NotFound("Change Template"))?,
            ),
            None => None,
        };
        let (action, version) = match &existing {
            Some(e) => (
                AuditAction::Updated,
                if e.content == t.content {
                    e.version
                } else {
                    e.version + 1
                },
            ),
            None => (AuditAction::Created, 1),
        };
        t.id = existing.as_ref().map_or_else(Uuid::now_v7, |e| e.id);
        t.version = version;
        t.created_at = existing.as_ref().map_or(now, |e| e.created_at);
        t.updated_at = now;
        let audit = AuditRecord::new(
            None,
            "change_template",
            t.id,
            action,
            principal.actor(),
            now,
        )
        .change("name", existing.as_ref().map(|e| &e.name), Some(&t.name))
        .change(
            "version",
            existing.as_ref().map(|e| &e.version),
            Some(&t.version),
        )
        .change(
            "is_active",
            existing.as_ref().map(|e| &e.is_active),
            Some(&t.is_active),
        );
        self.ports
            .changes
            .save_template(&t, principal.actor(), &audit)
            .await?;
        Ok(t)
    }

    // --- Changes -------------------------------------------------------------------------

    async fn check_details(&self, d: &mut ChangeDetails) -> Result<(), AppError> {
        d.type_id = self
            .active_or_default(LookupKind::Type, d.type_id, "type_id")
            .await?;
        d.priority_id = self
            .active_or_default(LookupKind::Priority, d.priority_id, "priority_id")
            .await?;
        d.impact_id = self
            .active_or_default(LookupKind::Impact, d.impact_id, "impact_id")
            .await?;
        if let Some(c) = d.category_id {
            self.active_or_default(LookupKind::Category, c, "category_id")
                .await?;
        }
        if d.cab_required && d.cab_approval_type.is_none() {
            d.cab_approval_type = Some(CabApprovalType::All);
        }
        Ok(())
    }

    /// Creates a Change in a Company the caller can see (needs `changes.create`).
    /// With a template, empty fields are pre-filled from its current version; the
    /// template must be usable by one of the caller's Teams.
    ///
    /// # Errors
    ///
    /// Forbidden, an invisible Company, an unusable template or invalid details.
    pub async fn create(&self, principal: &Principal, new: NewChange) -> Result<Change, AppError> {
        require(principal, capabilities::CREATE)?;
        let tenant = new
            .tenant_id
            .ok_or_else(|| ChangeError::invalid("company_id", "is required"))?;
        if !principal.scope().allows(tenant) {
            return Err(AppError::NotFound("Company"));
        }
        let mut d = new.details;
        let mut template = None;
        if let Some(tid) = new.template_id {
            let t = self
                .ports
                .changes
                .templates()
                .await?
                .into_iter()
                .find(|t| t.id == tid)
                .ok_or(AppError::NotFound("Change Template"))?;
            let teams = match analyst(principal) {
                Some(a) => self.ports.directory.teams_of(a).await?,
                None => BTreeSet::new(),
            };
            if !principal.is_admin() && !t.usable_by(&teams) {
                return Err(AppError::NotFound("Change Template"));
            }
            prefill(&mut d, &t.content);
            template = Some((t.id, t.version));
        }
        self.check_details(&mut d).await?;
        let status = self.first_status(ChangeStage::Draft).await?;
        let number = change_number(self.ports.changes.next_sequence().await?);
        let now = self.ports.clock.now();
        let change = Change::open(tenant, number, d, &status, template, principal.actor(), now)?;
        let revs = revisions(None, change.details(), principal.actor(), now);
        self.ports
            .changes
            .insert(
                &change,
                &revs,
                &change_audit(None, &change, principal.actor(), now),
                &[],
            )
            .await?;
        Ok(change)
    }

    /// One Change.
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn get(&self, principal: &Principal, id: Uuid) -> Result<Change, AppError> {
        require(principal, capabilities::READ)?;
        self.ports
            .changes
            .get(principal.scope(), id)
            .await?
            .ok_or(AppError::NotFound("Change"))
    }

    /// Changes, newest activity first.
    ///
    /// # Errors
    ///
    /// Forbidden.
    pub async fn list(
        &self,
        principal: &Principal,
        query: &ChangeQuery,
    ) -> Result<Vec<Change>, AppError> {
        require(principal, capabilities::READ)?;
        Ok(self.ports.changes.list(principal.scope(), query).await?)
    }

    /// The Change Calendar for `[from, to)`, with overlapping windows (FR-CHG-012).
    ///
    /// # Errors
    ///
    /// Forbidden or a period over 366 days.
    pub async fn calendar(
        &self,
        principal: &Principal,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<Calendar, AppError> {
        require(principal, capabilities::READ)?;
        if to <= from || to - from > chrono::Duration::days(366) {
            return Err(
                ChangeError::invalid("to", "must be after from, at most 366 days later").into(),
            );
        }
        let changes: Vec<Change> = self
            .ports
            .changes
            .in_window(principal.scope(), from, to)
            .await?
            .into_iter()
            .filter(|c| c.stage() != ChangeStage::Stopped)
            .collect();
        let windows: Vec<Window> = changes
            .iter()
            .flat_map(|c| {
                let d = c.details();
                [
                    (d.work_start, d.work_end, false),
                    (d.outage_start, d.outage_end, true),
                ]
                .into_iter()
                .filter_map(move |(s, e, outage)| {
                    Some(Window {
                        change_id: c.id(),
                        outage,
                        start: s?,
                        end: e?,
                    })
                })
            })
            .collect();
        Ok(Calendar {
            overlaps: overlaps(&windows),
            changes,
        })
    }

    /// Edits a Change (needs `changes.update`). A material edit after submission
    /// starts a new Approval Round and returns the Change to an awaiting status.
    ///
    /// # Errors
    ///
    /// Forbidden, not found, a stage that is not editable, or invalid details.
    pub async fn edit(
        &self,
        principal: &Principal,
        id: Uuid,
        mut d: ChangeDetails,
    ) -> Result<Change, AppError> {
        require(principal, capabilities::UPDATE)?;
        let mut change = self.get(principal, id).await?;
        self.check_details(&mut d).await?;
        let before = change.clone();
        let now = self.ports.clock.now();
        let mut events = Vec::new();
        if change.edit(d, now)? {
            change.restart_approval(&self.first_status(ChangeStage::Awaiting).await?, now);
            events.push(event(
                &ChangeSubmitted {
                    change_id: id,
                    tenant_id: change.tenant_id(),
                    round_no: change.approval_round(),
                },
                principal.actor(),
                now,
            )?);
        }
        let revs = revisions(
            Some(before.details()),
            change.details(),
            principal.actor(),
            now,
        );
        self.ports
            .changes
            .update(
                &change,
                &revs,
                &change_audit(Some(&before), &change, principal.actor(), now),
                &events,
            )
            .await?;
        Ok(change)
    }

    /// Moves a Change to another status following the stage order (needs
    /// `changes.update`).
    ///
    /// # Errors
    ///
    /// Forbidden, not found, a move against the stage order, or submitting a CAB
    /// Change without CAB members.
    pub async fn move_to(
        &self,
        principal: &Principal,
        id: Uuid,
        status_id: Uuid,
    ) -> Result<Change, AppError> {
        require(principal, capabilities::UPDATE)?;
        let mut change = self.get(principal, id).await?;
        let status = self.lookup(LookupKind::Status, status_id).await?;
        let change_type = self
            .lookup(LookupKind::Type, change.details().type_id)
            .await?;
        if status.stage == ChangeStage::Awaiting
            && change.details().cab_required
            && self
                .ports
                .changes
                .cab(principal.scope(), id)
                .await?
                .is_empty()
        {
            return Err(
                ChangeError::InvalidState("add CAB members before submitting".into()).into(),
            );
        }
        let before = change.clone();
        let now = self.ports.clock.now();
        let moved = change.move_to(&status, &change_type, now)?;
        let (tenant, round) = (change.tenant_id(), change.approval_round());
        let events = match moved {
            Moved::Submitted => vec![event(
                &ChangeSubmitted {
                    change_id: id,
                    tenant_id: tenant,
                    round_no: round,
                },
                principal.actor(),
                now,
            )?],
            Moved::Scheduled => vec![event(
                &ChangeScheduled {
                    change_id: id,
                    tenant_id: tenant,
                    round_no: round,
                },
                principal.actor(),
                now,
            )?],
            Moved::Finished => vec![event(
                &ChangeCompleted {
                    change_id: id,
                    tenant_id: tenant,
                    round_no: round,
                },
                principal.actor(),
                now,
            )?],
            Moved::Unchanged | Moved::Other => Vec::new(),
        };
        if moved != Moved::Unchanged {
            self.ports
                .changes
                .update(
                    &change,
                    &[],
                    &change_audit(Some(&before), &change, principal.actor(), now),
                    &events,
                )
                .await?;
        }
        Ok(change)
    }

    // --- CAB and votes (FR-CHG-006, -007) ------------------------------------------------

    /// The CAB of a Change.
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn cab(&self, principal: &Principal, id: Uuid) -> Result<Vec<CabMember>, AppError> {
        self.get(principal, id).await?;
        Ok(self.ports.changes.cab(principal.scope(), id).await?)
    }

    /// Replaces the CAB while the Change is a draft (needs `changes.update`).
    ///
    /// # Errors
    ///
    /// Forbidden, not found, duplicates, or a submitted Change.
    pub async fn set_cab(
        &self,
        principal: &Principal,
        id: Uuid,
        members: Vec<CabMember>,
    ) -> Result<Vec<CabMember>, AppError> {
        require(principal, capabilities::UPDATE)?;
        let mut change = self.get(principal, id).await?;
        if change.stage() != ChangeStage::Draft {
            return Err(ChangeError::InvalidState(
                "the CAB is set while the Change is a draft".into(),
            )
            .into());
        }
        let ids: BTreeSet<AnalystId> = members.iter().map(|m| m.analyst_id).collect();
        if ids.len() != members.len() {
            return Err(ChangeError::invalid("members", "list each member once").into());
        }
        let now = self.ports.clock.now();
        change.touch(now);
        let describe = |ms: &[CabMember]| {
            ms.iter()
                .map(|m| {
                    format!(
                        "{}{}",
                        m.analyst_id,
                        if m.is_required { "" } else { " (optional)" }
                    )
                })
                .collect::<Vec<_>>()
        };
        let old = self.ports.changes.cab(principal.scope(), id).await?;
        let audit = AuditRecord::new(
            Some(change.tenant_id()),
            "change",
            id,
            AuditAction::Updated,
            principal.actor(),
            now,
        )
        .change("cab", Some(&describe(&old)), Some(&describe(&members)));
        self.ports
            .changes
            .set_cab(&change, &members, &audit)
            .await?;
        Ok(members)
    }

    /// Votes of a round (default: the current one).
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn votes(
        &self,
        principal: &Principal,
        id: Uuid,
        round: Option<i32>,
    ) -> Result<Vec<Vote>, AppError> {
        let change = self.get(principal, id).await?;
        Ok(self
            .ports
            .changes
            .votes(
                principal.scope(),
                id,
                round.unwrap_or_else(|| change.approval_round()),
            )
            .await?)
    }

    /// Votes on the current round (needs `changes.approve`, and to be its Approver or
    /// a CAB member when the Change names them). Votes are final; the round's outcome
    /// moves the Change on (approved) or back to draft (rejected).
    ///
    /// # Errors
    ///
    /// Forbidden, not found, no open round, not a decider, or a second vote.
    pub async fn vote(
        &self,
        principal: &Principal,
        id: Uuid,
        decision: Decision,
        comment: &str,
    ) -> Result<Change, AppError> {
        require(principal, capabilities::APPROVE)?;
        let voter = analyst(principal).ok_or_else(|| Forbidden("analyst".into()))?;
        let mut change = self.get(principal, id).await?;
        let change_type = self
            .lookup(LookupKind::Type, change.details().type_id)
            .await?;
        if !change.accepts_votes(&change_type) {
            return Err(ChangeError::InvalidState(
                "the Change is not waiting for a decision".into(),
            )
            .into());
        }
        let deciders = if change.details().cab_required {
            Deciders::Cab(
                self.ports.changes.cab(principal.scope(), id).await?,
                change
                    .details()
                    .cab_approval_type
                    .unwrap_or(CabApprovalType::All),
            )
        } else if let Some(a) = change.details().approver {
            Deciders::Approver(a)
        } else {
            Deciders::Anyone
        };
        if !deciders.may_vote(voter) {
            return Err(Forbidden("change approver or CAB member".into()).into());
        }
        let now = self.ports.clock.now();
        let vote = Vote {
            id: Uuid::now_v7(),
            round_no: change.approval_round(),
            analyst_id: voter,
            decision,
            comment: text("comment", comment, 10_000, decision == Decision::Reject)?,
            voted_at: now,
        };
        let mut votes = self
            .ports
            .changes
            .votes(principal.scope(), id, change.approval_round())
            .await?;
        if votes.iter().any(|v| v.analyst_id == voter) {
            return Err(AppError::Conflict("you already voted in this round".into()));
        }
        votes.push(vote.clone());
        let before = change.clone();
        let outcome = deciders.outcome(&votes);
        change.decide(
            outcome,
            &self.first_status(ChangeStage::Approved).await?,
            &self.first_status(ChangeStage::Draft).await?,
            now,
        );
        let (tenant, round) = (change.tenant_id(), change.approval_round());
        let events = match outcome {
            RoundOutcome::Approved => vec![event(
                &ChangeApproved {
                    change_id: id,
                    tenant_id: tenant,
                    round_no: round,
                },
                principal.actor(),
                now,
            )?],
            RoundOutcome::Rejected => vec![event(
                &ChangeRejected {
                    change_id: id,
                    tenant_id: tenant,
                    round_no: round,
                },
                principal.actor(),
                now,
            )?],
            RoundOutcome::Pending => Vec::new(),
        };
        let audit = change_audit(Some(&before), &change, principal.actor(), now).change(
            "vote",
            None,
            Some(&format!("{} by {voter}", decision.as_str())),
        );
        self.ports
            .changes
            .vote(&change, &vote, &audit, &events)
            .await?;
        Ok(change)
    }

    // --- Checklist, comments, links, revisions, PIR, attachments (FR-CHG-008, -009, -011) --

    /// The checklist.
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn checklist(
        &self,
        principal: &Principal,
        id: Uuid,
    ) -> Result<Vec<ChecklistItem>, AppError> {
        self.get(principal, id).await?;
        Ok(self.ports.changes.checklist(principal.scope(), id).await?)
    }

    /// Adds or updates a checklist item (needs `changes.update`).
    ///
    /// # Errors
    ///
    /// Forbidden, not found or an invalid text.
    pub async fn save_item(
        &self,
        principal: &Principal,
        id: Uuid,
        item_id: Option<Uuid>,
        item_text: &str,
        is_done: bool,
        sort_order: i32,
    ) -> Result<ChecklistItem, AppError> {
        require(principal, capabilities::UPDATE)?;
        let mut change = self.get(principal, id).await?;
        let now = self.ports.clock.now();
        let existing = match item_id {
            Some(i) => Some(
                self.ports
                    .changes
                    .checklist(principal.scope(), id)
                    .await?
                    .into_iter()
                    .find(|x| x.id == i)
                    .ok_or(AppError::NotFound("checklist item"))?,
            ),
            None => None,
        };
        let was_done = existing.as_ref().is_some_and(|e| e.is_done);
        let item = ChecklistItem {
            id: existing.as_ref().map_or_else(Uuid::now_v7, |e| e.id),
            text: text("text", item_text, 500, true)?,
            is_done,
            done_by: if is_done {
                if was_done {
                    existing.as_ref().and_then(|e| e.done_by)
                } else {
                    analyst(principal)
                }
            } else {
                None
            },
            done_at: if is_done {
                if was_done {
                    existing.as_ref().and_then(|e| e.done_at)
                } else {
                    Some(now)
                }
            } else {
                None
            },
            sort_order,
            created_at: existing.as_ref().map_or(now, |e| e.created_at),
        };
        change.touch(now);
        let audit = AuditRecord::new(
            Some(change.tenant_id()),
            "change_checklist_item",
            item.id,
            if existing.is_some() {
                AuditAction::Updated
            } else {
                AuditAction::Created
            },
            principal.actor(),
            now,
        )
        .change("text", existing.as_ref().map(|e| &e.text), Some(&item.text))
        .change(
            "is_done",
            existing.as_ref().map(|e| &e.is_done),
            Some(&item.is_done),
        );
        self.ports.changes.save_item(&change, &item, &audit).await?;
        Ok(item)
    }

    /// Deletes a checklist item (needs `changes.update`).
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn delete_item(
        &self,
        principal: &Principal,
        id: Uuid,
        item: Uuid,
    ) -> Result<(), AppError> {
        require(principal, capabilities::UPDATE)?;
        let mut change = self.get(principal, id).await?;
        let now = self.ports.clock.now();
        change.touch(now);
        let audit = AuditRecord::new(
            Some(change.tenant_id()),
            "change_checklist_item",
            item,
            AuditAction::Deleted,
            principal.actor(),
            now,
        );
        self.ports
            .changes
            .delete_item(&change, item, &audit)
            .await?;
        Ok(())
    }

    /// Comments, oldest first.
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn comments(
        &self,
        principal: &Principal,
        id: Uuid,
    ) -> Result<Vec<Comment>, AppError> {
        self.get(principal, id).await?;
        Ok(self.ports.changes.comments(principal.scope(), id).await?)
    }

    /// Adds a comment (needs `changes.update`).
    ///
    /// # Errors
    ///
    /// Forbidden, not found or an empty body.
    pub async fn add_comment(
        &self,
        principal: &Principal,
        id: Uuid,
        body: &str,
    ) -> Result<Comment, AppError> {
        require(principal, capabilities::UPDATE)?;
        let mut change = self.get(principal, id).await?;
        let now = self.ports.clock.now();
        let comment = Comment {
            id: Uuid::now_v7(),
            author_id: analyst(principal),
            body: text("body", body, 100_000, true)?,
            created_at: now,
        };
        change.touch(now);
        let audit = AuditRecord::new(
            Some(change.tenant_id()),
            "change_comment",
            comment.id,
            AuditAction::Created,
            principal.actor(),
            now,
        )
        .change("change_id", None, Some(&id));
        self.ports
            .changes
            .add_comment(&change, &comment, &audit)
            .await?;
        Ok(comment)
    }

    /// Links (to records the caller may see; others are left out).
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn links(
        &self,
        principal: &Principal,
        id: Uuid,
    ) -> Result<Vec<(ChangeLink, String)>, AppError> {
        self.get(principal, id).await?;
        let mut out = Vec::new();
        for l in self.ports.changes.links(principal.scope(), id).await? {
            if let Some((_, label)) = self.target(principal, l.kind, l.target_id).await? {
                out.push((l, label));
            }
        }
        Ok(out)
    }

    async fn target(
        &self,
        principal: &Principal,
        kind: LinkKind,
        target: Uuid,
    ) -> Result<Option<(TenantId, String)>, AppError> {
        Ok(match kind {
            LinkKind::Ticket => self.ports.directory.ticket(principal, target).await?,
            LinkKind::Problem => self.ports.directory.problem(principal, target).await?,
            LinkKind::Change => self
                .ports
                .changes
                .get(principal.scope(), target)
                .await?
                .map(|c| (c.tenant_id(), c.number().to_owned())),
        })
    }

    /// Links a ticket, Problem or Change of the same Company (needs `changes.update`).
    ///
    /// # Errors
    ///
    /// Forbidden, not found, another Company, a self link or a duplicate.
    pub async fn link(
        &self,
        principal: &Principal,
        id: Uuid,
        kind: LinkKind,
        target: Uuid,
    ) -> Result<(ChangeLink, String), AppError> {
        require(principal, capabilities::UPDATE)?;
        let mut change = self.get(principal, id).await?;
        if kind == LinkKind::Change && target == id {
            return Err(
                ChangeError::invalid("target_id", "a Change cannot be linked to itself").into(),
            );
        }
        let (tenant, label) = self
            .target(principal, kind, target)
            .await?
            .ok_or(AppError::NotFound("link target"))?;
        if tenant != change.tenant_id() {
            return Err(ChangeError::invalid("target_id", "belongs to a different Company").into());
        }
        let now = self.ports.clock.now();
        change.touch(now);
        let link = ChangeLink {
            kind,
            target_id: target,
            linked_by: principal.actor(),
            linked_at: now,
        };
        let audit = AuditRecord::new(
            Some(change.tenant_id()),
            "change",
            id,
            AuditAction::Updated,
            principal.actor(),
            now,
        )
        .change(format!("linked_{}", kind.as_str()), None, Some(&label));
        self.ports.changes.add_link(&change, &link, &audit).await?;
        Ok((link, label))
    }

    /// Removes a link (needs `changes.update`).
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn unlink(
        &self,
        principal: &Principal,
        id: Uuid,
        kind: LinkKind,
        target: Uuid,
    ) -> Result<(), AppError> {
        require(principal, capabilities::UPDATE)?;
        let mut change = self.get(principal, id).await?;
        let now = self.ports.clock.now();
        change.touch(now);
        let audit = AuditRecord::new(
            Some(change.tenant_id()),
            "change",
            id,
            AuditAction::Updated,
            principal.actor(),
            now,
        )
        .change(format!("linked_{}", kind.as_str()), Some(&target), None);
        self.ports
            .changes
            .remove_link(&change, kind, target, &audit)
            .await?;
        Ok(())
    }

    /// Saved versions of a long-text field, oldest first (FR-CHG-009).
    ///
    /// # Errors
    ///
    /// Forbidden, not found or an unknown field.
    pub async fn revisions(
        &self,
        principal: &Principal,
        id: Uuid,
        field: &str,
    ) -> Result<Vec<TextRevision>, AppError> {
        self.get(principal, id).await?;
        if !LONG_TEXT_FIELDS.contains(&field) {
            return Err(ChangeError::invalid(
                "field",
                format!("must be one of {}", LONG_TEXT_FIELDS.join(", ")),
            )
            .into());
        }
        Ok(self
            .ports
            .changes
            .revisions(principal.scope(), id, field)
            .await?)
    }

    /// Compares two revisions of a field line by line.
    ///
    /// # Errors
    ///
    /// Forbidden, not found or revisions of different fields.
    pub async fn compare(
        &self,
        principal: &Principal,
        id: Uuid,
        field: &str,
        from: Uuid,
        to: Uuid,
    ) -> Result<Vec<DiffLine>, AppError> {
        let revs = self.revisions(principal, id, field).await?;
        let find = |r: Uuid| {
            revs.iter()
                .find(|x| x.id == r)
                .ok_or(AppError::NotFound("Text Revision"))
        };
        Ok(diff_lines(&find(from)?.body, &find(to)?.body))
    }

    /// Records the PIR (needs `changes.update`).
    ///
    /// # Errors
    ///
    /// Forbidden, not found, the Change is not done, or invalid input.
    pub async fn record_pir(
        &self,
        principal: &Principal,
        id: Uuid,
        pir: Pir,
    ) -> Result<Change, AppError> {
        require(principal, capabilities::UPDATE)?;
        let mut change = self.get(principal, id).await?;
        let before = change.clone();
        let now = self.ports.clock.now();
        change.record_pir(pir, now)?;
        self.ports
            .changes
            .update(
                &change,
                &[],
                &change_audit(Some(&before), &change, principal.actor(), now),
                &[],
            )
            .await?;
        Ok(change)
    }

    /// Attachments.
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn attachments(
        &self,
        principal: &Principal,
        id: Uuid,
    ) -> Result<Vec<ChangeAttachment>, AppError> {
        self.get(principal, id).await?;
        Ok(self
            .ports
            .changes
            .attachments(principal.scope(), id)
            .await?)
    }

    /// Records an uploaded file (needs `changes.update`; the bytes are already in the
    /// blob store).
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn attach(
        &self,
        principal: &Principal,
        id: Uuid,
        file_name: String,
        media_type: String,
        size_bytes: u64,
        sha256: String,
    ) -> Result<ChangeAttachment, AppError> {
        require(principal, capabilities::UPDATE)?;
        let mut change = self.get(principal, id).await?;
        let now = self.ports.clock.now();
        let a = ChangeAttachment {
            id: Uuid::now_v7(),
            file_name,
            media_type,
            size_bytes,
            sha256,
            created_by: principal.actor(),
            created_at: now,
        };
        change.touch(now);
        let audit = AuditRecord::new(
            Some(change.tenant_id()),
            "attachment",
            a.id,
            AuditAction::Created,
            principal.actor(),
            now,
        )
        .change("change_id", None, Some(&id))
        .change("file_name", None, Some(&a.file_name));
        self.ports
            .changes
            .add_attachment(&change, &a, &audit)
            .await?;
        Ok(a)
    }

    /// The details of a Change as the notification handler needs them (Automation).
    ///
    /// # Errors
    ///
    /// Storage failures.
    pub async fn for_notifications(
        &self,
        id: Uuid,
    ) -> Result<Option<(Change, Vec<CabMember>)>, AppError> {
        let scope = cooper_kernel::TenantScope::All;
        let Some(change) = self.ports.changes.get(&scope, id).await? else {
            return Ok(None);
        };
        let cab = self.ports.changes.cab(&scope, id).await?;
        Ok(Some((change, cab)))
    }
}

fn prefill(d: &mut ChangeDetails, t: &TemplateContent) {
    let fill = |v: &mut String, from: &str| {
        if v.trim().is_empty() {
            from.clone_into(v);
        }
    };
    fill(&mut d.title, &t.title);
    fill(&mut d.description, &t.description);
    fill(&mut d.reason_for_change, &t.reason_for_change);
    fill(&mut d.risk_evaluation, &t.risk_evaluation);
    fill(&mut d.test_plan, &t.test_plan);
    fill(&mut d.rollback_plan, &t.rollback_plan);
    for (v, from) in [
        (&mut d.type_id, t.type_id),
        (&mut d.priority_id, t.priority_id),
        (&mut d.impact_id, t.impact_id),
    ] {
        if v.is_nil()
            && let Some(f) = from
        {
            *v = f;
        }
    }
    d.category_id = d.category_id.or(t.category_id);
    if d.risk_likelihood.is_none() && d.risk_impact_score.is_none() {
        d.risk_likelihood = t.risk_likelihood;
        d.risk_impact_score = t.risk_impact_score;
    }
    if !d.cab_required && t.cab_required {
        d.cab_required = true;
        d.cab_approval_type = d.cab_approval_type.or(t.cab_approval_type);
    }
}
