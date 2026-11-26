//! Forms domain (FR-FRM-001 to FR-FRM-005, ADR-0039). Pure Rust: no I/O, no clock calls.

use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;

use chrono::{DateTime, NaiveDate, Utc};
use cooper_kernel::{Actor, AnalystId, DepartmentId, RequesterId, TeamId, TenantId};
use serde_json::{Value, json};
use thiserror::Error;
use uuid::Uuid;

cooper_kernel::define_id!(
    /// A Form (FR-FRM-001).
    FormId
);

cooper_kernel::define_id!(
    /// A Submission of a Form (FR-FRM-003).
    SubmissionId
);

/// A Forms rule was violated.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum FormsError {
    /// A value failed validation.
    #[error("{field}: {reason}")]
    Invalid {
        /// The offending field.
        field: &'static str,
        /// Why it is not valid.
        reason: String,
    },
    /// The action is not allowed in the current state.
    #[error("{0}")]
    InvalidState(String),
}

impl FormsError {
    /// An invalid value.
    pub fn invalid(field: &'static str, reason: impl Into<String>) -> Self {
        Self::Invalid {
            field,
            reason: reason.into(),
        }
    }
}

/// Most Form Fields in one version.
pub const MAX_FIELDS: usize = 100;
/// Most options of a choice field.
pub const MAX_OPTIONS: usize = 50;
/// Longest option, in characters.
pub const MAX_OPTION_LEN: usize = 100;
/// Longest Form name, in characters.
pub const MAX_NAME_LEN: usize = 150;
/// Longest Form description, in characters.
pub const MAX_DESCRIPTION_LEN: usize = 2000;
/// Longest answer to a text field, in characters.
pub const MAX_TEXT_LEN: usize = 500;
/// Longest answer to a long text field, in characters.
pub const MAX_LONG_TEXT_LEN: usize = 10_000;
/// Longest template of a Submission Action, in characters.
pub const MAX_TEMPLATE_LEN: usize = 10_000;
/// Most approvers, and most required approvals or rejections.
pub const MAX_APPROVERS: usize = 50;
/// Longest comment on a vote, in characters.
pub const MAX_COMMENT_LEN: usize = 2000;

/// The type of a Form Field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FieldKind {
    /// One line of text.
    Text,
    /// Several lines of text.
    LongText,
    /// A number.
    Number,
    /// A calendar date (`YYYY-MM-DD`).
    Date,
    /// Yes or no.
    Boolean,
    /// One option out of a list.
    Choice,
}

impl FieldKind {
    /// The stored name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::LongText => "long_text",
            Self::Number => "number",
            Self::Date => "date",
            Self::Boolean => "boolean",
            Self::Choice => "choice",
        }
    }

    /// Only a choice field has options.
    #[must_use]
    pub const fn takes_options(self) -> bool {
        matches!(self, Self::Choice)
    }
}

impl FromStr for FieldKind {
    type Err = FormsError;

    fn from_str(s: &str) -> Result<Self, FormsError> {
        Ok(match s {
            "text" => Self::Text,
            "long_text" => Self::LongText,
            "number" => Self::Number,
            "date" => Self::Date,
            "boolean" => Self::Boolean,
            "choice" => Self::Choice,
            other => {
                return Err(FormsError::invalid(
                    "kind",
                    format!("{other:?} is not a field type"),
                ));
            }
        })
    }
}

/// How wide a Form Field is in the layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FieldWidth {
    /// The whole row.
    #[default]
    Full,
    /// Half of the row.
    Half,
}

impl FieldWidth {
    /// The stored name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Half => "half",
        }
    }
}

impl FromStr for FieldWidth {
    type Err = FormsError;

    fn from_str(s: &str) -> Result<Self, FormsError> {
        match s {
            "full" => Ok(Self::Full),
            "half" => Ok(Self::Half),
            other => Err(FormsError::invalid(
                "width",
                format!("{other:?} is not a width (full or half)"),
            )),
        }
    }
}

/// What an Administrator enters for one Form Field, before validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormFieldInput {
    /// Stable identifier within the version: a lower-case letter, then lower-case letters,
    /// digits or underscores (at most 50 characters).
    pub key: String,
    /// Label shown to the Requester (1 to 255 characters).
    pub label: String,
    /// Field type.
    pub kind: FieldKind,
    /// Whether an answer is required.
    pub required: bool,
    /// Options of a choice field; must be empty for other types.
    pub options: Vec<String>,
    /// Layout width.
    pub width: FieldWidth,
}

/// A validated Form Field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormField {
    key: String,
    label: String,
    kind: FieldKind,
    required: bool,
    options: Vec<String>,
    width: FieldWidth,
}

impl FormField {
    /// Validates a field.
    ///
    /// # Errors
    ///
    /// A key, label or option set that breaks the rules above.
    pub fn new(input: FormFieldInput) -> Result<Self, FormsError> {
        let key = input.key.trim().to_owned();
        if !valid_key(&key) {
            return Err(FormsError::invalid(
                "key",
                "must be 1 to 50 characters: a lower-case letter, then lower-case letters, digits or underscores",
            ));
        }
        let label = input.label.trim().to_owned();
        if label.is_empty() || label.chars().count() > 255 {
            return Err(FormsError::invalid("label", "must be 1 to 255 characters"));
        }
        let options = normalise_options(input.kind, input.options)?;
        Ok(Self {
            key,
            label,
            kind: input.kind,
            required: input.required,
            options,
            width: input.width,
        })
    }

    /// The key.
    #[must_use]
    pub fn key(&self) -> &str {
        &self.key
    }

