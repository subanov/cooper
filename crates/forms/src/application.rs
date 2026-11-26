//! Forms use cases (ADR-0039, ADR-0040). Staff build and decide; Requesters use the Portal.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use cooper_kernel::{
    Actor, AnalystId, AuditAction, AuditRecord, Clock, Forbidden, Limit, Principal, RequesterId,
    TenantId, TenantScope,
};
use serde_json::Value;
use thiserror::Error;
use uuid::Uuid;

use crate::domain::{
    Answers, Approvers, ApprovalRule, Decision, Draft, Form, FormDetails, FormFieldInput, FormId,
    FormsError, PortalSettings, RequesterProfile, Submission, SubmissionActions, SubmissionId,
    SubmissionStatus, VoteOutcome, render,
};
use crate::ports::{
    Directory, FormQuery, FormsRepository, RepoError, SubmissionQuery, SubmissionTargets,
};

/// Capabilities Forms checks (FR-IAM-010).
pub mod capabilities {
    /// See Forms, their versions and Submissions.
    pub const READ: &str = "forms.read";
    /// Build and configure Forms and publish their versions.
    pub const WRITE: &str = "forms.write";
    /// Vote on Submissions that need approval, and retry their actions.
    pub const APPROVE: &str = "forms.approve";

    /// Every Capability declared by Forms.
    pub const ALL: &[&str] = &[READ, WRITE, APPROVE];
}

/// A use case failed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AppError {
    /// A business rule was violated.
    #[error(transparent)]
    Rule(#[from] FormsError),
    /// The caller lacks a Capability, or is not the kind of user the action needs.
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
            RepoError::Conflict(c) if c == "forms_name_key" => Self::Conflict(
                "a Form with this name already exists in the Company".into(),
            ),
            RepoError::Conflict(_) => {
                Self::Conflict("the record conflicts with an existing one".into())
            }
            RepoError::MissingReference(c) => Self::Rule(FormsError::invalid(
                "reference",
                format!("refers to a record that does not exist ({c})"),
            )),
            RepoError::NotFound => Self::NotFound("record"),
            RepoError::Storage(message) => {
                tracing::error!(error = %message, "forms storage error");
                Self::Internal
            }
        }
    }
}

/// Dependencies.
#[derive(Clone)]
pub struct FormsPorts {
    /// Storage.
    pub forms: Arc<dyn FormsRepository>,
    /// People and Teams.
    pub directory: Arc<dyn Directory>,
    /// Tickets and tasks.
    pub targets: Arc<dyn SubmissionTargets>,
    /// Time.
    pub clock: Arc<dyn Clock>,
}

/// Forms use cases.
#[derive(Clone)]
pub struct FormsService {
    ports: FormsPorts,
}

fn require(principal: &Principal, capability: &str) -> Result<(), AppError> {
    if matches!(principal.actor(), Actor::Requester(_)) {
        return Err(Forbidden("staff".into()).into());
    }
    Ok(principal.require(capability)?)
}

/// The Requester behind a Portal principal.
fn requester_of(principal: &Principal) -> Result<RequesterId, AppError> {
    match principal.actor() {
        Actor::Requester(r) => Ok(r),
        _ => Err(Forbidden("requester".into()).into()),
    }
}

