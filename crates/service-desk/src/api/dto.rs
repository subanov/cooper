//! Request and response bodies of the service desk API.

use std::collections::BTreeSet;
use std::str::FromStr;

use chrono::{DateTime, NaiveDate, Utc};
use cooper_kernel::{AnalystId, DepartmentId, Limit, RequesterId, Sensitivity, TeamId, TenantId};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::application::{AppError, LookupInput, TicketDetail, TimelineEntry};
use crate::domain::{
    Category, CategoryInput, ListFilter, Lookup, LookupFlags, Note, SortField, Ticket,
    TicketDetails, TicketError, TimeEntry,
};

/// A page of tickets.
#[derive(Debug, Serialize, ToSchema)]
pub struct TicketPage {
    /// The tickets.
    pub items: Vec<TicketDto>,
    /// Pass as `cursor` for the next page; absent on the last page.
    pub next_cursor: Option<String>,
}

/// A reference-data row (Ticket Status, Priority, Type, Origin or Resolution Code).
#[derive(Debug, Serialize, ToSchema)]
pub struct LookupDto {
    /// ID.
    pub id: Uuid,
    /// Stable key of seeded rows (code relies on keys, not names).
    pub key: Option<String>,
    /// Name.
    pub name: String,
    /// Status: closed.
    pub is_closed: bool,
    /// Status: pauses the SLA.
    pub pauses_sla: bool,
    /// Status or Priority: default for new tickets.
    pub is_default: bool,
    /// Priority: SLA response target (working minutes).
    pub response_minutes: Option<u32>,
    /// Priority: SLA resolution target (working minutes).
    pub resolution_minutes: Option<u32>,
    /// Type: `{TYPE}` token of Ticket Numbers.
    pub number_prefix: Option<String>,
    /// Priority: SLA Calendar (`null`: the default calendar).
    pub sla_calendar_id: Option<Uuid>,
    /// Display order.
    pub sort_order: i32,
    /// Active flag.
    pub is_active: bool,
}

impl From<Lookup> for LookupDto {
    fn from(l: Lookup) -> Self {
        let f = l.flags().clone();
        Self {
            id: l.id(),
            key: l.key().map(str::to_owned),
            name: l.name().to_owned(),
            is_closed: f.is_closed,
            pauses_sla: f.pauses_sla,
            is_default: f.is_default,
            response_minutes: f.response_minutes,
            resolution_minutes: f.resolution_minutes,
            number_prefix: f.number_prefix,
            sla_calendar_id: f.sla_calendar_id,
            sort_order: l.sort_order(),
            is_active: l.is_active(),
        }
    }
}

/// Create or replace a reference-data row. Flags that do not apply to the kind are
/// ignored.
#[derive(Debug, Deserialize, ToSchema)]
pub struct LookupRequest {
    /// Name.
    pub name: String,
    /// Status: closed.
    #[serde(default)]
    pub is_closed: bool,
    /// Status: pauses the SLA.
    #[serde(default)]
    pub pauses_sla: bool,
    /// Status or Priority: default for new tickets.
    #[serde(default)]
    pub is_default: bool,
    /// Priority: SLA response target (working minutes).
    pub response_minutes: Option<u32>,
    /// Priority: SLA resolution target (working minutes).
    pub resolution_minutes: Option<u32>,
    /// Type: `{TYPE}` token of Ticket Numbers.
    pub number_prefix: Option<String>,
    /// Priority: SLA Calendar (`null`: the default calendar).
    pub sla_calendar_id: Option<Uuid>,
    /// Display order.
    #[serde(default)]
    pub sort_order: i32,
    /// Active flag.
    #[serde(default = "yes")]
    pub is_active: bool,
}

const fn yes() -> bool {
    true
}

