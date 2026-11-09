//! Change Management domain (ADR-0027). Pure Rust: no I/O.
//!
//! The rules live here: the stage order (FR-CHG-004), risk scoring (FR-CHG-005), the
//! CAB outcome of an Approval Round (FR-CHG-006), new rounds after material edits
//! (FR-CHG-007), text revision diffs (FR-CHG-009) and calendar overlaps (FR-CHG-012).

use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use cooper_kernel::{Actor, AnalystId, DomainEvent, TenantId};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;
use uuid::Uuid;

/// A Change Management rule was violated.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ChangeError {
    /// A value failed validation.
    #[error("{field} {reason}")]
    Invalid {
        /// The field.
        field: &'static str,
        /// Why.
        reason: String,
    },
    /// The Change is not in a state that allows this.
    #[error("{0}")]
    InvalidState(String),
}

impl ChangeError {
    /// An [`ChangeError::Invalid`].
    #[must_use]
    pub fn invalid(field: &'static str, reason: impl Into<String>) -> Self {
        Self::Invalid {
            field,
            reason: reason.into(),
        }
    }

    fn state(reason: impl Into<String>) -> Self {
        Self::InvalidState(reason.into())
    }
}

pub(crate) fn text(
    field: &'static str,
    value: &str,
    max: usize,
    required: bool,
) -> Result<String, ChangeError> {
    let v = value.trim();
    if required && v.is_empty() {
        return Err(ChangeError::invalid(field, "is required"));
    }
    if v.chars().count() > max {
        return Err(ChangeError::invalid(
            field,
            format!("must be at most {max} characters"),
        ));
    }
    Ok(v.to_owned())
}

/// Fixed lifecycle phase grouping Change Statuses (FR-CHG-003).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeStage {
    /// Being written; editable.
    Draft,
    /// Submitted, waiting for approval.
    Awaiting,
    /// Approved, not started.
    Approved,
    /// Scheduled or under way.
    Implementing,
    /// Completed or failed.
    Done,
    /// Rejected or cancelled.
    Stopped,
}

impl ChangeStage {
    /// Stable name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Awaiting => "awaiting",
            Self::Approved => "approved",
            Self::Implementing => "implementing",
            Self::Done => "done",
            Self::Stopped => "stopped",
        }
    }
}

impl FromStr for ChangeStage {
    type Err = ChangeError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "draft" => Self::Draft,
            "awaiting" => Self::Awaiting,
            "approved" => Self::Approved,
            "implementing" => Self::Implementing,
            "done" => Self::Done,
            "stopped" => Self::Stopped,
            _ => {
                return Err(ChangeError::invalid(
                    "stage",
                    "must be draft, awaiting, approved, implementing, done or stopped",
                ));
            }
        })
    }
}

impl fmt::Display for ChangeStage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// When a Change Type is approved (fixed set).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalRoute {
    /// Approve before work starts (default).
    Review,
    /// Work may start while the decision is pending; approve afterwards.
    Emergency,
}

impl ApprovalRoute {
    /// Stable name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Review => "review",
            Self::Emergency => "emergency",
        }
    }
}

impl FromStr for ApprovalRoute {
    type Err = ChangeError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "review" => Ok(Self::Review),
            "emergency" => Ok(Self::Emergency),
            _ => Err(ChangeError::invalid(
                "approval_route",
                "must be review or emergency",
            )),
        }
    }
}

/// Which lookup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LookupKind {
    /// Change Type (`approval_route`, `is_standard`).
    Type,
    /// Change Status (`stage`, `is_closed`).
    Status,
    /// Change Priority.
    Priority,
    /// Change Impact.
    Impact,
    /// Change Category.
    Category,
}

impl LookupKind {
    /// Name in messages.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Type => "Change Type",
            Self::Status => "Change Status",
            Self::Priority => "Change Priority",
            Self::Impact => "Change Impact",
            Self::Category => "Change Category",
        }
    }

    /// Audit aggregate type.
    #[must_use]
    pub const fn table(self) -> &'static str {
        match self {
            Self::Type => "change_types",
            Self::Status => "change_statuses",
            Self::Priority => "change_priorities",
            Self::Impact => "change_impacts",
            Self::Category => "change_categories",
        }
    }
}

/// A Change Type, Status, Priority, Impact or Category (configurable reference data).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lookup {
    /// ID.
    pub id: Uuid,
    /// Kind.
    pub kind: LookupKind,
    /// Stable key of seeded rows.
    pub key: Option<String>,
    /// Name.
    pub name: String,
    /// Status: its Stage.
    pub stage: ChangeStage,
    /// Status: closing.
    pub is_closed: bool,
    /// Type: Approval Route.
    pub route: ApprovalRoute,
    /// Type: pre-approved (Standard).
    pub is_standard: bool,
    /// The default (exactly one per kind; Categories may have none).
    pub is_default: bool,
    /// Display order.
    pub sort_order: i32,
    /// Active.
    pub is_active: bool,
    /// Created.
    pub created_at: DateTime<Utc>,
    /// Last changed.
    pub updated_at: DateTime<Utc>,
}

/// What an Administrator sets on a lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LookupInput {
    /// Name.
    pub name: String,
    /// Status: Stage (ignored for other kinds).
    pub stage: ChangeStage,
    /// Status: closing.
    pub is_closed: bool,
    /// Type: Approval Route.
    pub route: ApprovalRoute,
    /// Type: pre-approved.
    pub is_standard: bool,
    /// Make it the default.
    pub is_default: bool,
    /// Display order.
    pub sort_order: i32,
    /// Active.
    pub is_active: bool,
}

impl Lookup {
    /// Creates a lookup row.
    ///
    /// # Errors
    ///
    /// See [`Lookup::apply`].
    pub fn create(
        kind: LookupKind,
        input: LookupInput,
        now: DateTime<Utc>,
    ) -> Result<Self, ChangeError> {
        let mut l = Self {
            id: Uuid::now_v7(),
            kind,
            key: None,
            name: String::new(),
            stage: ChangeStage::Draft,
            is_closed: false,
            route: ApprovalRoute::Review,
            is_standard: false,
            is_default: false,
            sort_order: 0,
            is_active: true,
            created_at: now,
            updated_at: now,
        };
        l.apply(input, now)?;
        Ok(l)
    }

