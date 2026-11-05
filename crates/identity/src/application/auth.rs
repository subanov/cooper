//! First-run setup, sign-in, request authentication and sign-out (FR-SYS-004,
//! FR-IAM-002).

use std::collections::BTreeSet;

use cooper_kernel::{Actor, AuditAction, Principal};

use super::{AppError, IdentityService};
use crate::domain::{
    Analyst, Company, DomainName, EmailAddress, Name, NewAnalyst, Password, Session, SessionToken,
    TicketCode, Username, audit_record,
};

/// Input for first-run setup.
#[derive(Debug, Clone)]
pub struct FirstRunSetup {
    /// Name of the default Company.
    pub company_name: Name,
    /// Ticket Code of the default Company.
    pub ticket_code: TicketCode,
    /// Sender Domains of the default Company.
    pub sender_domains: BTreeSet<DomainName>,
    /// The first Administrator's sign-in name.
    pub username: Username,
    /// The first Administrator's email.
    pub email: EmailAddress,
    /// The first Administrator's display name.
    pub full_name: Name,
    /// The first Administrator's password.
    pub password: Password,
}

/// Sign-in request.
#[derive(Debug, Clone)]
pub struct SignIn {
    /// Username or email address.
    pub login: String,
    /// Password as typed.
    pub password: String,
    /// Client IP address, for the session record.
    pub ip_address: Option<String>,
    /// Client user agent, for the session record.
    pub user_agent: Option<String>,
}

/// A successful sign-in or authenticated request.
#[derive(Debug, Clone)]
pub struct SignedIn {
    /// The session.
    pub session: Session,
    /// The raw session token for the cookie. Only set right after sign-in.
    pub token: Option<SessionToken>,
    /// The signed-in Analyst.
    pub analyst: Analyst,
    /// Their effective permissions and scope.
    pub principal: Principal,
}

impl IdentityService {
    /// Whether first-run setup still has to be done.
    ///
    /// # Errors
    ///
    /// Storage failures.
    pub async fn setup_required(&self) -> Result<bool, AppError> {
        Ok(self.ports.setup.is_required().await?)
    }

    /// Creates the default Company and the first Administrator (FR-SYS-004). Only
    /// possible while no Analyst exists.
    ///
    /// # Errors
    ///
    /// [`AppError::SetupCompleted`] if setup was already done, or validation errors.
    pub async fn complete_setup(&self, input: FirstRunSetup) -> Result<Analyst, AppError> {
        if !self.setup_required().await? {
            return Err(AppError::SetupCompleted);
        }
        let now = self.ports.clock.now();
        let freemail = self.ports.companies.freemail_domains().await?;
        let company = Company::create(
            input.company_name,
            input.ticket_code,
            input.sender_domains,
            true,
            &freemail,
            now,
        )?;
        let mut admin = Analyst::create(
            NewAnalyst {
                username: input.username,
                email: input.email,
                full_name: input.full_name,
                is_admin: true,
            },
            now,
        );
        admin.set_password(self.ports.hasher.hash(&input.password).await?, now);
        let actor = Actor::Analyst(admin.id());
        let company_audit = audit_record(None, &company, AuditAction::Created, actor, now);
        let admin_audit = audit_record(None, &admin, AuditAction::Created, actor, now);
        match self
            .ports
            .setup
            .complete(&company, &company_audit, &admin, &admin_audit)
            .await
        {
            Err(crate::domain::RepoError::Conflict(c)) if c == "setup" => {
                Err(AppError::SetupCompleted)
            }
            other => other.map_err(AppError::from),
        }?;
        tracing::info!(analyst_id = %admin.id(), "first-run setup completed");
        Ok(admin)
    }

    /// Signs an Analyst in and starts a session.
    ///
    /// # Errors
    ///
    /// [`AppError::InvalidCredentials`] for an unknown login, wrong password or inactive
    /// Analyst, without saying which.
    pub async fn sign_in(&self, request: SignIn) -> Result<SignedIn, AppError> {
        let analyst = self
            .ports
            .analysts
            .find_by_login(request.login.trim())
            .await?;
        let hash = analyst.as_ref().and_then(Analyst::password_hash);
        let password_ok = self.ports.hasher.verify(&request.password, hash).await;
        let analyst = match analyst {
            Some(a) if password_ok && a.is_active() => a,
            _ => {
                tracing::info!(login = %request.login.trim(), "failed sign-in");
                return Err(AppError::InvalidCredentials);
            }
        };
        let now = self.ports.clock.now();
        let token = SessionToken::new(self.ports.tokens.token()?);
        let csrf = self.ports.tokens.token()?;
        let session = Session::start(
            analyst.id(),
            &token,
            csrf,
            self.session_policy,
            now,
            request.ip_address,
            request.user_agent,
        );
        self.ports.sessions.insert(&session).await?;
        let principal = self.principal_for(&analyst).await?;
        tracing::info!(analyst_id = %analyst.id(), session_id = %session.id(), "signed in");
        Ok(SignedIn {
            session,
            token: Some(token),
            analyst,
            principal,
        })
    }

    /// Authenticates a request by its session token.
    ///
    /// # Errors
    ///
    /// [`AppError::Unauthenticated`] if the session is unknown, expired or revoked, or
    /// the Analyst is inactive.
    pub async fn authenticate(&self, token: &SessionToken) -> Result<SignedIn, AppError> {
        let now = self.ports.clock.now();
        let mut session = self
            .ports
            .sessions
            .find_by_token_hash(&token.hash())
            .await?
            .filter(|s| s.is_valid(now))
            .ok_or(AppError::Unauthenticated)?;
        let analyst = self
            .ports
            .analysts
            .get(session.analyst_id())
            .await?
            .filter(Analyst::is_active)
            .ok_or(AppError::Unauthenticated)?;
        if session.touch(self.session_policy, now) {
            self.ports.sessions.touch(&session).await?;
        }
        let principal = self.principal_for(&analyst).await?;
        Ok(SignedIn {
            session,
            token: None,
            analyst,
            principal,
        })
    }