impl From<LookupRequest> for LookupInput {
    fn from(r: LookupRequest) -> Self {
        Self {
            name: r.name,
            flags: LookupFlags {
                is_closed: r.is_closed,
                pauses_sla: r.pauses_sla,
                is_default: r.is_default,
                response_minutes: r.response_minutes,
                resolution_minutes: r.resolution_minutes,
                number_prefix: r.number_prefix,
                sla_calendar_id: r.sla_calendar_id,
            },
            sort_order: r.sort_order,
            is_active: r.is_active,
        }
    }
}

/// A Category.
#[derive(Debug, Serialize, ToSchema)]
pub struct CategoryDto {
    /// ID.
    pub id: Uuid,
    /// Parent Category.
    pub parent_id: Option<Uuid>,
    /// Name.
    pub name: String,
    /// Level, 1 to 3.
    pub depth: i16,
    /// Shown on the Portal.
    pub portal_visible: bool,
    /// Limited to these Ticket Types (empty = all).
    pub type_ids: Vec<Uuid>,
    /// Display order.
    pub sort_order: i32,
    /// Active flag.
    pub is_active: bool,
}

impl From<Category> for CategoryDto {
    fn from(c: Category) -> Self {
        Self {
            id: c.id(),
            parent_id: c.parent_id(),
            name: c.name().to_owned(),
            depth: c.depth(),
            portal_visible: c.portal_visible(),
            type_ids: c.type_ids().iter().copied().collect(),
            sort_order: c.sort_order(),
            is_active: c.is_active(),
        }
    }
}

/// Create or replace a Category.
#[derive(Debug, Deserialize, ToSchema)]
pub struct CategoryRequest {
    /// Parent (create only).
    pub parent_id: Option<Uuid>,
    /// Name.
    pub name: String,
    /// Shown on the Portal.
    #[serde(default)]
    pub portal_visible: bool,
    /// Limited to these Ticket Types.
    #[serde(default)]
    pub type_ids: Vec<Uuid>,
    /// Display order.
    #[serde(default)]
    pub sort_order: i32,
    /// Active flag.
    #[serde(default = "yes")]
    pub is_active: bool,
}

impl From<CategoryRequest> for CategoryInput {
    fn from(r: CategoryRequest) -> Self {
        Self {
            name: r.name,
            portal_visible: r.portal_visible,
            type_ids: r.type_ids.into_iter().collect(),
            sort_order: r.sort_order,
            is_active: r.is_active,
        }
    }
}

/// A ticket.
#[derive(Debug, Serialize, ToSchema)]
pub struct TicketDto {
    /// ID.
    pub id: Uuid,
    /// Ticket Number.
    pub number: String,
    /// Company.
    pub company_id: Uuid,
    /// Subject.
    pub subject: String,
    /// Description.
    pub description: String,
    /// Requester.
    pub requester_id: Option<Uuid>,
    /// Department.
    pub department_id: Option<Uuid>,
    /// Ticket Type.
    pub type_id: Option<Uuid>,
    /// Category.
    pub category_id: Option<Uuid>,
    /// Priority.
    pub priority_id: Uuid,
    /// Status.
    pub status_id: Uuid,
    /// Origin.
    pub origin_id: Option<Uuid>,
    /// Queue.
    pub team_id: Option<Uuid>,
    /// Assignee.
    pub analyst_id: Option<Uuid>,
    /// `normal` or `confidential`.
    pub sensitivity: String,
    /// First Time Fix flag.
    pub first_time_fix: bool,
    /// Training Provided flag.
    pub training_provided: bool,
    /// Resolution Code (closed tickets).
    pub resolution_code_id: Option<Uuid>,
    /// Closure Category (closed tickets).
    pub closure_category_id: Option<Uuid>,
    /// When it was closed.
    pub closed_at: Option<DateTime<Utc>>,
    /// Snoozed until.
    pub snoozed_until: Option<DateTime<Utc>>,
    /// Snooze reason.
    pub snooze_reason: Option<String>,
    /// When it was deleted (Administrators only see deleted tickets).
    pub deleted_at: Option<DateTime<Utc>>,
    /// Total minutes of Time Entries (single-ticket responses only).
    pub total_minutes: Option<u64>,
    /// SLA state and time remaining (FR-SLA-007).
    pub sla: SlaDto,
    /// Created.
    pub created_at: DateTime<Utc>,
    /// Last changed.
    pub updated_at: DateTime<Utc>,
}