/// An audit record of `after` against `before` for one aggregate.
fn audit(
    tenant: TenantId,
    aggregate: (&'static str, Uuid),
    action: AuditAction,
    before: &[(&'static str, Value)],
    after: Vec<(&'static str, Value)>,
    actor: Actor,
    now: DateTime<Utc>,
) -> AuditRecord {
    after.into_iter().fold(
        AuditRecord::new(Some(tenant), aggregate.0, aggregate.1, action, actor, now),
        |r, (field, value)| {
            r.change(
                field,
                before.iter().find(|(n, _)| *n == field).map(|(_, v)| v),
                Some(&value),
            )
        },
    )
}

const FORM: &str = "form";
const SUBMISSION: &str = "form_submission";

impl FormsService {
    /// Creates the service.
    #[must_use]
    pub const fn new(ports: FormsPorts) -> Self {
        Self { ports }
    }

    async fn find(&self, principal: &Principal, id: FormId) -> Result<Form, AppError> {
        self.ports
            .forms
            .form(principal.scope(), id)
            .await?
            .ok_or(AppError::NotFound("Form"))
    }

    async fn find_submission(
        &self,
        scope: &TenantScope,
        id: SubmissionId,
    ) -> Result<Submission, AppError> {
        self.ports
            .forms
            .submission(scope, id)
            .await?
            .ok_or(AppError::NotFound("Submission"))
    }

    // --- Staff: Forms ---------------------------------------------------------------------

    /// Forms in the caller's Companies (needs `forms.read`).
    ///
    /// # Errors
    ///
    /// Forbidden or storage failure.
    pub async fn list_forms(&self, principal: &Principal, query: FormQuery) -> Result<Vec<Form>, AppError> {
        require(principal, capabilities::READ)?;
        Ok(self.ports.forms.forms(principal.scope(), &query).await?)
    }

    /// One Form with its versions (needs `forms.read`).
    ///
    /// # Errors
    ///
    /// Forbidden, or not found (also when the Form is in another Company).
    pub async fn get_form(&self, principal: &Principal, id: FormId) -> Result<Form, AppError> {
        require(principal, capabilities::READ)?;
        self.find(principal, id).await
    }

    /// Creates a Form with its first version in a Company the caller can see (needs
    /// `forms.write`).
    ///
    /// # Errors
    ///
    /// Forbidden, an invisible Company, invalid input or a duplicate name.
    pub async fn create_form(
        &self,
        principal: &Principal,
        tenant: TenantId,
        details: FormDetails,
        fields: Vec<FormFieldInput>,
    ) -> Result<Form, AppError> {
        require(principal, capabilities::WRITE)?;
        if !principal.scope().allows(tenant) {
            return Err(AppError::NotFound("Company"));
        }
        let now = self.ports.clock.now();
        let form = Form::create(tenant, details, fields, principal.actor(), now)?;
        let record = audit(
            tenant,
            (FORM, form.id().as_uuid()),
            AuditAction::Created,
            &[],
            form.audit_fields(),
            principal.actor(),
            now,
        );
        self.ports.forms.save_form(&form, &record).await?;
        Ok(form)
    }

    /// Changes a Form's name, description and active flag (needs `forms.write`). Does not
    /// publish a version.
    ///
    /// # Errors
    ///
    /// Forbidden, not found, invalid input or a duplicate name.
    pub async fn edit_form(
        &self,
        principal: &Principal,
        id: FormId,
        details: FormDetails,
        is_active: bool,
    ) -> Result<Form, AppError> {
        require(principal, capabilities::WRITE)?;
        let mut form = self.find(principal, id).await?;
        let before = form.audit_fields();
        let now = self.ports.clock.now();
        form.edit(details, is_active, now);
        self.save_form_change(principal, &mut form, &before, now).await?;
        Ok(form)
    }

    /// Publishes a new version of a Form's fields (needs `forms.write`). Earlier versions
    /// are kept unchanged.
    ///
    /// # Errors
    ///
    /// Forbidden, not found or invalid fields.
    pub async fn publish_version(
        &self,
        principal: &Principal,
        id: FormId,
        fields: Vec<FormFieldInput>,
    ) -> Result<Form, AppError> {
        require(principal, capabilities::WRITE)?;
        let mut form = self.find(principal, id).await?;
        let before = form.audit_fields();
        let now = self.ports.clock.now();
        form.publish_version(fields, principal.actor(), now)?;
        self.save_form_change(principal, &mut form, &before, now).await?;
        Ok(form)
    }

    /// Replaces a Form's Portal settings, approval rule and Submission Actions (needs
    /// `forms.write`). Does not publish a version.
    ///
    /// # Errors
    ///
    /// Forbidden, not found, or settings that do not fit the current version.
    pub async fn configure_form(
        &self,
        principal: &Principal,
        id: FormId,
        portal: PortalSettings,
        approval: Option<ApprovalRule>,
        actions: SubmissionActions,
    ) -> Result<Form, AppError> {
        require(principal, capabilities::WRITE)?;
        let mut form = self.find(principal, id).await?;
        let before = form.audit_fields();
        let now = self.ports.clock.now();
        form.configure(portal, approval, actions, now)?;
        self.save_form_change(principal, &mut form, &before, now).await?;
        Ok(form)
    }

    async fn save_form_change(
        &self,
        principal: &Principal,
        form: &mut Form,
        before: &[(&'static str, Value)],
        now: DateTime<Utc>,
    ) -> Result<(), AppError> {
        let record = audit(
            form.tenant_id(),
            (FORM, form.id().as_uuid()),
            AuditAction::Updated,
            before,
            form.audit_fields(),
            principal.actor(),
            now,
        );
        self.ports.forms.save_form(form, &record).await?;
        Ok(())
    }

    // --- Staff: Submissions ---------------------------------------------------------------

    /// Submissions in the caller's Companies, newest first (needs `forms.read`).
    ///
    /// # Errors
    ///
    /// Forbidden or storage failure.
    pub async fn list_submissions(
        &self,
        principal: &Principal,
        query: SubmissionQuery,
    ) -> Result<Vec<Submission>, AppError> {
        require(principal, capabilities::READ)?;
        Ok(self.ports.forms.submissions(principal.scope(), &query).await?)
    }

    /// One Submission with its votes (needs `forms.read`).
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn get_submission(
        &self,
        principal: &Principal,
        id: SubmissionId,
    ) -> Result<Submission, AppError> {
        require(principal, capabilities::READ)?;
        self.find_submission(principal.scope(), id).await
    }

    /// Votes on a Submission that is waiting for approval (needs `forms.approve`). The
    /// caller must be an active Analyst the approval rule names (directly or through its
    /// Team), and, when the rule excludes the Requester, must not be the Requester's own
    /// e-mail address. An approval that completes the round runs the Submission Actions.
    ///
    /// # Errors
    ///
    /// Forbidden, not found, a vote that is not allowed in the current state, or an action
    /// that failed (the vote is kept; the actions can be retried).
    pub async fn vote(
        &self,
        principal: &Principal,
        id: SubmissionId,
        decision: Decision,
        comment: &str,
    ) -> Result<Submission, AppError> {
        require(principal, capabilities::APPROVE)?;
        let Actor::Analyst(analyst) = principal.actor() else {
            return Err(Forbidden("an Analyst".into()).into());
        };
        let mut s = self.find_submission(principal.scope(), id).await?;
        let form = self
            .ports
            .forms
            .form(&TenantScope::single(s.record().tenant_id), s.record().form_id)
            .await?
            .ok_or(AppError::NotFound("Form"))?;
        let rule = form
            .approval()
            .ok_or_else(|| FormsError::InvalidState("this Form needs no approval".into()))?
            .clone();
        self.check_approver(&rule, analyst, s.record().requester_id, s.record().tenant_id)
            .await?;
        let now = self.ports.clock.now();
        let before = s.audit_fields();
        let outcome = s.cast_vote(&rule, analyst, decision, comment, now)?;
        let record = audit(
            s.record().tenant_id,
            (SUBMISSION, s.record().id.as_uuid()),
            AuditAction::Updated,
            &before,
            s.audit_fields(),
            principal.actor(),
            now,
        );
        self.ports.forms.save_submission(&s, None, &record).await?;
        if outcome == VoteOutcome::Approved {
            self.run_actions(&form, &mut s).await?;
        }
        Ok(s)
    }

    async fn check_approver(
        &self,
        rule: &ApprovalRule,
        analyst: AnalystId,
        requester: RequesterId,
        tenant: TenantId,
    ) -> Result<(), AppError> {
        let profile = self
            .ports
            .directory
            .analyst(analyst)
            .await?
            .filter(|a| a.is_active)
            .ok_or(Forbidden("an active Analyst".into()))?;
        let named = match rule.approvers() {
            Approvers::Analysts(list) => list.contains(&analyst),
            Approvers::Team(team) => self
                .ports
                .directory
                .team_members(*team)
                .await?
                .is_some_and(|m| m.contains(&analyst)),
        };
        if !named {
            return Err(Forbidden("an approver of this Form".into()).into());
        }
        if rule.exclude_requester() {
            let requester_email = self
                .ports
                .directory
                .requester(requester)
                .await?
                .filter(|r| r.tenant_id == tenant)
                .map(|r| r.email);
            if requester_email.is_some_and(|e| e.eq_ignore_ascii_case(&profile.email)) {
                return Err(Forbidden("an approver other than the Requester".into()).into());
            }
        }
        Ok(())
    }

    /// Runs the Submission Actions of an approved Submission again, after a failure
    /// (needs `forms.approve`).
    ///
    /// # Errors
    ///
    /// Forbidden, not found, a Submission that is not approved, or an action that failed.
    pub async fn retry_actions(
        &self,
        principal: &Principal,
        id: SubmissionId,
    ) -> Result<Submission, AppError> {
        require(principal, capabilities::APPROVE)?;
        let mut s = self.find_submission(principal.scope(), id).await?;
        let form = self
            .ports
            .forms
            .form(&TenantScope::single(s.record().tenant_id), s.record().form_id)
            .await?
            .ok_or(AppError::NotFound("Form"))?;
        self.run_actions(&form, &mut s).await?;
        Ok(s)
    }

    /// Opens the ticket and creates the task of an approved Submission, then records them.
    /// Each Submission Action runs once: a Submission that is already completed is refused.
    async fn run_actions(&self, form: &Form, s: &mut Submission) -> Result<(), AppError> {
        if s.record().status != SubmissionStatus::Approved {
            return Err(FormsError::InvalidState("only an approved Submission runs its actions".into()).into());
        }
        let answers: &Answers = &s.record().answers;
        let (tenant, requester) = (s.record().tenant_id, s.record().requester_id);
        let ticket = match form.actions().ticket() {
            Some(t) => Some(
                self.ports
                    .targets
                    .open_ticket(
                        tenant,
                        requester,
                        t.category_id(),
                        &render(t.subject(), answers),
                        &render(t.description(), answers),
                    )
                    .await?,
            ),
            None => None,
        };
        let task = match form.actions().task() {
            Some(t) => Some(
                self.ports
                    .targets
                    .create_task(tenant, &render(t.title(), answers), &render(t.description(), answers))
                    .await?,
            ),
            None => None,
        };
        let now = self.ports.clock.now();
        let before = s.audit_fields();
        s.complete(ticket, task, now)?;
        let record = audit(
            tenant,
            (SUBMISSION, s.record().id.as_uuid()),
            AuditAction::Updated,
            &before,
            s.audit_fields(),
            Actor::Automation,
            now,
        );
        self.ports.forms.save_submission(s, None, &record).await?;
        Ok(())
    }

    // --- Portal: Requesters ---------------------------------------------------------------

    /// The Requester's profile, which must be active.
    async fn profile(&self, principal: &Principal) -> Result<(RequesterId, RequesterProfile), AppError> {
        let requester = requester_of(principal)?;
        let profile = self
            .ports
            .directory
            .requester(requester)
            .await?
            .filter(|p| p.is_active)
            .ok_or(Forbidden("an active Requester".into()))?;
        Ok((requester, profile))
    }

    /// The Forms this Requester may see on the Portal (FR-FRM-002).
    ///
    /// # Errors
    ///
    /// Not a Requester, or storage failure.
    pub async fn portal_forms(&self, principal: &Principal) -> Result<Vec<Form>, AppError> {
        let (requester, profile) = self.profile(principal).await?;
        let scope = TenantScope::single(profile.tenant_id);
        let query = FormQuery {
            tenant_id: Some(profile.tenant_id),
            active: Some(true),
            limit: Limit::new(200).unwrap_or(Limit::DEFAULT),
            ..FormQuery::default()
        };
        Ok(self
            .ports
            .forms
            .forms(&scope, &query)
            .await?
            .into_iter()
            .filter(|f| f.offered_to(requester, &profile))
            .collect())
    }

    async fn portal_form(
        &self,
        requester: RequesterId,
        profile: &RequesterProfile,
        id: FormId,
    ) -> Result<Form, AppError> {
        self.ports
            .forms
            .form(&TenantScope::single(profile.tenant_id), id)
            .await?
            .filter(|f| f.offered_to(requester, profile))
            .ok_or(AppError::NotFound("Form"))
    }

    /// One Form the Requester may see on the Portal.
    ///
    /// # Errors
    ///
    /// Not a Requester, or not found (also when the Requester may not see the Form).
    pub async fn portal_form_for(&self, principal: &Principal, id: FormId) -> Result<Form, AppError> {
        let (requester, profile) = self.profile(principal).await?;
        self.portal_form(requester, &profile, id).await
    }

    /// The Requester's Draft of a Form, if any.
    ///
    /// # Errors
    ///
    /// Not visible to the Requester, or storage failure.
    pub async fn get_draft(&self, principal: &Principal, id: FormId) -> Result<Option<Draft>, AppError> {
        let (requester, profile) = self.profile(principal).await?;
        self.portal_form(requester, &profile, id).await?;
        Ok(self
            .ports
            .forms
            .draft(&TenantScope::single(profile.tenant_id), id, requester)
            .await?)
    }

    /// Saves the Requester's Draft of a Form (FR-FRM-005).
    ///
    /// # Errors
    ///
    /// Not visible to the Requester, or answers that do not fit the Form.
    pub async fn save_draft(
        &self,
        principal: &Principal,
        id: FormId,
        answers: &Answers,
    ) -> Result<Draft, AppError> {
        let (requester, profile) = self.profile(principal).await?;
        let form = self.portal_form(requester, &profile, id).await?;
        let draft = Draft::save(&form, requester, answers, self.ports.clock.now())?;
        self.ports.forms.save_draft(&draft).await?;
        Ok(draft)
    }

    /// Submits a Form (FR-FRM-003). The Draft of that Form is removed. Without an approval
    /// rule the Submission Actions run at once.
    ///
    /// # Errors
    ///
    /// Not visible to the Requester, answers that do not fit or are incomplete, or an
    /// action that failed (the Submission is kept as approved; staff can retry).
    pub async fn submit(
        &self,
        principal: &Principal,
        id: FormId,
        answers: &Answers,
    ) -> Result<Submission, AppError> {
        let (requester, profile) = self.profile(principal).await?;
        let form = self.portal_form(requester, &profile, id).await?;
        let now = self.ports.clock.now();
        let mut s = Submission::submit(&form, requester, answers, now)?;
        let record = audit(
            s.record().tenant_id,
            (SUBMISSION, s.record().id.as_uuid()),
            AuditAction::Created,
            &[],
            s.audit_fields(),
            principal.actor(),
            now,
        );
        self.ports
            .forms
            .save_submission(&s, Some((id, requester)), &record)
            .await?;
        if s.record().status == SubmissionStatus::Approved {
            self.run_actions(&form, &mut s).await?;
        }
        Ok(s)
    }

    /// The Requester's own Submissions, newest first.
    ///
    /// # Errors
    ///
    /// Not a Requester, or storage failure.
    pub async fn my_submissions(
        &self,
        principal: &Principal,
        limit: Limit,
        after: Option<(DateTime<Utc>, Uuid)>,
    ) -> Result<Vec<Submission>, AppError> {
        let (requester, profile) = self.profile(principal).await?;
        Ok(self
            .ports
            .forms
            .submissions(
                &TenantScope::single(profile.tenant_id),
                &SubmissionQuery {
                    tenant_id: Some(profile.tenant_id),
                    requester_id: Some(requester),
                    status: None,
                    form_id: None,
                    limit,
                    after,
                },
            )
            .await?)
    }

    async fn own_submission(
        &self,
        principal: &Principal,
        id: SubmissionId,
    ) -> Result<(Submission, RequesterProfile), AppError> {
        let (requester, profile) = self.profile(principal).await?;
        let s = self
            .find_submission(&TenantScope::single(profile.tenant_id), id)
            .await?;
        if s.record().requester_id != requester {
            return Err(AppError::NotFound("Submission"));
        }
        Ok((s, profile))
    }

    /// One of the Requester's own Submissions.
    ///
    /// # Errors
    ///
    /// Not a Requester, or not found (also when it belongs to someone else).
    pub async fn my_submission(&self, principal: &Principal, id: SubmissionId) -> Result<Submission, AppError> {
        Ok(self.own_submission(principal, id).await?.0)
    }

    /// Amends a returned Submission and starts a new approval round (FR-FRM-004).
    ///
    /// # Errors
    ///
    /// Not the Requester's, not returned, or answers that do not fit the Form.
    pub async fn resubmit(
        &self,
        principal: &Principal,
        id: SubmissionId,
        answers: &Answers,
    ) -> Result<Submission, AppError> {
        let (mut s, profile) = self.own_submission(principal, id).await?;
        let form = self
            .ports
            .forms
            .form(&TenantScope::single(profile.tenant_id), s.record().form_id)
            .await?
            .ok_or(AppError::NotFound("Form"))?;
        let now = self.ports.clock.now();
        let before = s.audit_fields();
        s.resubmit(&form, answers, now)?;
        let record = audit(
            s.record().tenant_id,
            (SUBMISSION, s.record().id.as_uuid()),
            AuditAction::Updated,
            &before,
            s.audit_fields(),
            principal.actor(),
            now,
        );
        self.ports.forms.save_submission(&s, None, &record).await?;
        Ok(s)
    }
}