    /// Changes the row. Statuses keep `is_closed` consistent with their Stage, and the
    /// default status must be a draft one.
    ///
    /// # Errors
    ///
    /// Invalid name, an inactive default, a non-draft default status, or a closing
    /// status outside the `done`/`stopped` stages.
    pub fn apply(&mut self, input: LookupInput, now: DateTime<Utc>) -> Result<(), ChangeError> {
        let name = text("name", &input.name, 100, true)?;
        if input.is_default && !input.is_active {
            return Err(ChangeError::invalid(
                "is_default",
                "the default must be active",
            ));
        }
        if self.kind == LookupKind::Status {
            if input.is_default && input.stage != ChangeStage::Draft {
                return Err(ChangeError::invalid(
                    "is_default",
                    "new Changes start in a draft status",
                ));
            }
            if input.is_closed && !matches!(input.stage, ChangeStage::Done | ChangeStage::Stopped) {
                return Err(ChangeError::invalid(
                    "is_closed",
                    "only done or stopped statuses close a Change",
                ));
            }
            self.stage = input.stage;
            self.is_closed = input.is_closed;
        }
        if self.kind == LookupKind::Type {
            self.route = input.route;
            self.is_standard = input.is_standard;
        }
        self.name = name;
        self.is_default = input.is_default;
        self.sort_order = input.sort_order;
        self.is_active = input.is_active;
        self.updated_at = now;
        Ok(())
    }
}

/// Whether a move from `from` to `to` follows the stage order (FR-CHG-004).
///
/// Draft → Awaiting (submit), Awaiting → Draft (withdraw), Approved → Implementing,
/// Implementing → Done, any open stage → Stopped, Stopped → Draft (revive), and moves
/// within a stage. Entering Approved happens only through an approval decision. A
/// Standard (pre-approved) Change may go Draft → Implementing; an Emergency one may
/// start implementing while its approval is still awaited.
///
/// # Errors
///
/// [`ChangeError::InvalidState`] naming the refused move.
pub fn check_stage_move(
    from: ChangeStage,
    to: ChangeStage,
    change_type: &Lookup,
) -> Result<(), ChangeError> {
    use ChangeStage::{Approved, Awaiting, Done, Draft, Implementing, Stopped};
    let allowed = match (from, to) {
        (a, b) if a == b => true,
        (Draft, Awaiting)
        | (Awaiting, Draft)
        | (Approved, Implementing)
        | (Implementing, Done)
        | (Stopped, Draft) => true,
        (Draft | Awaiting | Approved | Implementing, Stopped) => true,
        (Draft, Implementing) => change_type.is_standard,
        (Awaiting, Implementing) => {
            change_type.route == ApprovalRoute::Emergency || change_type.is_standard
        }
        (_, Approved) => false,
        _ => false,
    };
    if allowed {
        Ok(())
    } else if to == Approved {
        Err(ChangeError::state(
            "a Change is approved by its approver or CAB, not by moving its status",
        ))
    } else {
        Err(ChangeError::state(format!(
            "a Change cannot move from {from} to {to}"
        )))
    }
}

/// A Risk Level band (FR-CHG-005): reference data, not code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RiskLevel {
    /// ID.
    pub id: Uuid,
    /// Name.
    pub name: String,
    /// Lowest score in the band.
    pub min_score: u8,
    /// Highest score in the band.
    pub max_score: u8,
    /// Display order.
    pub sort_order: i32,
}

/// Likelihood × Impact Score, each 1–5.
///
/// # Errors
///
/// A value outside 1–5.
pub fn risk_score(likelihood: u8, impact: u8) -> Result<u8, ChangeError> {
    if !(1..=5).contains(&likelihood) {
        return Err(ChangeError::invalid("risk_likelihood", "must be 1 to 5"));
    }
    if !(1..=5).contains(&impact) {
        return Err(ChangeError::invalid("risk_impact_score", "must be 1 to 5"));
    }
    Ok(likelihood * impact)
}

/// The band a score falls in.
#[must_use]
pub fn risk_level(score: u8, bands: &[RiskLevel]) -> Option<&RiskLevel> {
    bands
        .iter()
        .find(|b| (b.min_score..=b.max_score).contains(&score))
}

/// Checks that bands cover 1–25 exactly once.
///
/// # Errors
///
/// Gaps or overlaps.
pub fn check_bands(bands: &[RiskLevel]) -> Result<(), ChangeError> {
    for score in 1..=25_u8 {
        let n = bands
            .iter()
            .filter(|b| (b.min_score..=b.max_score).contains(&score))
            .count();
        if n != 1 {
            return Err(ChangeError::invalid(
                "risk_levels",
                format!("score {score} must fall in exactly one band"),
            ));
        }
    }
    Ok(())
}

/// How a CAB decides (fixed set).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CabApprovalType {
    /// Every required member approves (default).
    All,
    /// More than half of the required members approve.
    Majority,
}

impl CabApprovalType {
    /// Stable name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Majority => "majority",
        }
    }
}

impl FromStr for CabApprovalType {
    type Err = ChangeError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "all" => Ok(Self::All),
            "majority" => Ok(Self::Majority),
            _ => Err(ChangeError::invalid(
                "cab_approval_type",
                "must be all or majority",
            )),
        }
    }
}

/// A CAB member of a Change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CabMember {
    /// The Analyst.
    pub analyst_id: AnalystId,
    /// Required for the decision.
    pub is_required: bool,
}

/// A vote decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    /// Approve.
    Approve,
    /// Reject.
    Reject,
}

impl Decision {
    /// Stable name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Approve => "approve",
            Self::Reject => "reject",
        }
    }
}