impl From<Ticket> for TicketDto {
    fn from(t: Ticket) -> Self {
        let d = t.details().clone();
        Self {
            id: t.id().as_uuid(),
            number: t.number().to_owned(),
            company_id: t.tenant_id().as_uuid(),
            subject: d.subject,
            description: d.description,
            requester_id: d.requester_id.map(|x| x.as_uuid()),
            department_id: d.department_id.map(|x| x.as_uuid()),
            type_id: d.type_id,
            category_id: d.category_id,
            priority_id: d.priority_id,
            status_id: t.status_id(),
            origin_id: d.origin_id,
            team_id: t.assigned_team_id().map(|x| x.as_uuid()),
            analyst_id: t.assigned_analyst_id().map(|x| x.as_uuid()),
            sensitivity: d.sensitivity.as_str().to_owned(),
            first_time_fix: d.first_time_fix,
            training_provided: d.training_provided,
            resolution_code_id: t.resolution_code_id(),
            closure_category_id: t.closure_category_id(),
            closed_at: t.closed_at(),
            snoozed_until: t.snoozed_until(),
            snooze_reason: t.snooze_reason().map(str::to_owned),
            deleted_at: t.deleted_at(),
            total_minutes: None,
            sla: SlaDto::default(),
            created_at: t.created_at(),
            updated_at: t.updated_at(),
        }
    }
}

impl TicketDto {
    /// The ticket with its SLA state at `now`.
    #[must_use]
    pub fn at(t: Ticket, now: DateTime<Utc>) -> Self {
        let sla = SlaDto::of(&t, now);
        let mut dto = Self::from(t);
        dto.sla = sla;
        dto
    }
}

/// One SLA target.
#[derive(Debug, Default, Serialize, ToSchema)]
pub struct SlaTargetDto {
    /// `none`, `on_track`, `warning`, `breached`, `paused`, `met` or `met_late`.
    pub state: String,
    /// When it is due.
    pub due_at: Option<DateTime<Utc>>,
    /// Seconds until due (negative when overdue); absent when met, paused or without a
    /// target.
    pub remaining_seconds: Option<i64>,
}

/// SLA state of a ticket.
#[derive(Debug, Default, Serialize, ToSchema)]
pub struct SlaDto {
    /// Response target.
    pub response: SlaTargetDto,
    /// Resolution target.
    pub resolution: SlaTargetDto,
    /// When the clock was paused, if it is.
    pub paused_at: Option<DateTime<Utc>>,
    /// When the first response was recorded.
    pub responded_at: Option<DateTime<Utc>>,
}

impl SlaDto {
    fn of(t: &Ticket, now: DateTime<Utc>) -> Self {
        use crate::domain::TargetState;
        let (response, resolution) = t.sla_state(now);
        let target = |state: TargetState, due: Option<DateTime<Utc>>| {
            let running = matches!(
                state,
                TargetState::OnTrack | TargetState::Warning | TargetState::Breached
            );
            SlaTargetDto {
                state: serde_json::to_value(state)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_default(),
                due_at: due,
                remaining_seconds: due.filter(|_| running).map(|d| (d - now).num_seconds()),
            }
        };
        let s = t.sla();
        Self {
            response: target(response, s.response_due_at),
            resolution: target(resolution, s.resolution_due_at),
            paused_at: s.paused_at,
            responded_at: s.responded_at,
        }
    }
}