    /// The label.
    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }

    /// The type.
    #[must_use]
    pub const fn kind(&self) -> FieldKind {
        self.kind
    }

    /// Whether an answer is required.
    #[must_use]
    pub const fn required(&self) -> bool {
        self.required
    }

    /// The options of a choice field, in the order given.
    #[must_use]
    pub fn options(&self) -> &[String] {
        &self.options
    }

    /// The layout width.
    #[must_use]
    pub const fn width(&self) -> FieldWidth {
        self.width
    }

    /// The field as recorded in an audit entry.
    #[must_use]
    pub fn audit_value(&self) -> Value {
        json!({
            "key": self.key,
            "label": self.label,
            "kind": self.kind.as_str(),
            "required": self.required,
            "options": self.options,
            "width": self.width.as_str(),
        })
    }

    /// Checks one answer against this field (already trimmed, not empty).
    fn check_answer(&self, value: &str) -> Result<(), FormsError> {
        let fail = |reason: &str| {
            Err(FormsError::invalid(
                "answers",
                format!("{}: {reason}", self.key),
            ))
        };
        match self.kind {
            FieldKind::Text if value.chars().count() > MAX_TEXT_LEN => {
                fail("is longer than 500 characters")
            }
            FieldKind::LongText if value.chars().count() > MAX_LONG_TEXT_LEN => {
                fail("is longer than 10000 characters")
            }
            FieldKind::Text | FieldKind::LongText => Ok(()),
            FieldKind::Number => match value.parse::<f64>() {
                Ok(n) if n.is_finite() => Ok(()),
                _ => fail("is not a number"),
            },
            FieldKind::Date => match NaiveDate::parse_from_str(value, "%Y-%m-%d") {
                Ok(_) => Ok(()),
                Err(_) => fail("is not a date (YYYY-MM-DD)"),
            },
            FieldKind::Boolean => match value {
                "true" | "false" => Ok(()),
                _ => fail("is not true or false"),
            },
            FieldKind::Choice => {
                if self.options.iter().any(|o| o == value) {
                    Ok(())
                } else {
                    fail("is not one of the options")
                }
            }
        }
    }
}

fn valid_key(key: &str) -> bool {
    let mut chars = key.chars();
    key.len() <= 50
        && matches!(chars.next(), Some(c) if c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn normalise_options(kind: FieldKind, options: Vec<String>) -> Result<Vec<String>, FormsError> {
    if !kind.takes_options() {
        if options.iter().any(|o| !o.trim().is_empty()) {
            return Err(FormsError::invalid(
                "options",
                "only a choice field takes options",
            ));
        }
        return Ok(Vec::new());
    }
    let mut out: Vec<String> = Vec::with_capacity(options.len());
    for raw in options {
        let option = raw.trim().to_owned();
        if option.is_empty() || option.chars().count() > MAX_OPTION_LEN {
            return Err(FormsError::invalid(
                "options",
                "each option must be 1 to 100 characters",
            ));
        }
        let lower = option.to_lowercase();
        if out.iter().any(|x| x.to_lowercase() == lower) {
            return Err(FormsError::invalid("options", "options must be unique"));
        }
        out.push(option);
    }
    if out.is_empty() || out.len() > MAX_OPTIONS {
        return Err(FormsError::invalid(
            "options",
            "a choice field needs 1 to 50 options",
        ));
    }
    Ok(out)
}

/// Answers by field key, as the Requester gave them.
pub type Answers = BTreeMap<String, String>;

/// One immutable version of a Form's fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormVersion {
    number: u32,
    fields: Vec<FormField>,
    created_by: Actor,
    created_at: DateTime<Utc>,
}

impl FormVersion {
    /// Validates a version: 1 to 100 fields with distinct keys.
    ///
    /// # Errors
    ///
    /// No fields, too many fields, a repeated key, or an invalid field.
    pub fn new(
        number: u32,
        inputs: Vec<FormFieldInput>,
        created_by: Actor,
        created_at: DateTime<Utc>,
    ) -> Result<Self, FormsError> {
        if inputs.is_empty() || inputs.len() > MAX_FIELDS {
            return Err(FormsError::invalid("fields", "a form needs 1 to 100 fields"));
        }
        let mut fields: Vec<FormField> = Vec::with_capacity(inputs.len());
        for input in inputs {
            let field = FormField::new(input)?;
            if fields.iter().any(|f| f.key == field.key) {
                return Err(FormsError::invalid(
                    "fields",
                    format!("the key {:?} is used by more than one field", field.key),
                ));
            }
            fields.push(field);
        }
        Ok(Self {
            number,
            fields,
            created_by,
            created_at,
        })
    }

    /// Version number, from 1.
    #[must_use]
    pub const fn number(&self) -> u32 {
        self.number
    }

    /// The fields, in layout order.
    #[must_use]
    pub fn fields(&self) -> &[FormField] {
        &self.fields
    }

    /// Who published the version.
    #[must_use]
    pub const fn created_by(&self) -> Actor {
        self.created_by
    }

    /// When the version was published.
    #[must_use]
    pub const fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }

    /// Checks answers against this version. Unknown keys are rejected; blank answers are
    /// dropped; every other answer must fit its field. With `complete`, every required
    /// field must be answered (a Submission); without, a Draft may be partial.
    ///
    /// # Errors
    ///
    /// An unknown key, a value that does not fit its field, or a missing required answer.
    pub fn check_answers(&self, answers: &Answers, complete: bool) -> Result<Answers, FormsError> {
        let mut out = Answers::new();
        for (key, raw) in answers {
            let Some(field) = self.fields.iter().find(|f| &f.key == key) else {
                return Err(FormsError::invalid(
                    "answers",
                    format!("{key:?} is not a field of this form"),
                ));
            };
            let value = raw.trim();
            if value.is_empty() {
                continue;
            }
            field.check_answer(value)?;
            out.insert(key.clone(), value.to_owned());
        }
        if complete {
            for field in &self.fields {
                if field.required && !out.contains_key(&field.key) {
                    return Err(FormsError::invalid(
                        "answers",
                        format!("{} is required", field.key),
                    ));
                }
            }
        }
        Ok(out)
    }
}

/// The name and description of a Form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormDetails {
    name: String,
    description: String,
}

impl FormDetails {
    /// Validates the details.
    ///
    /// # Errors
    ///
    /// An empty or too long name, or too long a description.
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Result<Self, FormsError> {
        let name = name.into().trim().to_owned();
        if name.is_empty() || name.chars().count() > MAX_NAME_LEN {
            return Err(FormsError::invalid("name", "must be 1 to 150 characters"));
        }
        let description = description.into().trim().to_owned();
        if description.chars().count() > MAX_DESCRIPTION_LEN {
            return Err(FormsError::invalid(
                "description",
                "must be at most 2000 characters",
            ));
        }
        Ok(Self { name, description })
    }

    /// The name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The description.
    #[must_use]
    pub fn description(&self) -> &str {
        &self.description
    }
}

// ----- Portal settings, approval and actions -------------------------------------------

/// What a Form audience is made of (FR-FRM-002).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum AudienceKind {
    /// One Requester.
    Requester,
    /// A User Group of Requesters.
    Group,
    /// A Department (of its Requesters).
    Department,
}