impl FromStr for Decision {
    type Err = ChangeError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "approve" => Ok(Self::Approve),
            "reject" => Ok(Self::Reject),
            _ => Err(ChangeError::invalid(
                "decision",
                "must be approve or reject",
            )),
        }
    }
}

/// An append-only vote in one Approval Round (FR-CHG-007).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Vote {
    /// ID.
    pub id: Uuid,
    /// Round.
    pub round_no: i32,
    /// Who voted.
    pub analyst_id: AnalystId,
    /// The decision.
    pub decision: Decision,
    /// Comment.
    pub comment: String,
    /// When.
    pub voted_at: DateTime<Utc>,
}

/// The state of an Approval Round.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoundOutcome {
    /// More votes are needed.
    Pending,
    /// Approved.
    Approved,
    /// Rejected: the Change goes back.
    Rejected,
}

/// Who decides a Change, and how (FR-CHG-006).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Deciders {
    /// The CAB, by its approval type. With no required members, every member counts.
    Cab(Vec<CabMember>, CabApprovalType),
    /// The named Approver alone.
    Approver(AnalystId),
    /// Anyone allowed to approve Changes; the first vote decides.
    Anyone,
}

impl Deciders {
    /// Whether `analyst` may vote.
    #[must_use]
    pub fn may_vote(&self, analyst: AnalystId) -> bool {
        match self {
            Self::Cab(members, _) => members.iter().any(|m| m.analyst_id == analyst),
            Self::Approver(a) => *a == analyst,
            Self::Anyone => true,
        }
    }

    /// The outcome of a round given its votes. A rejection by anyone who counts (the
    /// Approver, a required CAB member, or any member when none is required) rejects;
    /// optional members only advise.
    #[must_use]
    pub fn outcome(&self, votes: &[Vote]) -> RoundOutcome {
        let voted =
            |a: AnalystId, d: Decision| votes.iter().any(|v| v.analyst_id == a && v.decision == d);
        match self {
            Self::Anyone => match votes.first() {
                None => RoundOutcome::Pending,
                Some(v) if v.decision == Decision::Reject => RoundOutcome::Rejected,
                Some(_) => RoundOutcome::Approved,
            },
            Self::Approver(a) => {
                if voted(*a, Decision::Reject) {
                    RoundOutcome::Rejected
                } else if voted(*a, Decision::Approve) {
                    RoundOutcome::Approved
                } else {
                    RoundOutcome::Pending
                }
            }
            Self::Cab(members, kind) => {
                let counted: Vec<AnalystId> = if members.iter().any(|m| m.is_required) {
                    members
                        .iter()
                        .filter(|m| m.is_required)
                        .map(|m| m.analyst_id)
                        .collect()
                } else {
                    members.iter().map(|m| m.analyst_id).collect()
                };
                if counted.is_empty() {
                    return RoundOutcome::Pending;
                }
                if counted.iter().any(|a| voted(*a, Decision::Reject)) {
                    return RoundOutcome::Rejected;
                }
                let yes = counted
                    .iter()
                    .filter(|a| voted(**a, Decision::Approve))
                    .count();
                let passed = match kind {
                    CabApprovalType::All => yes == counted.len(),
                    CabApprovalType::Majority => yes * 2 > counted.len(),
                };
                if passed {
                    RoundOutcome::Approved
                } else {
                    RoundOutcome::Pending
                }
            }
        }
    }
}

/// Fields of a Change an Analyst edits (FR-CHG-001).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChangeDetails {
    /// Title.
    pub title: String,
    /// Change Type.
    pub type_id: Uuid,
    /// Priority.
    pub priority_id: Uuid,
    /// Impact.
    pub impact_id: Uuid,
    /// Category.
    pub category_id: Option<Uuid>,
    /// Requested By (an Analyst, as in FreeITSM).
    pub requested_by: Option<AnalystId>,
    /// Assigned To.
    pub assigned_to: Option<AnalystId>,
    /// Approver.
    pub approver: Option<AnalystId>,
    /// Work Window start.
    pub work_start: Option<DateTime<Utc>>,
    /// Work Window end.
    pub work_end: Option<DateTime<Utc>>,
    /// Outage Window start.
    pub outage_start: Option<DateTime<Utc>>,
    /// Outage Window end.
    pub outage_end: Option<DateTime<Utc>>,
    /// Description.
    pub description: String,
    /// Reason for Change.
    pub reason_for_change: String,
    /// Risk Evaluation (narrative).
    pub risk_evaluation: String,
    /// Risk Likelihood 1–5.
    pub risk_likelihood: Option<u8>,
    /// Risk Impact Score 1–5.
    pub risk_impact_score: Option<u8>,
    /// Test Plan.
    pub test_plan: String,
    /// Rollback Plan.
    pub rollback_plan: String,
    /// The CAB decides.
    pub cab_required: bool,
    /// How the CAB decides.
    pub cab_approval_type: Option<CabApprovalType>,
}

/// Long-text fields kept as Text Revisions (FR-CHG-009).
pub const LONG_TEXT_FIELDS: &[&str] = &[
    "description",
    "reason_for_change",
    "risk_evaluation",
    "test_plan",
    "rollback_plan",
];

impl ChangeDetails {
    fn validate(mut self) -> Result<Self, ChangeError> {
        self.title = text("title", &self.title, 255, true)?;
        for (name, value) in [
            ("description", &mut self.description),
            ("reason_for_change", &mut self.reason_for_change),
            ("risk_evaluation", &mut self.risk_evaluation),
            ("test_plan", &mut self.test_plan),
            ("rollback_plan", &mut self.rollback_plan),
        ] {
            *value = text(name, value, 1_000_000, false)?;
        }
        if let (Some(l), Some(i)) = (self.risk_likelihood, self.risk_impact_score) {
            risk_score(l, i)?;
        } else if self.risk_likelihood.is_some() != self.risk_impact_score.is_some() {
            return Err(ChangeError::invalid(
                "risk_likelihood",
                "give both Risk Likelihood and Risk Impact Score, or neither",
            ));
        }
        for (start, end, field) in [
            (self.work_start, self.work_end, "work_end"),
            (self.outage_start, self.outage_end, "outage_end"),
        ] {
            if let (Some(s), Some(e)) = (start, end)
                && e <= s
            {
                return Err(ChangeError::invalid(field, "must be after the start"));
            }
        }
        Ok(self)
    }