impl TicketDto {
    /// A single ticket with its total time and SLA state at `now`.
    #[must_use]
    pub fn detail(d: TicketDetail, now: DateTime<Utc>) -> Self {
        let mut dto = Self::at(d.ticket, now);
        dto.total_minutes = Some(d.total_minutes);
        dto
    }
}

/// Create or edit a ticket.
#[derive(Debug, Deserialize, ToSchema)]
pub struct TicketRequest {
    /// Company (create only).
    pub company_id: Option<Uuid>,
    /// Subject.
    pub subject: String,
    /// Description.
    #[serde(default)]
    pub description: String,
    /// Requester.
    pub requester_id: Option<Uuid>,
    /// Department.
    pub department_id: Option<Uuid>,
    /// Ticket Type.
    pub type_id: Option<Uuid>,
    /// Category.
    pub category_id: Option<Uuid>,
    /// Priority (default Priority when absent).
    pub priority_id: Option<Uuid>,
    /// Origin.
    pub origin_id: Option<Uuid>,
    /// `normal` or `confidential` (the Department's default when absent).
    pub sensitivity: Option<String>,
    /// First Time Fix flag.
    #[serde(default)]
    pub first_time_fix: bool,
    /// Training Provided flag.
    #[serde(default)]
    pub training_provided: bool,
    /// Queue (create only; use the assignment endpoint afterwards).
    pub team_id: Option<Uuid>,
    /// Assignee (create only).
    pub analyst_id: Option<Uuid>,
}

impl TicketRequest {
    /// The details and whether Sensitivity was given.
    ///
    /// # Errors
    ///
    /// An unknown Sensitivity.
    pub fn details(&self) -> Result<(TicketDetails, bool), AppError> {
        let sensitivity = self
            .sensitivity
            .as_deref()
            .map(Sensitivity::from_str)
            .transpose()
            .map_err(|_| TicketError::invalid("sensitivity", "must be normal or confidential"))?;
        Ok((
            TicketDetails {
                subject: self.subject.clone(),
                description: self.description.clone(),
                requester_id: self.requester_id.map(RequesterId::from_uuid),
                department_id: self.department_id.map(DepartmentId::from_uuid),
                type_id: self.type_id,
                category_id: self.category_id,
                priority_id: self.priority_id.unwrap_or_default(),
                origin_id: self.origin_id,
                sensitivity: sensitivity.unwrap_or_default(),
                first_time_fix: self.first_time_fix,
                training_provided: self.training_provided,
            },
            sensitivity.is_some(),
        ))
    }
}

/// Move a ticket to another status.
#[derive(Debug, Deserialize, ToSchema)]
pub struct StatusRequest {
    /// New status.
    pub status_id: Uuid,
    /// Required when closing.
    pub resolution_code_id: Option<Uuid>,
    /// Closure Category (required when closing, if configured).
    pub closure_category_id: Option<Uuid>,
}

/// Assign a ticket.
#[derive(Debug, Deserialize, ToSchema)]
pub struct AssignmentRequest {
    /// Queue; `null` to clear.
    pub team_id: Option<Uuid>,
    /// Assignee; `null` to clear.
    pub analyst_id: Option<Uuid>,
}

/// Snooze a ticket.
#[derive(Debug, Deserialize, ToSchema)]
pub struct SnoozeRequest {
    /// Hidden from default views until then.
    pub until: DateTime<Utc>,
    /// Why.
    #[serde(default)]
    pub reason: String,
}

/// Add a Note.
#[derive(Debug, Deserialize, ToSchema)]
pub struct NoteRequest {
    /// Text.
    pub body: String,
    /// Visible to the Requester (default: internal).
    #[serde(default)]
    pub is_public: bool,
}

/// A Note.
#[derive(Debug, Serialize, ToSchema)]
pub struct NoteDto {
    /// ID.
    pub id: Uuid,
    /// Author.
    pub author_id: Option<Uuid>,
    /// Text.
    pub body: String,
    /// Visible to the Requester.
    pub is_public: bool,
    /// Written.
    pub created_at: DateTime<Utc>,
}