impl AudienceKind {
    /// The stored name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Requester => "requester",
            Self::Group => "group",
            Self::Department => "department",
        }
    }
}

impl FromStr for AudienceKind {
    type Err = FormsError;

    fn from_str(s: &str) -> Result<Self, FormsError> {
        match s {
            "requester" => Ok(Self::Requester),
            "group" => Ok(Self::Group),
            "department" => Ok(Self::Department),
            other => Err(FormsError::invalid(
                "audiences",
                format!("{other:?} is not an audience kind"),
            )),
        }
    }
}

/// One audience of a Portal-visible Form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Audience {
    /// What the ID refers to.
    pub kind: AudienceKind,
    /// The Requester, User Group or Department.
    pub id: Uuid,
}

/// Whether and to whom a Form is offered on the Self-Service Portal (FR-FRM-002).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PortalSettings {
    visible: bool,
    audiences: Vec<Audience>,
}

impl PortalSettings {
    /// Validated settings. A visible Form without audiences is offered to every Requester
    /// of its Company; with audiences, only to the members of those audiences.
    ///
    /// # Errors
    ///
    /// Duplicate audiences.
    pub fn new(visible: bool, audiences: Vec<Audience>) -> Result<Self, FormsError> {
        let mut seen = BTreeSet::new();
        for a in &audiences {
            if !seen.insert(*a) {
                return Err(FormsError::invalid("audiences", "an audience is listed twice"));
            }
        }
        Ok(Self {
            visible,
            audiences,
        })
    }

    /// Whether the Form is on the Portal at all.
    #[must_use]
    pub const fn visible(&self) -> bool {
        self.visible
    }

    /// The audiences (empty: every Requester of the Company).
    #[must_use]
    pub fn audiences(&self) -> &[Audience] {
        &self.audiences
    }
}

/// Who may approve a Submission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Approvers {
    /// Named Analysts.
    Analysts(Vec<AnalystId>),
    /// Members of an approval Team.
    Team(TeamId),
}

/// The approval rule of a Form (FR-FRM-004).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalRule {
    approvers: Approvers,
    required_approvals: u32,
    required_rejections: u32,
    exclude_requester: bool,
}

impl ApprovalRule {
    /// Validated rule. A Submission is approved once `required_approvals` approvals are
    /// cast in its round, and returned to the Requester once `required_rejections`
    /// rejections are cast in that round.
    ///
    /// # Errors
    ///
    /// No or too many approvers, duplicate approvers, or counts outside 1 to 50.
    pub fn new(
        approvers: Approvers,
        required_approvals: u32,
        required_rejections: u32,
        exclude_requester: bool,
    ) -> Result<Self, FormsError> {
        if let Approvers::Analysts(list) = &approvers {
            if list.is_empty() || list.len() > MAX_APPROVERS {
                return Err(FormsError::invalid("approvers", "name 1 to 50 Analysts"));
            }
            let unique: BTreeSet<_> = list.iter().collect();
            if unique.len() != list.len() {
                return Err(FormsError::invalid("approvers", "an Analyst is listed twice"));
            }
        }
        for (field, n) in [
            ("required_approvals", required_approvals),
            ("required_rejections", required_rejections),
        ] {
            if n == 0 || n as usize > MAX_APPROVERS {
                return Err(FormsError::invalid(field, "must be between 1 and 50"));
            }
        }
        Ok(Self {
            approvers,
            required_approvals,
            required_rejections,
            exclude_requester,
        })
    }

    /// Who may approve.
    #[must_use]
    pub const fn approvers(&self) -> &Approvers {
        &self.approvers
    }

    /// Approvals needed in a round.
    #[must_use]
    pub const fn required_approvals(&self) -> u32 {
        self.required_approvals
    }

    /// Rejections that return a Submission in a round.
    #[must_use]
    pub const fn required_rejections(&self) -> u32 {
        self.required_rejections
    }

    /// Whether an Analyst with the Requester's email address may not approve.
    #[must_use]
    pub const fn exclude_requester(&self) -> bool {
        self.exclude_requester
    }
}

/// Opens a ticket from a submitted Form (FR-FRM-003).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketAction {
    category_id: Uuid,
    subject: String,
    description: String,
}

impl TicketAction {
    /// Validated action. The subject and description are templates: `{field_key}` is
    /// replaced by the answer.
    ///
    /// # Errors
    ///
    /// An empty subject or a template that is too long or names an unknown field.
    pub fn new(category_id: Uuid, subject: &str, description: &str) -> Result<Self, FormsError> {
        let subject = subject.trim().to_owned();
        if subject.is_empty() || subject.chars().count() > 500 {
            return Err(FormsError::invalid("ticket.subject", "must be 1 to 500 characters"));
        }
        check_template("ticket.description", description)?;
        Ok(Self {
            category_id,
            subject,
            description: description.trim().to_owned(),
        })
    }

    /// The Portal Catalogue Category of the ticket.
    #[must_use]
    pub const fn category_id(&self) -> Uuid {
        self.category_id
    }

    /// The subject template.
    #[must_use]
    pub fn subject(&self) -> &str {
        &self.subject
    }

    /// The description template.
    #[must_use]
    pub fn description(&self) -> &str {
        &self.description
    }
}

/// Creates a task from a submitted Form (FR-FRM-003).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskAction {
    title: String,
    description: String,
}

impl TaskAction {
    /// Validated action (templates as for [`TicketAction`]).
    ///
    /// # Errors
    ///
    /// An empty or too long title, or a bad template.
    pub fn new(title: &str, description: &str) -> Result<Self, FormsError> {
        let title = title.trim().to_owned();
        if title.is_empty() || title.chars().count() > 255 {
            return Err(FormsError::invalid("task.title", "must be 1 to 255 characters"));
        }
        check_template("task.description", description)?;
        Ok(Self {
            title,
            description: description.trim().to_owned(),
        })
    }

    /// The title template.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    /// The description template.
    #[must_use]
    pub fn description(&self) -> &str {
        &self.description
    }
}

/// The Submission Actions of a Form.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SubmissionActions {
    ticket: Option<TicketAction>,
    task: Option<TaskAction>,
}

impl SubmissionActions {
    /// Actions, each optional.
    #[must_use]
    pub const fn new(ticket: Option<TicketAction>, task: Option<TaskAction>) -> Self {
        Self { ticket, task }
    }