    /// The value of a long-text field by name.
    #[must_use]
    pub fn long_text(&self, field: &str) -> Option<&str> {
        Some(match field {
            "description" => &self.description,
            "reason_for_change" => &self.reason_for_change,
            "risk_evaluation" => &self.risk_evaluation,
            "test_plan" => &self.test_plan,
            "rollback_plan" => &self.rollback_plan,
            _ => return None,
        })
    }

    /// Whether `other` differs in material content: what the approvers judged (type,
    /// impact, windows, plans, risk, description, reason, CAB settings). People and
    /// priority are not material.
    #[must_use]
    pub fn materially_differs(&self, other: &Self) -> bool {
        (
            self.type_id,
            self.impact_id,
            self.work_start,
            self.work_end,
            self.outage_start,
            self.outage_end,
        ) != (
            other.type_id,
            other.impact_id,
            other.work_start,
            other.work_end,
            other.outage_start,
            other.outage_end,
        ) || (
            self.risk_likelihood,
            self.risk_impact_score,
            self.cab_required,
            self.cab_approval_type,
        ) != (
            other.risk_likelihood,
            other.risk_impact_score,
            other.cab_required,
            other.cab_approval_type,
        ) || LONG_TEXT_FIELDS
            .iter()
            .any(|f| self.long_text(f) != other.long_text(f))
            || self.title != other.title
    }

    /// The Risk Score, if both factors are set.
    #[must_use]
    pub fn risk_score(&self) -> Option<u8> {
        match (self.risk_likelihood, self.risk_impact_score) {
            (Some(l), Some(i)) => risk_score(l, i).ok(),
            _ => None,
        }
    }
}

/// The Post-Implementation Review (FR-CHG-011).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pir {
    /// Whether the Change succeeded.
    pub successful: bool,
    /// Actual start.
    pub actual_start: Option<DateTime<Utc>>,
    /// Actual end.
    pub actual_end: Option<DateTime<Utc>>,
    /// Lessons learned.
    pub lessons_learned: String,
    /// Follow-up.
    pub follow_up: String,
    /// When it was recorded.
    pub recorded_at: DateTime<Utc>,
}

/// A planned modification to IT services (FR-CHG-001).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    id: Uuid,
    tenant_id: TenantId,
    number: String,
    details: ChangeDetails,
    status_id: Uuid,
    stage: ChangeStage,
    approval_round: i32,
    approved_at: Option<DateTime<Utc>>,
    template: Option<(Uuid, i32)>,
    pir: Option<Pir>,
    created_by: Actor,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    closed_at: Option<DateTime<Utc>>,
}

/// What a status change did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Moved {
    /// Nothing.
    Unchanged,
    /// Submitted for approval: a new round opened.
    Submitted,
    /// Moved into implementation (scheduled).
    Scheduled,
    /// Completed or failed.
    Finished,
    /// Some other move.
    Other,
}

/// `CHG-00001`.
#[must_use]
pub fn change_number(sequence: u64) -> String {
    format!("CHG-{sequence:05}")
}

impl Change {
    /// Opens a Change in the default (draft) status.
    ///
    /// # Errors
    ///
    /// Invalid details or a non-draft status.
    pub fn open(
        tenant_id: TenantId,
        number: String,
        details: ChangeDetails,
        status: &Lookup,
        template: Option<(Uuid, i32)>,
        created_by: Actor,
        now: DateTime<Utc>,
    ) -> Result<Self, ChangeError> {
        if status.stage != ChangeStage::Draft {
            return Err(ChangeError::invalid(
                "status_id",
                "a new Change starts in a draft status",
            ));
        }
        Ok(Self {
            id: Uuid::now_v7(),
            tenant_id,
            number,
            details: details.validate()?,
            status_id: status.id,
            stage: ChangeStage::Draft,
            approval_round: 0,
            approved_at: None,
            template,
            pir: None,
            created_by,
            created_at: now,
            updated_at: now,
            closed_at: None,
        })
    }