impl From<Note> for NoteDto {
    fn from(n: Note) -> Self {
        Self {
            id: n.id(),
            author_id: n.author_id().map(|a| a.as_uuid()),
            body: n.body().to_owned(),
            is_public: n.is_public(),
            created_at: n.created_at(),
        }
    }
}

/// Record time.
#[derive(Debug, Deserialize, ToSchema)]
pub struct TimeEntryRequest {
    /// Minutes, 1 to 1440.
    pub minutes: u32,
    /// Day worked (today when absent).
    pub work_date: Option<NaiveDate>,
    /// Note.
    #[serde(default)]
    pub note: String,
}

/// A Time Entry.
#[derive(Debug, Serialize, ToSchema)]
pub struct TimeEntryDto {
    /// ID.
    pub id: Uuid,
    /// Analyst.
    pub analyst_id: Uuid,
    /// Minutes.
    pub minutes: u32,
    /// Day worked.
    pub work_date: NaiveDate,
    /// Note.
    pub note: String,
    /// Recorded.
    pub created_at: DateTime<Utc>,
}

impl From<TimeEntry> for TimeEntryDto {
    fn from(e: TimeEntry) -> Self {
        Self {
            id: e.id(),
            analyst_id: e.analyst_id().as_uuid(),
            minutes: e.minutes(),
            work_date: e.work_date(),
            note: e.note().to_owned(),
            created_at: e.created_at(),
        }
    }
}

/// One timeline item.
#[derive(Debug, Serialize, ToSchema)]
pub struct TimelineItemDto {
    /// `note`, `time_entry` or `change`.
    pub kind: String,
    /// When.
    pub at: DateTime<Utc>,
    /// Who (`{kind, id}`).
    #[schema(value_type = Object)]
    pub actor: serde_json::Value,
    /// The Note, Time Entry or change (`{action, changes: [{field, old_value, new_value}]}`).
    #[schema(value_type = Object)]
    pub data: serde_json::Value,
}

impl From<TimelineEntry> for TimelineItemDto {
    fn from(e: TimelineEntry) -> Self {
        let at = e.at();
        match e {
            TimelineEntry::Note(n) => Self {
                kind: "note".into(),
                at,
                actor: serde_json::json!({ "kind": "analyst", "id": n.author_id() }),
                data: serde_json::to_value(NoteDto::from(n)).unwrap_or_default(),
            },
            TimelineEntry::Time(t) => Self {
                kind: "time_entry".into(),
                at,
                actor: serde_json::json!({ "kind": "analyst", "id": t.analyst_id() }),
                data: serde_json::to_value(TimeEntryDto::from(t)).unwrap_or_default(),
            },
            TimelineEntry::Email(e) => Self {
                kind: "email".into(),
                at,
                actor: serde_json::json!({ "kind": if e.author_id.is_some() { "analyst" } else { "requester" }, "id": e.author_id }),
                data: serde_json::to_value(TicketEmailDto::from(e)).unwrap_or_default(),
            },
            TimelineEntry::Change(a) => Self {
                kind: "change".into(),
                at,
                actor: serde_json::to_value(a.actor).unwrap_or_default(),
                data: serde_json::json!({ "action": a.action, "changes": a.changes }),
            },
        }
    }
}