    /// The ticket to open, if any.
    #[must_use]
    pub const fn ticket(&self) -> Option<&TicketAction> {
        self.ticket.as_ref()
    }

    /// The task to create, if any.
    #[must_use]
    pub const fn task(&self) -> Option<&TaskAction> {
        self.task.as_ref()
    }
}

/// The field keys a template refers to, as `{key}` placeholders.
///
/// # Errors
///
/// Unbalanced braces, an empty placeholder or one that is not a valid key.
pub fn placeholders(template: &str) -> Result<Vec<String>, FormsError> {
    let mut keys = Vec::new();
    let mut rest = template;
    while let Some(open) = rest.find(['{', '}']) {
        let bytes = rest.as_bytes();
        if bytes[open] == b'}' {
            return Err(FormsError::invalid("template", "has a closing brace without an opening one"));
        }
        let Some(len) = rest[open..].find('}') else {
            return Err(FormsError::invalid("template", "has an unclosed placeholder"));
        };
        let key = &rest[open + 1..open + len];
        if !valid_key(key) {
            return Err(FormsError::invalid(
                "template",
                format!("{key:?} is not a field key"),
            ));
        }
        keys.push(key.to_owned());
        rest = &rest[open + len + 1..];
    }
    Ok(keys)
}

fn check_template(field: &'static str, template: &str) -> Result<(), FormsError> {
    if template.chars().count() > MAX_TEMPLATE_LEN {
        return Err(FormsError::invalid(field, "is longer than 10000 characters"));
    }
    placeholders(template).map(|_| ()).map_err(|e| match e {
        FormsError::Invalid { reason, .. } => FormsError::invalid(field, reason),
        other => other,
    })
}

/// Replaces each `{key}` with the answer for that key (blank when unanswered).
#[must_use]
pub fn render(template: &str, answers: &Answers) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let Some(len) = rest[open..].find('}') else {
            out.push_str(&rest[open..]);
            return out;
        };
        let key = &rest[open + 1..open + len];
        out.push_str(answers.get(key).map_or("", String::as_str));
        rest = &rest[open + len + 1..];
    }
    out.push_str(rest);
    out
}

/// The facts about a Requester that Forms needs to decide visibility.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequesterProfile {
    /// Company.
    pub tenant_id: TenantId,
    /// Whether the Requester is active.
    pub is_active: bool,
    /// Sign-in e-mail address.
    pub email: String,
    /// Department.
    pub department_id: Option<DepartmentId>,
    /// Active User Groups.
    pub groups: BTreeSet<Uuid>,
}

// ----- Forms ----------------------------------------------------------------------------

/// Stored state of a Form, for restoring it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormRecord {
    /// ID.
    pub id: FormId,
    /// Company.
    pub tenant_id: TenantId,
    /// Name and description.
    pub details: FormDetails,
    /// Whether the Form is active.
    pub is_active: bool,
    /// Earlier versions, oldest first.
    pub previous: Vec<FormVersion>,
    /// The current version.
    pub current: FormVersion,
    /// Portal settings.
    pub portal: PortalSettings,
    /// Approval rule, if any.
    pub approval: Option<ApprovalRule>,
    /// Submission Actions.
    pub actions: SubmissionActions,
    /// Who created the Form.
    pub created_by: Actor,
    /// When it was created.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