    /// Rebuilds a Change from storage.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub const fn restore(
        id: Uuid,
        tenant_id: TenantId,
        number: String,
        details: ChangeDetails,
        status_id: Uuid,
        stage: ChangeStage,
        approval_round: i32,
        approved_at: Option<DateTime<Utc>>,
        template: Option<(Uuid, i32)>,
        pir: Option<Pir>,
        created_by: Actor,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
        closed_at: Option<DateTime<Utc>>,
    ) -> Self {
        Self {
            id,
            tenant_id,
            number,
            details,
            status_id,
            stage,
            approval_round,
            approved_at,
            template,
            pir,
            created_by,
            created_at,
            updated_at,
            closed_at,
        }
    }

    /// Edits the details. Editable in draft, awaiting and approved stages. A material
    /// edit after submission invalidates approval: returns `true`, and the caller moves
    /// the Change back to an awaiting status with a new round (FR-CHG-007).
    ///
    /// # Errors
    ///
    /// Invalid details, or a Change being implemented, done or stopped.
    pub fn edit(
        &mut self,
        details: ChangeDetails,
        now: DateTime<Utc>,
    ) -> Result<bool, ChangeError> {
        if !matches!(
            self.stage,
            ChangeStage::Draft | ChangeStage::Awaiting | ChangeStage::Approved
        ) {
            return Err(ChangeError::state(format!(
                "a Change in the {} stage cannot be edited",
                self.stage
            )));
        }
        let details = details.validate()?;
        let reopens = self.stage != ChangeStage::Draft && self.details.materially_differs(&details);
        self.details = details;
        self.updated_at = now;
        Ok(reopens)
    }

    /// Moves the Change to `status` (FR-CHG-004). Entering `awaiting` opens a new
    /// Approval Round; leaving `approved` backwards clears the approval; closing records
    /// the time.
    ///
    /// # Errors
    ///
    /// An inactive status or a move against the stage order.
    pub fn move_to(
        &mut self,
        status: &Lookup,
        change_type: &Lookup,
        now: DateTime<Utc>,
    ) -> Result<Moved, ChangeError> {
        if status.id == self.status_id {
            return Ok(Moved::Unchanged);
        }
        if !status.is_active {
            return Err(ChangeError::invalid("status_id", "is not active"));
        }
        check_stage_move(self.stage, status.stage, change_type)?;
        let from = self.stage;
        self.apply_status(status, now);
        Ok(match (from, status.stage) {
            (a, b) if a == b => Moved::Other,
            (_, ChangeStage::Awaiting) => {
                self.open_round();
                Moved::Submitted
            }
            (_, ChangeStage::Implementing) => Moved::Scheduled,
            (_, ChangeStage::Done) => Moved::Finished,
            (_, ChangeStage::Draft) => {
                self.approved_at = None;
                Moved::Other
            }
            _ => Moved::Other,
        })
    }

    fn apply_status(&mut self, status: &Lookup, now: DateTime<Utc>) {
        self.status_id = status.id;
        self.stage = status.stage;
        self.closed_at = if status.is_closed {
            self.closed_at.or(Some(now))
        } else {
            None
        };
        self.updated_at = now;
    }

    fn open_round(&mut self) {
        self.approval_round += 1;
        self.approved_at = None;
    }

    /// After a material edit: back to `awaiting` with a new round.
    pub fn restart_approval(&mut self, awaiting: &Lookup, now: DateTime<Utc>) {
        self.apply_status(awaiting, now);
        self.open_round();
    }

    /// Records the round's outcome: approved moves to `approved_status` unless the
    /// Change is already under way (an Emergency Change approved afterwards); rejected
    /// sends it back to `draft_status`.
    pub fn decide(
        &mut self,
        outcome: RoundOutcome,
        approved_status: &Lookup,
        draft_status: &Lookup,
        now: DateTime<Utc>,
    ) {
        match outcome {
            RoundOutcome::Pending => {}
            RoundOutcome::Approved => {
                self.approved_at = Some(now);
                if self.stage == ChangeStage::Awaiting {
                    self.apply_status(approved_status, now);
                } else {
                    self.updated_at = now;
                }
            }
            RoundOutcome::Rejected => {
                self.approved_at = None;
                if matches!(self.stage, ChangeStage::Awaiting | ChangeStage::Approved) {
                    self.apply_status(draft_status, now);
                } else {
                    self.updated_at = now;
                }
            }
        }
    }

    /// Whether votes are taken now: a round is open and the Change awaits approval, or
    /// an Emergency Change is under way without a decision yet.
    #[must_use]
    pub fn accepts_votes(&self, change_type: &Lookup) -> bool {
        self.approval_round > 0
            && self.approved_at.is_none()
            && (self.stage == ChangeStage::Awaiting
                || (change_type.route == ApprovalRoute::Emergency
                    && matches!(self.stage, ChangeStage::Implementing | ChangeStage::Done)))
    }

    /// Records the PIR (FR-CHG-011).
    ///
    /// # Errors
    ///
    /// The Change is not done, or the actual window is reversed.
    pub fn record_pir(&mut self, mut pir: Pir, now: DateTime<Utc>) -> Result<(), ChangeError> {
        if self.stage != ChangeStage::Done {
            return Err(ChangeError::state(
                "a PIR is recorded once the Change is completed or failed",
            ));
        }
        if let (Some(s), Some(e)) = (pir.actual_start, pir.actual_end)
            && e < s
        {
            return Err(ChangeError::invalid(
                "actual_end",
                "must not be before the actual start",
            ));
        }
        pir.lessons_learned = text("lessons_learned", &pir.lessons_learned, 100_000, false)?;
        pir.follow_up = text("follow_up", &pir.follow_up, 100_000, false)?;
        pir.recorded_at = now;
        self.pir = Some(pir);
        self.updated_at = now;
        Ok(())
    }

    /// Records activity (comments, checklist, links).
    pub fn touch(&mut self, now: DateTime<Utc>) {
        self.updated_at = now;
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
    /// Change Number.
    #[must_use]
    pub fn number(&self) -> &str {
        &self.number
    }
    /// Details.
    #[must_use]
    pub const fn details(&self) -> &ChangeDetails {
        &self.details
    }
    /// Status.
    #[must_use]
    pub const fn status_id(&self) -> Uuid {
        self.status_id
    }
    /// Stage of the status.
    #[must_use]
    pub const fn stage(&self) -> ChangeStage {
        self.stage
    }
    /// Current Approval Round (0 before the first submission).
    #[must_use]
    pub const fn approval_round(&self) -> i32 {
        self.approval_round
    }
    /// When the current round was approved.
    #[must_use]
    pub const fn approved_at(&self) -> Option<DateTime<Utc>> {
        self.approved_at
    }
    /// Template and version it was created from.
    #[must_use]
    pub const fn template(&self) -> Option<(Uuid, i32)> {
        self.template
    }
    /// PIR.
    #[must_use]
    pub const fn pir(&self) -> Option<&Pir> {
        self.pir.as_ref()
    }
    /// Who created it.
    #[must_use]
    pub const fn created_by(&self) -> Actor {
        self.created_by
    }
    /// Created.
    #[must_use]
    pub const fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }
    /// Last changed.
    #[must_use]
    pub const fn updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }
    /// Closed.
    #[must_use]
    pub const fn closed_at(&self) -> Option<DateTime<Utc>> {
        self.closed_at
    }

    /// The audited fields.
    #[must_use]
    pub fn audit_fields(&self) -> Vec<(&'static str, Value)> {
        let d = &self.details;
        let mut f = vec![
            ("title", json!(d.title)),
            ("type_id", json!(d.type_id)),
            ("priority_id", json!(d.priority_id)),
            ("impact_id", json!(d.impact_id)),
            ("category_id", json!(d.category_id)),
            ("requested_by_id", json!(d.requested_by)),
            ("assigned_to_id", json!(d.assigned_to)),
            ("approver_id", json!(d.approver)),
            ("work_start", json!(d.work_start)),
            ("work_end", json!(d.work_end)),
            ("outage_start", json!(d.outage_start)),
            ("outage_end", json!(d.outage_end)),
            ("risk_likelihood", json!(d.risk_likelihood)),
            ("risk_impact_score", json!(d.risk_impact_score)),
            ("cab_required", json!(d.cab_required)),
            (
                "cab_approval_type",
                json!(d.cab_approval_type.map(CabApprovalType::as_str)),
            ),
            ("status_id", json!(self.status_id)),
            ("approval_round", json!(self.approval_round)),
            ("approved_at", json!(self.approved_at)),
            ("closed_at", json!(self.closed_at)),
            (
                "pir_successful",
                json!(self.pir.as_ref().map(|p| p.successful)),
            ),
        ];
        // Long texts are kept whole as Text Revisions; the audit notes that they changed.
        for name in LONG_TEXT_FIELDS {
            f.push((
                name,
                json!(
                    d.long_text(name)
                        .map(|t| format!("{} characters", t.chars().count()))
                ),
            ));
        }
        f
    }
}