/// Ticket list parameters (FR-GEN-003, FR-TKT-040, FR-TKT-041). ID filters accept
/// comma-separated lists.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct TicketListParams {
    /// Built-in view: `mine`, `queue`, `unassigned`, `open`, `snoozed`,
    /// `recently_closed` or `deleted` (Administrators).
    pub view: Option<String>,
    /// Search: Ticket Number prefix, subject, description and Notes.
    pub q: Option<String>,
    /// Companies.
    pub company_id: Option<String>,
    /// Statuses.
    pub status_id: Option<String>,
    /// `true`: open only; `false`: closed only.
    pub open: Option<bool>,
    /// Priorities.
    pub priority_id: Option<String>,
    /// Ticket Types.
    pub type_id: Option<String>,
    /// Queues.
    pub team_id: Option<String>,
    /// Assignees.
    pub analyst_id: Option<String>,
    /// Only unassigned tickets.
    #[serde(default)]
    pub unassigned: bool,
    /// Requester.
    pub requester_id: Option<Uuid>,
    /// Department.
    pub department_id: Option<Uuid>,
    /// Category.
    pub category_id: Option<Uuid>,
    /// `normal` or `confidential`.
    pub sensitivity: Option<String>,
    /// `created_at` (default), `updated_at` or `number`.
    pub sort: Option<String>,
    /// `desc` (default) or `asc`.
    pub order: Option<String>,
    /// Page size, 1 to 200 (default 50).
    pub limit: Option<u32>,
    /// Cursor from the previous page.
    pub cursor: Option<String>,
}

fn ids(field: &'static str, raw: Option<&str>) -> Result<BTreeSet<Uuid>, AppError> {
    raw.unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| {
            s.parse().map_err(|_| {
                AppError::from(TicketError::invalid(
                    field,
                    "must be UUIDs separated by commas",
                ))
            })
        })
        .collect()
}

impl TryFrom<TicketListParams> for crate::application::ListRequest {
    type Error = AppError;

    fn try_from(p: TicketListParams) -> Result<Self, AppError> {
        let limit = match p.limit {
            None => Limit::DEFAULT,
            Some(n) => Limit::new(n)
                .map_err(|_| TicketError::invalid("limit", "must be between 1 and 200"))?,
        };
        let sort = match p.sort.as_deref() {
            None | Some("created_at") => SortField::CreatedAt,
            Some("updated_at") => SortField::UpdatedAt,
            Some("number") => SortField::Number,
            Some(_) => {
                return Err(TicketError::invalid(
                    "sort",
                    "must be created_at, updated_at or number",
                )
                .into());
            }
        };
        let ascending = match p.order.as_deref() {
            None | Some("desc") => false,
            Some("asc") => true,
            Some(_) => return Err(TicketError::invalid("order", "must be asc or desc").into()),
        };
        let sensitivity = p
            .sensitivity
            .as_deref()
            .map(Sensitivity::from_str)
            .transpose()
            .map_err(|_| TicketError::invalid("sensitivity", "must be normal or confidential"))?;
        Ok(Self {
            view: p.view.as_deref().map(str::parse).transpose()?,
            filter: ListFilter {
                tenant_ids: ids("company_id", p.company_id.as_deref())?
                    .into_iter()
                    .map(TenantId::from_uuid)
                    .collect(),
                status_ids: ids("status_id", p.status_id.as_deref())?,
                open: p.open,
                priority_ids: ids("priority_id", p.priority_id.as_deref())?,
                type_ids: ids("type_id", p.type_id.as_deref())?,
                team_ids: ids("team_id", p.team_id.as_deref())?
                    .into_iter()
                    .map(TeamId::from_uuid)
                    .collect(),
                analyst_ids: ids("analyst_id", p.analyst_id.as_deref())?
                    .into_iter()
                    .map(AnalystId::from_uuid)
                    .collect(),
                unassigned: p.unassigned,
                requester_id: p.requester_id.map(RequesterId::from_uuid),
                department_id: p.department_id.map(DepartmentId::from_uuid),
                category_id: p.category_id,
                sensitivity,
                search: p.q,
                ..ListFilter::default()
            },
            sort,
            ascending,
            limit,
            cursor: p.cursor.map(cooper_kernel::Cursor::from_client),
        })
    }
}