/// A Form: its details, its versions (never changed once published) and its settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Form {
    id: FormId,
    tenant_id: TenantId,
    details: FormDetails,
    is_active: bool,
    previous: Vec<FormVersion>,
    current: FormVersion,
    portal: PortalSettings,
    approval: Option<ApprovalRule>,
    actions: SubmissionActions,
    created_by: Actor,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl Form {
    /// Creates a Form with its first version. It is not on the Portal until configured.
    ///
    /// # Errors
    ///
    /// Invalid fields.
    pub fn create(
        tenant_id: TenantId,
        details: FormDetails,
        fields: Vec<FormFieldInput>,
        actor: Actor,
        now: DateTime<Utc>,
    ) -> Result<Self, FormsError> {
        let current = FormVersion::new(1, fields, actor, now)?;
        Ok(Self {
            id: FormId::new(),
            tenant_id,
            details,
            is_active: true,
            previous: Vec::new(),
            current,
            portal: PortalSettings::default(),
            approval: None,
            actions: SubmissionActions::default(),
            created_by: actor,
            created_at: now,
            updated_at: now,
        })
    }

    /// Restores a stored Form.
    #[must_use]
    pub fn restore(r: FormRecord) -> Self {
        Self {
            id: r.id,
            tenant_id: r.tenant_id,
            details: r.details,
            is_active: r.is_active,
            previous: r.previous,
            current: r.current,
            portal: r.portal,
            approval: r.approval,
            actions: r.actions,
            created_by: r.created_by,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }

    /// Changes the name, description and active flag. Does not publish a version.
    pub fn edit(&mut self, details: FormDetails, is_active: bool, now: DateTime<Utc>) {
        self.details = details;
        self.is_active = is_active;
        self.updated_at = now;
    }

    /// Publishes a new version with these fields and makes it the current one. Earlier
    /// versions stay as they were.
    ///
    /// # Errors
    ///
    /// Invalid fields; nothing changes then. Also when the Submission Actions name a
    /// placeholder that the new version does not have.
    pub fn publish_version(
        &mut self,
        fields: Vec<FormFieldInput>,
        actor: Actor,
        now: DateTime<Utc>,
    ) -> Result<u32, FormsError> {
        let number = self.current.number + 1;
        let version = FormVersion::new(number, fields, actor, now)?;
        self.check_placeholders(&version)?;
        let previous = std::mem::replace(&mut self.current, version);
        self.previous.push(previous);
        self.updated_at = now;
        Ok(number)
    }

    /// Replaces the Portal settings, approval rule and Submission Actions. Does not publish
    /// a version.
    ///
    /// # Errors
    ///
    /// A template names a field the current version does not have.
    pub fn configure(
        &mut self,
        portal: PortalSettings,
        approval: Option<ApprovalRule>,
        actions: SubmissionActions,
        now: DateTime<Utc>,
    ) -> Result<(), FormsError> {
        self.check_placeholders(&self.current)?;
        let candidate = Self {
            portal: portal.clone(),
            approval: approval.clone(),
            actions: actions.clone(),
            ..self.clone()
        };
        candidate.check_placeholders(&candidate.current)?;
        self.portal = portal;
        self.approval = approval;
        self.actions = actions;
        self.updated_at = now;
        Ok(())
    }

    fn check_placeholders(&self, version: &FormVersion) -> Result<(), FormsError> {
        let mut templates = Vec::new();
        if let Some(t) = &self.actions.ticket {
            templates.push(t.subject());
            templates.push(t.description());
        }
        if let Some(t) = &self.actions.task {
            templates.push(t.title());
            templates.push(t.description());
        }
        for template in templates {
            for key in placeholders(template)? {
                if !version.fields.iter().any(|f| f.key == key) {
                    return Err(FormsError::invalid(
                        "actions",
                        format!("{{{key}}} is not a field of version {}", version.number),
                    ));
                }
            }
        }
        Ok(())
    }

    /// Whether a Requester may see this Form on the Portal (FR-FRM-002): the Form is
    /// active and visible, the Requester is active in the same Company, and the Requester
    /// is in one of the audiences (or the Form has none).
    #[must_use]
    pub fn offered_to(&self, requester: RequesterId, profile: &RequesterProfile) -> bool {
        self.is_active
            && self.portal.visible
            && profile.is_active
            && profile.tenant_id == self.tenant_id
            && (self.portal.audiences.is_empty()
                || self.portal.audiences.iter().any(|a| match a.kind {
                    AudienceKind::Requester => a.id == requester.as_uuid(),
                    AudienceKind::Group => profile.groups.contains(&a.id),
                    AudienceKind::Department => profile.department_id.map(|d| d.as_uuid()) == Some(a.id),
                }))
    }

    /// ID.
    #[must_use]
    pub const fn id(&self) -> FormId {
        self.id
    }

    /// Company.
    #[must_use]
    pub const fn tenant_id(&self) -> TenantId {
        self.tenant_id
    }

    /// Name and description.
    #[must_use]
    pub const fn details(&self) -> &FormDetails {
        &self.details
    }

    /// Whether the Form is active (inactive Forms are not offered).
    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.is_active
    }

    /// The current version.
    #[must_use]
    pub const fn current(&self) -> &FormVersion {
        &self.current
    }

    /// All versions, oldest first.
    pub fn versions(&self) -> impl Iterator<Item = &FormVersion> {
        self.previous.iter().chain(std::iter::once(&self.current))
    }

    /// Portal settings.
    #[must_use]
    pub const fn portal(&self) -> &PortalSettings {
        &self.portal
    }

    /// The approval rule, if any.
    #[must_use]
    pub const fn approval(&self) -> Option<&ApprovalRule> {
        self.approval.as_ref()
    }

    /// Submission Actions.
    #[must_use]
    pub const fn actions(&self) -> &SubmissionActions {
        &self.actions
    }

    /// Who created the Form.
    #[must_use]
    pub const fn created_by(&self) -> Actor {
        self.created_by
    }

    /// When it was created.
    #[must_use]
    pub const fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }

    /// When it last changed.
    #[must_use]
    pub const fn updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }

    /// The audited values of the Form.
    #[must_use]
    pub fn audit_fields(&self) -> Vec<(&'static str, Value)> {
        vec![
            ("name", json!(self.details.name)),
            ("description", json!(self.details.description)),
            ("is_active", json!(self.is_active)),
            ("current_version", json!(self.current.number)),
            (
                "fields",
                Value::Array(self.current.fields.iter().map(FormField::audit_value).collect()),
            ),
            ("portal_visible", json!(self.portal.visible)),
            (
                "portal_audiences",
                Value::Array(
                    self.portal
                        .audiences
                        .iter()
                        .map(|a| json!({ "kind": a.kind.as_str(), "id": a.id }))
                        .collect(),
                ),
            ),
            (
                "approval",
                self.approval.as_ref().map_or(Value::Null, |r| {
                    json!({
                        "approvers": match &r.approvers {
                            Approvers::Analysts(list) => json!(list.iter().map(|a| a.as_uuid()).collect::<Vec<_>>()),
                            Approvers::Team(team) => json!({ "team": team.as_uuid() }),
                        },
                        "required_approvals": r.required_approvals,
                        "required_rejections": r.required_rejections,
                        "exclude_requester": r.exclude_requester,
                    })
                }),
            ),
            (
                "actions",
                json!({
                    "ticket": self.actions.ticket.as_ref().map(|t| json!({
                        "category_id": t.category_id, "subject": t.subject, "description": t.description })),
                    "task": self.actions.task.as_ref().map(|t| json!({
                        "title": t.title, "description": t.description })),
                }),
            ),
        ]
    }
}

// ----- Submissions and drafts -----------------------------------------------------------

/// Where a Submission is (fixed set).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SubmissionStatus {
    /// Waiting for approvals in the current round.
    PendingApproval,
    /// Rejected in the current round: the Requester may amend it and resubmit.
    Returned,
    /// Approved (or no approval needed); its actions are not yet recorded.
    Approved,
    /// Approved and its actions have run.
    Completed,
}

impl SubmissionStatus {
    /// The stored name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PendingApproval => "pending_approval",
            Self::Returned => "returned",
            Self::Approved => "approved",
            Self::Completed => "completed",
        }
    }
}

impl FromStr for SubmissionStatus {
    type Err = FormsError;

    fn from_str(s: &str) -> Result<Self, FormsError> {
        match s {
            "pending_approval" => Ok(Self::PendingApproval),
            "returned" => Ok(Self::Returned),
            "approved" => Ok(Self::Approved),
            "completed" => Ok(Self::Completed),
            other => Err(FormsError::invalid(
                "status",
                format!("{other:?} is not a submission status"),
            )),
        }
    }
}

/// An approver's decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Decision {
    /// Approve.
    Approve,
    /// Reject (a comment is required).
    Reject,
}

impl Decision {
    /// The stored name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Approve => "approve",
            Self::Reject => "reject",
        }
    }
}

impl FromStr for Decision {
    type Err = FormsError;

    fn from_str(s: &str) -> Result<Self, FormsError> {
        match s {
            "approve" => Ok(Self::Approve),
            "reject" => Ok(Self::Reject),
            other => Err(FormsError::invalid(
                "decision",
                format!("{other:?} is not approve or reject"),
            )),
        }
    }
}