/// A Change Template version's content (FR-CHG-010): the fields it pre-fills.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TemplateContent {
    /// Title.
    pub title: String,
    /// Change Type.
    pub type_id: Option<Uuid>,
    /// Priority.
    pub priority_id: Option<Uuid>,
    /// Impact.
    pub impact_id: Option<Uuid>,
    /// Category.
    pub category_id: Option<Uuid>,
    /// Description.
    pub description: String,
    /// Reason for Change.
    pub reason_for_change: String,
    /// Risk Evaluation.
    pub risk_evaluation: String,
    /// Risk Likelihood.
    pub risk_likelihood: Option<u8>,
    /// Risk Impact Score.
    pub risk_impact_score: Option<u8>,
    /// Test Plan.
    pub test_plan: String,
    /// Rollback Plan.
    pub rollback_plan: String,
    /// CAB required.
    pub cab_required: bool,
    /// CAB approval type.
    pub cab_approval_type: Option<CabApprovalType>,
}

/// A Change Template (FR-CHG-010). Each save makes a new version; Changes record the
/// version they were created from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeTemplate {
    /// ID.
    pub id: Uuid,
    /// Name.
    pub name: String,
    /// Description.
    pub description: String,
    /// Teams whose members may use it; empty for everyone.
    pub team_ids: Vec<cooper_kernel::TeamId>,
    /// Current version.
    pub version: i32,
    /// The current version's content.
    pub content: TemplateContent,
    /// Active.
    pub is_active: bool,
    /// Created.
    pub created_at: DateTime<Utc>,
    /// Last changed.
    pub updated_at: DateTime<Utc>,
}

impl ChangeTemplate {
    /// Whether an Analyst in `teams` may use it.
    #[must_use]
    pub fn usable_by(&self, teams: &std::collections::BTreeSet<cooper_kernel::TeamId>) -> bool {
        self.is_active
            && (self.team_ids.is_empty() || self.team_ids.iter().any(|t| teams.contains(t)))
    }

    /// Validates name and description.
    ///
    /// # Errors
    ///
    /// Invalid name or description.
    pub fn validate(&mut self) -> Result<(), ChangeError> {
        self.name = text("name", &self.name, 150, true)?;
        self.description = text("description", &self.description, 2000, false)?;
        if let (Some(l), Some(i)) = (self.content.risk_likelihood, self.content.risk_impact_score) {
            risk_score(l, i)?;
        }
        Ok(())
    }
}

/// One line of a revision comparison.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "op", content = "text", rename_all = "snake_case")]
pub enum DiffLine {
    /// In both.
    Same(String),
    /// Only in the newer revision.
    Added(String),
    /// Only in the older revision.
    Removed(String),
}

/// A line diff of two texts (longest common subsequence). Inputs over 2,000 lines each
/// are compared as two whole blocks.
#[must_use]
pub fn diff_lines(old: &str, new: &str) -> Vec<DiffLine> {
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();
    if a.len() > 2000 || b.len() > 2000 {
        return vec![
            DiffLine::Removed(old.to_owned()),
            DiffLine::Added(new.to_owned()),
        ];
    }
    let mut lcs = vec![vec![0_u16; b.len() + 1]; a.len() + 1];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            lcs[i][j] = if a[i] == b[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    let (mut i, mut j, mut out) = (0, 0, Vec::new());
    while i < a.len() && j < b.len() {
        if a[i] == b[j] {
            out.push(DiffLine::Same(a[i].to_owned()));
            i += 1;
            j += 1;
        } else if lcs[i + 1][j] >= lcs[i][j + 1] {
            out.push(DiffLine::Removed(a[i].to_owned()));
            i += 1;
        } else {
            out.push(DiffLine::Added(b[j].to_owned()));
            j += 1;
        }
    }
    out.extend(a[i..].iter().map(|l| DiffLine::Removed((*l).to_owned())));
    out.extend(b[j..].iter().map(|l| DiffLine::Added((*l).to_owned())));
    out
}

/// A time window on the Change Calendar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    /// The Change.
    pub change_id: Uuid,
    /// Work or outage.
    pub outage: bool,
    /// Start.
    pub start: DateTime<Utc>,
    /// End.
    pub end: DateTime<Utc>,
}