/// An SLA Calendar.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct SlaCalendarDto {
    /// ID (ignored in requests).
    #[serde(default)]
    pub id: Option<Uuid>,
    /// Name.
    pub name: String,
    /// IANA time zone, for example `Europe/London`.
    pub time_zone: String,
    /// Working hours Monday to Sunday: seven entries, each `{start, end}` (`HH:MM`) or
    /// `null` for a day off.
    pub hours: Vec<Option<HoursDto>>,
    /// Holidays (local dates).
    #[serde(default)]
    pub holidays: Vec<NaiveDate>,
    /// Used for Priorities without their own calendar.
    #[serde(default)]
    pub is_default: bool,
}

/// Working hours of one day.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema)]
pub struct HoursDto {
    /// Start, `HH:MM`.
    #[schema(value_type = String)]
    pub start: chrono::NaiveTime,
    /// End, `HH:MM`.
    #[schema(value_type = String)]
    pub end: chrono::NaiveTime,
}

impl From<crate::domain::SlaCalendar> for SlaCalendarDto {
    fn from(c: crate::domain::SlaCalendar) -> Self {
        Self {
            id: Some(c.id()),
            name: c.name().to_owned(),
            time_zone: c.time_zone().name().to_owned(),
            hours: c
                .hours()
                .iter()
                .map(|h| {
                    h.map(|h| HoursDto {
                        start: h.start,
                        end: h.end,
                    })
                })
                .collect(),
            holidays: c.holidays().iter().copied().collect(),
            is_default: c.is_default(),
        }
    }
}

impl TryFrom<SlaCalendarDto> for crate::domain::SlaCalendarInput {
    type Error = AppError;
    fn try_from(d: SlaCalendarDto) -> Result<Self, AppError> {
        let time_zone = d
            .time_zone
            .parse()
            .map_err(|_| TicketError::invalid("time_zone", "is not an IANA time zone"))?;
        if d.hours.len() != 7 {
            return Err(
                TicketError::invalid("hours", "needs seven entries, Monday to Sunday").into(),
            );
        }
        let mut hours = [None; 7];
        for (slot, h) in hours.iter_mut().zip(d.hours) {
            *slot = h.map(|h| crate::domain::DayHours {
                start: h.start,
                end: h.end,
            });
        }
        Ok(Self {
            name: d.name,
            time_zone,
            hours,
            holidays: d.holidays.into_iter().collect(),
            is_default: d.is_default,
        })
    }
}

/// An SLA Notification Rule.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct SlaRuleDto {
    /// ID (ignored in requests).
    #[serde(default)]
    pub id: Option<Uuid>,
    /// Name.
    pub name: String,
    /// `warning` or `breach`.
    pub trigger: String,
    /// `response`, `resolution` or `both`.
    pub target: String,
    /// Notify the Assignee.
    #[serde(default)]
    pub notify_assignee: bool,
    /// Notify the Queue's members.
    #[serde(default)]
    pub notify_team: bool,
    /// Also notify these Analysts.
    #[serde(default)]
    pub analyst_ids: Vec<Uuid>,
    /// Active flag.
    #[serde(default = "yes")]
    pub is_active: bool,
}

impl From<crate::domain::SlaNotificationRule> for SlaRuleDto {
    fn from(r: crate::domain::SlaNotificationRule) -> Self {
        let i = r.rule();
        Self {
            id: Some(r.id()),
            name: i.name.clone(),
            trigger: i.trigger.as_str().to_owned(),
            target: i.target.as_str().to_owned(),
            notify_assignee: i.notify_assignee,
            notify_team: i.notify_team,
            analyst_ids: i.analyst_ids.iter().map(|a| a.as_uuid()).collect(),
            is_active: i.is_active,
        }
    }
}