/// One Analyst's vote in one approval round. Votes are never changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Vote {
    /// Approval round.
    pub round: u32,
    /// The approver.
    pub approver: AnalystId,
    /// The decision.
    pub decision: Decision,
    /// Comment (required to reject).
    pub comment: String,
    /// When it was cast.
    pub cast_at: DateTime<Utc>,
}

/// What a vote did to the Submission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoteOutcome {
    /// Still waiting for more votes.
    Pending,
    /// Enough approvals: the Submission is approved.
    Approved,
    /// Enough rejections: the Submission goes back to the Requester.
    Returned,
}

/// Stored state of a Submission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmissionRecord {
    /// ID.
    pub id: SubmissionId,
    /// Company.
    pub tenant_id: TenantId,
    /// The Form.
    pub form_id: FormId,
    /// The version the answers were given against.
    pub form_version: u32,
    /// The Requester who submitted.
    pub requester_id: RequesterId,
    /// Answers.
    pub answers: Answers,
    /// Status.
    pub status: SubmissionStatus,
    /// Current approval round, from 1.
    pub round: u32,
    /// All votes, in the order cast.
    pub votes: Vec<Vote>,
    /// The ticket opened by the Submission Actions.
    pub ticket_id: Option<Uuid>,
    /// The task created by the Submission Actions.
    pub task_id: Option<Uuid>,
    /// When it was submitted.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

/// A Submission: one completed Form, with its approval and the records it created.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Submission {
    rec: SubmissionRecord,
}

impl Submission {
    /// Submits a Form. A Form with an approval rule waits for approvals; without one, the
    /// Submission is approved at once and its actions can run.
    ///
    /// # Errors
    ///
    /// Answers that do not fit the current version, or a missing required answer.
    pub fn submit(
        form: &Form,
        requester: RequesterId,
        answers: &Answers,
        now: DateTime<Utc>,
    ) -> Result<Self, FormsError> {
        let answers = form.current.check_answers(answers, true)?;
        Ok(Self {
            rec: SubmissionRecord {
                id: SubmissionId::new(),
                tenant_id: form.tenant_id,
                form_id: form.id,
                form_version: form.current.number,
                requester_id: requester,
                answers,
                status: if form.approval.is_some() {
                    SubmissionStatus::PendingApproval
                } else {
                    SubmissionStatus::Approved
                },
                round: 1,
                votes: Vec::new(),
                ticket_id: None,
                task_id: None,
                created_at: now,
                updated_at: now,
            },
        })
    }

    /// Restores a stored Submission.
    #[must_use]
    pub const fn restore(rec: SubmissionRecord) -> Self {
        Self { rec }
    }

    /// The stored state.
    #[must_use]
    pub const fn record(&self) -> &SubmissionRecord {
        &self.rec
    }

    /// Amends a returned Submission and starts a new approval round against the current
    /// version of the Form (FR-FRM-004).
    ///
    /// # Errors
    ///
    /// Not returned, or answers that do not fit the current version.
    pub fn resubmit(
        &mut self,
        form: &Form,
        answers: &Answers,
        now: DateTime<Utc>,
    ) -> Result<(), FormsError> {
        if self.rec.status != SubmissionStatus::Returned {
            return Err(FormsError::InvalidState(
                "only a returned Submission can be resubmitted".into(),
            ));
        }
        self.rec.answers = form.current.check_answers(answers, true)?;
        self.rec.form_version = form.current.number;
        self.rec.round += 1;
        self.rec.status = SubmissionStatus::PendingApproval;
        self.rec.updated_at = now;
        Ok(())
    }

    /// Casts a vote in the current round. The caller has checked that the approver may
    /// vote; this checks the round and the rule.
    ///
    /// # Errors
    ///
    /// Not waiting for approval, a second vote by the same approver in the round, or a
    /// rejection without a comment.
    pub fn cast_vote(
        &mut self,
        rule: &ApprovalRule,
        approver: AnalystId,
        decision: Decision,
        comment: &str,
        now: DateTime<Utc>,
    ) -> Result<VoteOutcome, FormsError> {
        if self.rec.status != SubmissionStatus::PendingApproval {
            return Err(FormsError::InvalidState(
                "this Submission is not waiting for approval".into(),
            ));
        }
        let round = self.rec.round;
        if self
            .rec
            .votes
            .iter()
            .any(|v| v.round == round && v.approver == approver)
        {
            return Err(FormsError::InvalidState(
                "this Analyst has already voted in this round".into(),
            ));
        }
        let comment = comment.trim().to_owned();
        if comment.chars().count() > MAX_COMMENT_LEN {
            return Err(FormsError::invalid("comment", "must be at most 2000 characters"));
        }
        if decision == Decision::Reject && comment.is_empty() {
            return Err(FormsError::invalid("comment", "is required to reject"));
        }
        self.rec.votes.push(Vote {
            round,
            approver,
            decision,
            comment,
            cast_at: now,
        });
        self.rec.updated_at = now;
        let (approvals, rejections) = self
            .rec
            .votes
            .iter()
            .filter(|v| v.round == round)
            .fold((0u32, 0u32), |(a, r), v| match v.decision {
                Decision::Approve => (a + 1, r),
                Decision::Reject => (a, r + 1),
            });
        if approvals >= rule.required_approvals {
            self.rec.status = SubmissionStatus::Approved;
            Ok(VoteOutcome::Approved)
        } else if rejections >= rule.required_rejections {
            self.rec.status = SubmissionStatus::Returned;
            Ok(VoteOutcome::Returned)
        } else {
            Ok(VoteOutcome::Pending)
        }
    }

    /// Records that the Submission Actions ran.
    ///
    /// # Errors
    ///
    /// The Submission is not approved.
    pub fn complete(
        &mut self,
        ticket_id: Option<Uuid>,
        task_id: Option<Uuid>,
        now: DateTime<Utc>,
    ) -> Result<(), FormsError> {
        if self.rec.status != SubmissionStatus::Approved {
            return Err(FormsError::InvalidState(
                "only an approved Submission can complete".into(),
            ));
        }
        self.rec.ticket_id = ticket_id;
        self.rec.task_id = task_id;
        self.rec.status = SubmissionStatus::Completed;
        self.rec.updated_at = now;
        Ok(())
    }