/// Pairs of different Changes whose windows overlap (FR-CHG-012), each pair once.
#[must_use]
pub fn overlaps(windows: &[Window]) -> Vec<(Uuid, Uuid)> {
    let mut out = Vec::new();
    for (i, a) in windows.iter().enumerate() {
        for b in &windows[i + 1..] {
            if a.change_id != b.change_id && a.start < b.end && b.start < a.end {
                let pair = if a.change_id < b.change_id {
                    (a.change_id, b.change_id)
                } else {
                    (b.change_id, a.change_id)
                };
                if !out.contains(&pair) {
                    out.push(pair);
                }
            }
        }
    }
    out
}

macro_rules! change_event {
    ($(#[$meta:meta])* $name:ident, $type:literal) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
        pub struct $name {
            /// The Change.
            pub change_id: Uuid,
            /// Its Company.
            pub tenant_id: TenantId,
            /// The Approval Round.
            pub round_no: i32,
        }

        impl DomainEvent for $name {
            const EVENT_TYPE: &'static str = $type;
            const AGGREGATE_TYPE: &'static str = "change";
            fn aggregate_id(&self) -> Uuid {
                self.change_id
            }
            fn tenant_id(&self) -> Option<TenantId> {
                Some(self.tenant_id)
            }
        }
    };
}

change_event!(
    /// A Change was submitted for approval (a new Approval Round opened).
    ChangeSubmitted, "change.submitted"
);
change_event!(
    /// An Approval Round approved the Change.
    ChangeApproved, "change.approved"
);
change_event!(
    /// An Approval Round rejected the Change; it went back to draft.
    ChangeRejected, "change.rejected"
);
change_event!(
    /// The Change entered implementation (scheduled or under way).
    ChangeScheduled, "change.scheduled"
);
change_event!(
    /// The Change was completed or failed.
    ChangeCompleted, "change.completed"
);

#[cfg(test)]
mod tests {
    use super::*;

    fn lookup(kind: LookupKind, stage: ChangeStage) -> Lookup {
        let mut l = Lookup::create(
            kind,
            LookupInput {
                name: format!("{stage}"),
                stage,
                is_closed: matches!(stage, ChangeStage::Done | ChangeStage::Stopped),
                route: ApprovalRoute::Review,
                is_standard: false,
                is_default: false,
                sort_order: 0,
                is_active: true,
            },
            Utc::now(),
        )
        .unwrap();
        l.kind = kind;
        l
    }

    fn status(stage: ChangeStage) -> Lookup {
        lookup(LookupKind::Status, stage)
    }

    fn change_type(standard: bool, route: ApprovalRoute) -> Lookup {
        let mut t = lookup(LookupKind::Type, ChangeStage::Draft);
        t.is_standard = standard;
        t.route = route;
        t
    }

    fn details() -> ChangeDetails {
        ChangeDetails {
            title: "Upgrade the core switch".into(),
            type_id: Uuid::now_v7(),
            ..ChangeDetails::default()
        }
    }

    fn change() -> Change {
        Change::open(
            TenantId::new(),
            "CHG-00001".into(),
            details(),
            &status(ChangeStage::Draft),
            None,
            Actor::Automation,
            Utc::now(),
        )
        .unwrap()
    }

    #[test]
    fn the_stage_order_is_enforced() {
        let normal = change_type(false, ApprovalRoute::Review);
        let standard = change_type(true, ApprovalRoute::Review);
        let emergency = change_type(false, ApprovalRoute::Emergency);
        use ChangeStage::*;
        for (from, to, t, ok) in [
            (Draft, Awaiting, &normal, true),
            (Awaiting, Draft, &normal, true),
            (Draft, Implementing, &normal, false),
            (Draft, Implementing, &standard, true),
            (Awaiting, Implementing, &normal, false),
            (Awaiting, Implementing, &emergency, true),
            (Awaiting, Approved, &normal, false),
            (Approved, Implementing, &normal, true),
            (Implementing, Done, &normal, true),
            (Draft, Done, &normal, false),
            (Done, Stopped, &normal, false),
            (Approved, Stopped, &normal, true),
            (Stopped, Draft, &normal, true),
        ] {
            assert_eq!(check_stage_move(from, to, t).is_ok(), ok, "{from} → {to}");
        }
    }

    #[test]
    fn risk_levels_come_from_bands() {
        let bands: Vec<RiskLevel> = [
            ("Low", 1, 4),
            ("Medium", 5, 9),
            ("High", 10, 15),
            ("Very High", 16, 20),
            ("Critical", 21, 25),
        ]
        .into_iter()
        .map(|(n, a, b)| RiskLevel {
            id: Uuid::now_v7(),
            name: n.into(),
            min_score: a,
            max_score: b,
            sort_order: 0,
        })
        .collect();
        check_bands(&bands).unwrap();
        assert_eq!(risk_score(3, 4).unwrap(), 12);
        assert_eq!(risk_level(12, &bands).unwrap().name, "High");
        assert_eq!(risk_level(25, &bands).unwrap().name, "Critical");
        assert!(risk_score(0, 3).is_err() && risk_score(3, 6).is_err());
        assert!(check_bands(&bands[1..]).is_err(), "1–4 uncovered");
        let mut bad = details();
        bad.risk_likelihood = Some(3);
        assert!(
            Change::open(
                TenantId::new(),
                "C".into(),
                bad,
                &status(ChangeStage::Draft),
                None,
                Actor::Automation,
                Utc::now()
            )
            .is_err()
        );
    }

    fn vote(a: AnalystId, d: Decision) -> Vote {
        Vote {
            id: Uuid::now_v7(),
            round_no: 1,
            analyst_id: a,
            decision: d,
            comment: String::new(),
            voted_at: Utc::now(),
        }
    }

    #[test]
    fn cab_outcomes_follow_the_approval_type_and_a_counted_rejection_rejects() {
        let (a, b, c, d) = (
            AnalystId::new(),
            AnalystId::new(),
            AnalystId::new(),
            AnalystId::new(),
        );
        let members = vec![
            CabMember {
                analyst_id: a,
                is_required: true,
            },
            CabMember {
                analyst_id: b,
                is_required: true,
            },
            CabMember {
                analyst_id: c,
                is_required: true,
            },
            CabMember {
                analyst_id: d,
                is_required: false,
            },
        ];
        let all = Deciders::Cab(members.clone(), CabApprovalType::All);
        let majority = Deciders::Cab(members, CabApprovalType::Majority);
        let two = [vote(a, Decision::Approve), vote(b, Decision::Approve)];
        assert_eq!(all.outcome(&two), RoundOutcome::Pending);
        assert_eq!(majority.outcome(&two), RoundOutcome::Approved);
        assert_eq!(
            majority.outcome(&[vote(a, Decision::Approve), vote(d, Decision::Approve)]),
            RoundOutcome::Pending,
            "optional members do not count"
        );
        let three = [
            vote(a, Decision::Approve),
            vote(b, Decision::Approve),
            vote(c, Decision::Approve),
        ];
        assert_eq!(all.outcome(&three), RoundOutcome::Approved);
        assert_eq!(
            all.outcome(&[vote(a, Decision::Approve), vote(d, Decision::Reject)]),
            RoundOutcome::Pending,
            "optional members only advise"
        );
        assert_eq!(
            majority.outcome(&[vote(a, Decision::Approve), vote(c, Decision::Reject)]),
            RoundOutcome::Rejected,
            "a required member vetoes"
        );
        assert_eq!(
            Deciders::Approver(a).outcome(&[vote(a, Decision::Reject)]),
            RoundOutcome::Rejected
        );
        assert_eq!(
            Deciders::Anyone.outcome(&[vote(b, Decision::Reject)]),
            RoundOutcome::Rejected
        );
        assert!(all.may_vote(d) && !all.may_vote(AnalystId::new()));
        assert_eq!(
            Deciders::Approver(a).outcome(&[vote(a, Decision::Approve)]),
            RoundOutcome::Approved
        );
        assert_eq!(
            Deciders::Cab(vec![], CabApprovalType::All).outcome(&[]),
            RoundOutcome::Pending
        );
    }

    #[test]
    fn material_edits_after_submission_restart_approval() {
        let normal = change_type(false, ApprovalRoute::Review);
        let mut c = change();
        assert!(
            !c.edit(
                ChangeDetails {
                    test_plan: "ping".into(),
                    ..c.details().clone()
                },
                Utc::now()
            )
            .unwrap(),
            "drafts edit freely"
        );
        assert_eq!(
            c.move_to(&status(ChangeStage::Awaiting), &normal, Utc::now())
                .unwrap(),
            Moved::Submitted
        );
        assert_eq!(c.approval_round(), 1);
        c.decide(
            RoundOutcome::Approved,
            &status(ChangeStage::Approved),
            &status(ChangeStage::Draft),
            Utc::now(),
        );
        assert_eq!(c.stage(), ChangeStage::Approved);
        assert!(c.approved_at().is_some());
        let mut people = c.details().clone();
        people.assigned_to = Some(AnalystId::new());
        assert!(
            !c.edit(people, Utc::now()).unwrap(),
            "people are not material"
        );
        let mut plan = c.details().clone();
        plan.rollback_plan = "restore config".into();
        assert!(c.edit(plan, Utc::now()).unwrap());
        c.restart_approval(&status(ChangeStage::Awaiting), Utc::now());
        assert_eq!(
            (c.stage(), c.approval_round(), c.approved_at()),
            (ChangeStage::Awaiting, 2, None)
        );
        c.decide(
            RoundOutcome::Rejected,
            &status(ChangeStage::Approved),
            &status(ChangeStage::Draft),
            Utc::now(),
        );
        assert_eq!(c.stage(), ChangeStage::Draft);
    }

    #[test]
    fn emergency_changes_take_votes_while_under_way_and_pirs_need_done() {
        let emergency = change_type(false, ApprovalRoute::Emergency);
        let mut c = change();
        c.move_to(&status(ChangeStage::Awaiting), &emergency, Utc::now())
            .unwrap();
        c.move_to(&status(ChangeStage::Implementing), &emergency, Utc::now())
            .unwrap();
        assert!(c.accepts_votes(&emergency));
        let pir = Pir {
            successful: true,
            actual_start: None,
            actual_end: None,
            lessons_learned: String::new(),
            follow_up: String::new(),
            recorded_at: Utc::now(),
        };
        assert!(c.record_pir(pir.clone(), Utc::now()).is_err());
        assert_eq!(
            c.move_to(&status(ChangeStage::Done), &emergency, Utc::now())
                .unwrap(),
            Moved::Finished
        );
        assert!(c.closed_at().is_some());
        c.record_pir(pir, Utc::now()).unwrap();
        assert!(
            c.edit(c.details().clone(), Utc::now()).is_err(),
            "done Changes are not edited"
        );
    }

    #[test]
    fn diffs_and_overlaps() {
        assert_eq!(
            diff_lines("a\nb\nc", "a\nx\nc"),
            vec![
                DiffLine::Same("a".into()),
                DiffLine::Removed("b".into()),
                DiffLine::Added("x".into()),
                DiffLine::Same("c".into())
            ]
        );
        let t = |h: i64| {
            Utc::now()
                .date_naive()
                .and_hms_opt(0, 0, 0)
                .unwrap()
                .and_utc()
                + chrono::Duration::hours(h)
        };
        let (x, y, z) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
        let w = [
            Window {
                change_id: x,
                outage: false,
                start: t(1),
                end: t(3),
            },
            Window {
                change_id: x,
                outage: true,
                start: t(2),
                end: t(3),
            },
            Window {
                change_id: y,
                outage: false,
                start: t(2),
                end: t(4),
            },
            Window {
                change_id: z,
                outage: false,
                start: t(4),
                end: t(5),
            },
        ];
        let pairs = overlaps(&w);
        assert_eq!(pairs.len(), 1, "x–y once; y ends when z starts");
    }
}