    /// Ends a session.
    ///
    /// # Errors
    ///
    /// Storage failures.
    pub async fn sign_out(&self, session: &Session) -> Result<(), AppError> {
        let mut session = session.clone();
        session.revoke(self.ports.clock.now());
        self.ports.sessions.revoke(&session).await?;
        Ok(())
    }

    /// Changes the signed-in Analyst's own password after checking the current one.
    /// Every session of the Analyst is revoked; the caller signs in again.
    ///
    /// # Errors
    ///
    /// [`AppError::InvalidCredentials`] if `current` is wrong.
    pub async fn change_own_password(
        &self,
        principal: &Principal,
        current: &str,
        new: Password,
    ) -> Result<(), AppError> {
        let Actor::Analyst(id) = principal.actor() else {
            return Err(AppError::Unauthenticated);
        };
        let mut analyst = self
            .ports
            .analysts
            .get(id)
            .await?
            .ok_or(AppError::Unauthenticated)?;
        if !self
            .ports
            .hasher
            .verify(current, analyst.password_hash())
            .await
        {
            return Err(AppError::InvalidCredentials);
        }
        let before = analyst.clone();
        let now = self.ports.clock.now();
        analyst.set_password(self.ports.hasher.hash(&new).await?, now);
        let audit = audit_record(
            Some(&before),
            &analyst,
            AuditAction::Updated,
            principal.actor(),
            now,
        );
        self.ports.analysts.update(&analyst, &audit, true).await?;
        Ok(())
    }

    /// Builds the principal for an active Analyst.
    async fn principal_for(&self, analyst: &Analyst) -> Result<Principal, AppError> {
        let authorization = self.ports.authorization.load(analyst.id()).await?;
        Ok(Principal::new(
            Actor::Analyst(analyst.id()),
            analyst.is_admin(),
            authorization.capabilities,
            authorization.company_access.scope(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::fakes::{Fakes, setup_input};
    use chrono::Duration;
    use cooper_kernel::TenantScope;

    #[tokio::test]
    async fn setup_creates_default_company_and_administrator_once() {
        let fakes = Fakes::new();
        let svc = fakes.service();
        assert!(svc.setup_required().await.unwrap());
        let admin = svc.complete_setup(setup_input()).await.unwrap();
        assert!(admin.is_admin());
        assert!(fakes.state().companies.values().any(Company::is_default));
        assert_eq!(fakes.state().audits.len(), 2);
        assert!(!svc.setup_required().await.unwrap());
        assert_eq!(
            svc.complete_setup(setup_input()).await.unwrap_err(),
            AppError::SetupCompleted
        );
    }

    #[tokio::test]
    async fn sign_in_checks_password_and_active_flag_without_revealing_which() {
        let fakes = Fakes::new();
        let svc = fakes.service();
        svc.complete_setup(setup_input()).await.unwrap();
        let ok = svc
            .sign_in(SignIn {
                login: "ADMIN".into(),
                password: "correct horse battery".into(),
                ip_address: None,
                user_agent: None,
            })
            .await
            .unwrap();
        assert!(ok.principal.is_admin());
        assert_eq!(ok.principal.scope(), &TenantScope::All);
        for (login, password) in [
            ("admin", "wrong password!!"),
            ("nobody", "correct horse battery"),
        ] {
            let err = svc
                .sign_in(SignIn {
                    login: login.into(),
                    password: password.into(),
                    ip_address: None,
                    user_agent: None,
                })
                .await
                .unwrap_err();
            assert_eq!(err, AppError::InvalidCredentials);
        }
    }

    #[tokio::test]
    async fn sessions_expire_when_idle_and_after_sign_out() {
        let fakes = Fakes::new();
        let svc = fakes.service();
        svc.complete_setup(setup_input()).await.unwrap();
        let signed_in = fakes.sign_in_admin(&svc).await;
        let token = signed_in.token.clone().unwrap();
        assert!(svc.authenticate(&token).await.is_ok());

        fakes.clock.advance(Duration::hours(9));
        assert_eq!(
            svc.authenticate(&token).await.unwrap_err(),
            AppError::Unauthenticated
        );

        let again = fakes.sign_in_admin(&svc).await;
        let token = again.token.clone().unwrap();
        svc.sign_out(&again.session).await.unwrap();
        assert_eq!(
            svc.authenticate(&token).await.unwrap_err(),
            AppError::Unauthenticated
        );
    }

    #[tokio::test]
    async fn changing_password_revokes_sessions() {
        let fakes = Fakes::new();
        let svc = fakes.service();
        svc.complete_setup(setup_input()).await.unwrap();
        let signed_in = fakes.sign_in_admin(&svc).await;
        let token = signed_in.token.clone().unwrap();
        assert_eq!(
            svc.change_own_password(
                &signed_in.principal,
                "not my password",
                Password::new("another long password".into()).unwrap()
            )
            .await
            .unwrap_err(),
            AppError::InvalidCredentials
        );
        svc.change_own_password(
            &signed_in.principal,
            "correct horse battery",
            Password::new("another long password".into()).unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(
            svc.authenticate(&token).await.unwrap_err(),
            AppError::Unauthenticated
        );
    }
}