    /// The audited values of the Submission.
    #[must_use]
    pub fn audit_fields(&self) -> Vec<(&'static str, Value)> {
        vec![
            ("status", json!(self.rec.status.as_str())),
            ("round", json!(self.rec.round)),
            ("form_version", json!(self.rec.form_version)),
            ("ticket_id", json!(self.rec.ticket_id)),
            ("task_id", json!(self.rec.task_id)),
        ]
    }
}

/// A Requester's saved, unsubmitted answers to a Form (FR-FRM-005). One per Requester and
/// Form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draft {
    /// Company.
    pub tenant_id: TenantId,
    /// The Form.
    pub form_id: FormId,
    /// The Requester.
    pub requester_id: RequesterId,
    /// Answers so far (checked against the fields, but not required).
    pub answers: Answers,
    /// When it was saved.
    pub updated_at: DateTime<Utc>,
}

impl Draft {
    /// Saves answers; a blank answer is dropped and an unknown key is refused.
    ///
    /// # Errors
    ///
    /// Answers that do not fit the current version.
    pub fn save(
        form: &Form,
        requester: RequesterId,
        answers: &Answers,
        now: DateTime<Utc>,
    ) -> Result<Self, FormsError> {
        Ok(Self {
            tenant_id: form.tenant_id,
            form_id: form.id,
            requester_id: requester,
            answers: form.current.check_answers(answers, false)?,
            updated_at: now,
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)] // test code

    use super::*;
    use proptest::prelude::*;

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-11-29T09:00:00Z")
            .unwrap()
            .to_utc()
    }

    fn actor() -> Actor {
        Actor::Analyst(AnalystId::new())
    }

    fn text(key: &str) -> FormFieldInput {
        FormFieldInput {
            key: key.into(),
            label: "Label".into(),
            kind: FieldKind::Text,
            required: false,
            options: Vec::new(),
            width: FieldWidth::Full,
        }
    }

    fn choice(options: &[&str]) -> FormFieldInput {
        FormFieldInput {
            kind: FieldKind::Choice,
            options: options.iter().map(|o| (*o).to_owned()).collect(),
            ..text("pick")
        }
    }

    fn form(fields: Vec<FormFieldInput>) -> Form {
        Form::create(
            TenantId::new(),
            FormDetails::new("Leave", "").unwrap(),
            fields,
            actor(),
            now(),
        )
        .unwrap()
    }