impl TryFrom<SlaRuleDto> for crate::domain::SlaRuleInput {
    type Error = AppError;
    fn try_from(d: SlaRuleDto) -> Result<Self, AppError> {
        use crate::domain::{RuleTarget, SlaTrigger};
        let trigger = match d.trigger.as_str() {
            "warning" => SlaTrigger::Warning,
            "breach" => SlaTrigger::Breach,
            _ => return Err(TicketError::invalid("trigger", "must be warning or breach").into()),
        };
        let target = match d.target.as_str() {
            "response" => RuleTarget::Response,
            "resolution" => RuleTarget::Resolution,
            "both" => RuleTarget::Both,
            _ => {
                return Err(
                    TicketError::invalid("target", "must be response, resolution or both").into(),
                );
            }
        };
        Ok(Self {
            name: d.name,
            trigger,
            target,
            notify_assignee: d.notify_assignee,
            notify_team: d.notify_team,
            analyst_ids: d
                .analyst_ids
                .into_iter()
                .map(AnalystId::from_uuid)
                .collect(),
            is_active: d.is_active,
        })
    }
}

/// An email on a ticket.
#[derive(Debug, Serialize, ToSchema)]
pub struct TicketEmailDto {
    /// ID.
    pub id: Uuid,
    /// `inbound` or `outbound`.
    pub direction: String,
    /// `From`.
    pub from: String,
    /// `To`.
    pub to: Vec<String>,
    /// `Cc`.
    pub cc: Vec<String>,
    /// Subject.
    pub subject: String,
    /// Plain-text body.
    pub text_body: String,
    /// `Message-ID`.
    pub message_id: String,
    /// Author (outbound).
    pub author_id: Option<Uuid>,
    /// Sent or received.
    pub occurred_at: DateTime<Utc>,
}

impl From<crate::domain::TicketEmail> for TicketEmailDto {
    fn from(e: crate::domain::TicketEmail) -> Self {
        Self {
            id: e.id,
            direction: e.direction.as_str().to_owned(),
            from: e.from,
            to: e.to,
            cc: e.cc,
            subject: e.subject,
            text_body: e.text_body,
            message_id: e.message_id,
            author_id: e.author_id.map(|a| a.as_uuid()),
            occurred_at: e.occurred_at,
        }
    }
}

/// A Reply to the Requester.
#[derive(Debug, Deserialize, ToSchema)]
pub struct ReplyRequest {
    /// Text; your Signature is appended.
    pub body: String,
    /// Copy recipients.
    #[serde(default)]
    pub cc: Vec<String>,
}

/// A Reply Template.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ReplyTemplateDto {
    /// ID (ignored in requests).
    #[serde(default)]
    pub id: Option<Uuid>,
    /// Name.
    pub name: String,
    /// Body with placeholders: `{requester_name}`, `{ticket_number}`, `{ticket_subject}`,
    /// `{analyst_name}`.
    pub body: String,
    /// Active flag.
    #[serde(default = "yes")]
    pub is_active: bool,
}

impl From<crate::domain::ReplyTemplate> for ReplyTemplateDto {
    fn from(t: crate::domain::ReplyTemplate) -> Self {
        Self {
            id: Some(t.id()),
            name: t.name().to_owned(),
            body: t.body().to_owned(),
            is_active: t.is_active(),
        }
    }
}

/// A rendered Reply Template.
#[derive(Debug, Serialize, ToSchema)]
pub struct RenderedTemplateDto {
    /// The text with placeholders filled in.
    pub text: String,
}

/// An attachment.
#[derive(Debug, Serialize, ToSchema)]
pub struct AttachmentDto {
    /// ID.
    pub id: Uuid,
    /// File name.
    pub file_name: String,
    /// Media type detected from the content.
    pub media_type: String,
    /// Size in bytes.
    pub size_bytes: i64,
    /// Uploaded.
    pub created_at: DateTime<Utc>,
}

impl From<crate::domain::AttachmentInfo> for AttachmentDto {
    fn from(a: crate::domain::AttachmentInfo) -> Self {
        Self {
            id: a.id,
            file_name: a.file_name,
            media_type: a.media_type,
            size_bytes: a.size_bytes,
            created_at: a.created_at,
        }
    }
}