    fn answers(pairs: &[(&str, &str)]) -> Answers {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn form_needs_a_name() {
        assert!(FormDetails::new("   ", "").is_err());
        assert!(FormDetails::new("x".repeat(151), "").is_err());
        assert_eq!(FormDetails::new("  Leave request ", "").unwrap().name(), "Leave request");
    }

    #[test]
    fn key_must_start_with_a_letter_and_use_lower_case_only() {
        assert!(FormField::new(text("start_date")).is_ok());
        assert!(FormField::new(text("1st")).is_err());
        assert!(FormField::new(text("Start")).is_err());
        assert!(FormField::new(text("with-dash")).is_err());
        assert!(FormField::new(text("")).is_err());
        assert!(FormField::new(text(&"a".repeat(51))).is_err());
    }

    #[test]
    fn choice_fields_need_unique_options_and_other_fields_have_none() {
        assert!(FormField::new(choice(&["Yes", "No"])).is_ok());
        assert!(FormField::new(choice(&[])).is_err());
        assert!(FormField::new(choice(&["Yes", " yes "])).is_err());
        assert!(FormField::new(choice(&["", "No"])).is_err());
        let mut text_with_options = text("note");
        text_with_options.options = vec!["A".into()];
        assert!(FormField::new(text_with_options).is_err());
    }

    #[test]
    fn version_needs_fields_with_distinct_keys() {
        assert!(FormVersion::new(1, vec![], actor(), now()).is_err());
        assert!(FormVersion::new(1, vec![text("a"), text("a")], actor(), now()).is_err());
        let many: Vec<FormFieldInput> = (0..=MAX_FIELDS).map(|i| text(&format!("f{i}"))).collect();
        assert!(FormVersion::new(1, many, actor(), now()).is_err());
        assert!(FormVersion::new(1, vec![text("a"), text("b")], actor(), now()).is_ok());
    }

    #[test]
    fn publishing_a_version_keeps_earlier_versions_unchanged() {
        let mut f = form(vec![text("a")]);
        let first = f.current().clone();
        let number = f.publish_version(vec![text("a"), text("b")], actor(), now()).unwrap();
        assert_eq!(number, 2);
        assert_eq!(f.current().fields().len(), 2);
        let versions: Vec<u32> = f.versions().map(FormVersion::number).collect();
        assert_eq!(versions, [1, 2]);
        assert_eq!(f.versions().next(), Some(&first));
    }

    #[test]
    fn a_rejected_version_changes_nothing() {
        let mut f = form(vec![text("a")]);
        let before = f.clone();
        assert!(f.publish_version(vec![], actor(), now()).is_err());
        assert_eq!(f, before);
    }

    #[test]
    fn edit_changes_details_and_active_flag_without_a_new_version() {
        let mut f = form(vec![text("a")]);
        f.edit(FormDetails::new("Holiday", "Annual").unwrap(), false, now());
        assert_eq!(f.details().name(), "Holiday");
        assert!(!f.is_active());
        assert_eq!(f.versions().count(), 1);
    }

    #[test]
    fn answers_must_fit_their_fields() {
        let mut number = text("n");
        number.kind = FieldKind::Number;
        let mut date = text("d");
        date.kind = FieldKind::Date;
        let mut flag = text("b");
        flag.kind = FieldKind::Boolean;
        let v = FormVersion::new(1, vec![number, date, flag, choice(&["Yes", "No"])], actor(), now()).unwrap();
        assert!(v.check_answers(&answers(&[("n", "3.5"), ("d", "2026-12-01"), ("b", "true"), ("pick", "No")]), true).is_ok());
        assert!(v.check_answers(&answers(&[("n", "three")]), false).is_err());
        assert!(v.check_answers(&answers(&[("d", "01/12/2026")]), false).is_err());
        assert!(v.check_answers(&answers(&[("b", "yes")]), false).is_err());
        assert!(v.check_answers(&answers(&[("pick", "Maybe")]), false).is_err());
        assert!(v.check_answers(&answers(&[("unknown", "x")]), false).is_err());
    }

    #[test]
    fn a_submission_needs_its_required_answers_but_a_draft_does_not() {
        let mut required = text("reason");
        required.required = true;
        let f = form(vec![required, text("note")]);
        assert!(Submission::submit(&f, RequesterId::new(), &answers(&[("note", "x")]), now()).is_err());
        assert!(Submission::submit(&f, RequesterId::new(), &answers(&[("reason", "  ")]), now()).is_err());
        assert!(Draft::save(&f, RequesterId::new(), &answers(&[("note", "x")]), now()).is_ok());
    }

    #[test]
    fn blank_answers_are_dropped() {
        let f = form(vec![text("a")]);
        let s = Submission::submit(&f, RequesterId::new(), &answers(&[("a", "  hi  ")]), now()).unwrap();
        assert_eq!(s.record().answers.get("a").map(String::as_str), Some("hi"));
        let s = Submission::submit(&f, RequesterId::new(), &answers(&[("a", " ")]), now()).unwrap();
        assert!(s.record().answers.is_empty());
    }

    #[test]
    fn placeholders_must_name_real_keys_and_render_answers() {
        assert_eq!(placeholders("Hi {name}, {x_1}").unwrap(), ["name", "x_1"]);
        assert!(placeholders("{Name}").is_err());
        assert!(placeholders("open {name").is_err());
        assert!(placeholders("close name}").is_err());
        let a = answers(&[("name", "Ann")]);
        assert_eq!(render("Hi {name}, {missing}!", &a), "Hi Ann, !");
    }

    #[test]
    fn actions_may_only_use_fields_of_the_current_version() {
        let mut f = form(vec![text("name")]);
        let actions = SubmissionActions::new(
            Some(TicketAction::new(Uuid::now_v7(), "Leave for {name}", "").unwrap()),
            None,
        );
        assert!(f.configure(PortalSettings::default(), None, actions.clone(), now()).is_ok());
        assert!(f.publish_version(vec![text("other")], actor(), now()).is_err());
        let bad = SubmissionActions::new(None, Some(TaskAction::new("T {nope}", "").unwrap()));
        assert!(f.configure(PortalSettings::default(), None, bad, now()).is_err());
    }

    #[test]
    fn a_requester_sees_a_visible_form_only_inside_its_audience() {
        let company = TenantId::new();
        let mut f = Form::create(company, FormDetails::new("Leave", "").unwrap(), vec![text("a")], actor(), now()).unwrap();
        let me = RequesterId::new();
        let group = Uuid::now_v7();
        let profile = |tenant: TenantId, groups: &[Uuid]| RequesterProfile {
            tenant_id: tenant,
            is_active: true,
            email: "r@example.test".into(),
            department_id: None,
            groups: groups.iter().copied().collect(),
        };
        assert!(!f.offered_to(me, &profile(company, &[])), "hidden until visible");
        f.configure(PortalSettings::new(true, vec![]).unwrap(), None, SubmissionActions::default(), now()).unwrap();
        assert!(f.offered_to(me, &profile(company, &[])), "visible to all of the Company");
        assert!(!f.offered_to(me, &profile(TenantId::new(), &[])), "never another Company");
        let grouped = PortalSettings::new(true, vec![Audience { kind: AudienceKind::Group, id: group }]).unwrap();
        f.configure(grouped, None, SubmissionActions::default(), now()).unwrap();
        assert!(!f.offered_to(me, &profile(company, &[])));
        assert!(f.offered_to(me, &profile(company, &[group])));
    }

    #[test]
    fn approval_by_enough_approvers_approves_and_rejection_returns_for_a_new_round() {
        let mut f = form(vec![text("a")]);
        let rule = ApprovalRule::new(Approvers::Analysts(vec![AnalystId::new()]), 2, 1, false).unwrap();
        f.configure(PortalSettings::default(), Some(rule.clone()), SubmissionActions::default(), now()).unwrap();
        let mut s = Submission::submit(&f, RequesterId::new(), &answers(&[("a", "x")]), now()).unwrap();
        assert_eq!(s.record().status, SubmissionStatus::PendingApproval);
        let (p1, p2) = (AnalystId::new(), AnalystId::new());
        assert_eq!(s.cast_vote(&rule, p1, Decision::Approve, "", now()).unwrap(), VoteOutcome::Pending);
        assert!(s.cast_vote(&rule, p1, Decision::Approve, "", now()).is_err(), "one vote per approver per round");
        assert!(s.cast_vote(&rule, p2, Decision::Reject, "", now()).is_err(), "a rejection needs a comment");
        assert_eq!(s.cast_vote(&rule, p2, Decision::Reject, "Needs manager", now()).unwrap(), VoteOutcome::Returned);
        assert_eq!(s.record().status, SubmissionStatus::Returned);
        s.resubmit(&f, &answers(&[("a", "better")]), now()).unwrap();
        assert_eq!(s.record().round, 2);
        assert_eq!(s.record().status, SubmissionStatus::PendingApproval);
        assert_eq!(s.cast_vote(&rule, p1, Decision::Approve, "", now()).unwrap(), VoteOutcome::Pending, "votes restart each round");
        assert_eq!(s.cast_vote(&rule, p2, Decision::Approve, "", now()).unwrap(), VoteOutcome::Approved);
        s.complete(Some(Uuid::now_v7()), None, now()).unwrap();
        assert_eq!(s.record().status, SubmissionStatus::Completed);
        assert_eq!(s.record().votes.len(), 4, "votes are kept, across rounds");
    }

    #[test]
    fn a_submission_without_approval_is_approved_at_once() {
        let f = form(vec![text("a")]);
        let mut s = Submission::submit(&f, RequesterId::new(), &answers(&[]), now()).unwrap();
        assert_eq!(s.record().status, SubmissionStatus::Approved);
        s.complete(None, None, now()).unwrap();
        assert!(s.complete(None, None, now()).is_err(), "completes once");
    }

    proptest! {
        #[test]
        fn any_lower_case_identifier_is_a_valid_key(key in "[a-z][a-z0-9_]{0,49}") {
            prop_assert!(FormField::new(text(&key)).is_ok());
        }

        #[test]
        fn keys_starting_with_a_digit_are_rejected(key in "[0-9][a-z0-9_]{0,20}") {
            prop_assert!(FormField::new(text(&key)).is_err());
        }

        #[test]
        fn rendering_without_placeholders_changes_nothing(text in "[a-zA-Z0-9 .,!?]{0,80}") {
            prop_assert_eq!(render(&text, &Answers::new()), text);
        }
    }
}
