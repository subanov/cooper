//! First-run setup, sign-in, request authentication and sign-out (FR-SYS-004,
//! FR-IAM-002).

use std::collections::BTreeSet;
use std::net::IpAddr;

use cooper_kernel::{Actor, AuditAction, Principal};

use super::{AppError, IdentityService};
use crate::domain::{
    Analyst, Company, DomainName, EmailAddress, IdentityError, Name, NewAnalyst, Password,
    Revocations, SecurityPolicy, Session, SessionToken, TicketCode, TrustedDevice, Username,
    audit_record,
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
#[derive(Debug, Clone, Default)]
pub struct SignIn {
    /// Username or email address.
    pub login: String,
    /// Password as typed.
    pub password: String,
    /// Two-factor code, when the Analyst uses two-factor sign-in.
    pub totp_code: Option<String>,
    /// Remember this browser for 30 days after a successful two-factor sign-in.
    pub remember_device: bool,
    /// Token of a remembered browser, from its cookie.
    pub trusted_device: Option<SessionToken>,
    /// Client IP address, for IP bans and the session record.
    pub ip_address: Option<IpAddr>,
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
    /// Token for the trusted-device cookie, when the browser was just remembered.
    pub trusted_device_token: Option<SessionToken>,
    /// The signed-in Analyst.
    pub analyst: Analyst,
    /// Their effective permissions and scope.
    pub principal: Principal,
    /// Two-factor sign-in is required but not set up yet: until it is, only the session
    /// and enrollment endpoints may be used.
    pub totp_enrollment_required: bool,
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

    /// Signs an Analyst in and starts a session (FR-IAM-002, FR-IAM-003, FR-IAM-005).
    ///
    /// # Errors
    ///
    /// - [`AppError::TooManyAttempts`] while the client IP is banned;
    /// - [`AppError::InvalidCredentials`] for an unknown login, wrong password or inactive
    ///   Analyst, without saying which;
    /// - [`AppError::TotpRequired`] when the password is right but a two-factor code is
    ///   needed, and [`IdentityError::InvalidTotpCode`] when the code is wrong.
    pub async fn sign_in(&self, request: SignIn) -> Result<SignedIn, AppError> {
        let now = self.ports.clock.now();
        let policy = self.ports.policy.policy().await;
        if let Some(ip) = request.ip_address
            && let Some(until) = self.ports.throttle.banned_until(ip, now).await?
        {
            return Err(AppError::TooManyAttempts { retry_after: until });
        }
        let login = request.login.trim();
        let analyst = self.ports.analysts.find_by_login(login).await?;
        let hash = analyst.as_ref().and_then(Analyst::password_hash);
        let password_ok = self.ports.hasher.verify(&request.password, hash).await;
        let mut analyst = match analyst {
            Some(a) if password_ok && a.is_active() => a,
            _ => {
                return Err(self
                    .failed_sign_in(
                        request.login.trim(),
                        request.ip_address,
                        &policy,
                        AppError::InvalidCredentials,
                    )
                    .await);
            }
        };

        let mut remember = false;
        if analyst.totp_enabled() {
            let trusted = match &request.trusted_device {
                Some(token) => self
                    .ports
                    .trusted_devices
                    .find_by_token_hash(&token.hash())
                    .await?
                    .is_some_and(|d| d.trusts(analyst.id(), now)),
                None => false,
            };
            if !trusted {
                let Some(code) = request
                    .totp_code
                    .as_deref()
                    .filter(|c| !c.trim().is_empty())
                else {
                    return Err(AppError::TotpRequired);
                };
                let before = analyst.clone();
                if !analyst.verify_totp(code, now) {
                    let err = AppError::Rule(IdentityError::InvalidTotpCode);
                    return Err(self
                        .failed_sign_in(request.login.trim(), request.ip_address, &policy, err)
                        .await);
                }
                // Store the used step so the code cannot be replayed.
                let audit = audit_record(
                    Some(&before),
                    &analyst,
                    AuditAction::Updated,
                    Actor::Analyst(analyst.id()),
                    now,
                );
                self.ports
                    .analysts
                    .update(&analyst, &audit, Revocations::NONE)
                    .await?;
                remember = request.remember_device;
            }
        }
        if let Some(ip) = request.ip_address {
            self.ports.throttle.clear(ip).await?;
        }

        let token = SessionToken::new(self.ports.tokens.token()?);
        let csrf = self.ports.tokens.token()?;
        let session = Session::start(
            analyst.id(),
            &token,
            csrf,
            self.session_policy,
            now,
            request.ip_address.map(|ip| ip.to_string()),
            request.user_agent.clone(),
        );
        self.ports.sessions.insert(&session).await?;
        let trusted_device_token = if remember {
            let device_token = SessionToken::new(self.ports.tokens.token()?);
            let device =
                TrustedDevice::remember(analyst.id(), &device_token, request.user_agent, now);
            self.ports.trusted_devices.insert(&device).await?;
            Some(device_token)
        } else {
            None
        };
        let principal = self.principal_for(&analyst).await?;
        tracing::info!(analyst_id = %analyst.id(), session_id = %session.id(), "signed in");
        Ok(SignedIn {
            session,
            token: Some(token),
            trusted_device_token,
            totp_enrollment_required: policy.require_totp && !analyst.totp_enabled(),
            analyst,
            principal,
        })
    }

    /// Records a failed sign-in for the client IP; turns `err` into
    /// [`AppError::TooManyAttempts`] when this failure starts a ban.
    pub(super) async fn failed_sign_in(
        &self,
        login: &str,
        ip_address: Option<IpAddr>,
        policy: &SecurityPolicy,
        err: AppError,
    ) -> AppError {
        tracing::info!(%login, ip = ?ip_address, "failed sign-in");
        let Some(ip) = ip_address else {
            return err;
        };
        let now = self.ports.clock.now();
        match self
            .ports
            .throttle
            .record_failure(ip, login, now, policy)
            .await
        {
            Ok(Some(until)) => {
                tracing::warn!(%ip, "IP address banned after repeated failed sign-ins");
                AppError::TooManyAttempts { retry_after: until }
            }
            Ok(None) => err,
            Err(e) => AppError::from(e),
        }
    }

    /// Authenticates a request by its session token.
    ///
    /// # Errors
    ///
    /// [`AppError::Unauthenticated`] if the session is unknown, expired or revoked, or
    /// the Analyst is inactive.
    pub async fn authenticate(&self, token: &SessionToken) -> Result<SignedIn, AppError> {
        let now = self.ports.clock.now();
        let (mut session, analyst) = self.active_session(token, now).await?;
        if session.touch(self.session_policy, now) {
            self.ports.sessions.touch(&session).await?;
        }
        let principal = self.principal_for(&analyst).await?;
        let policy = self.ports.policy.policy().await;
        Ok(SignedIn {
            session,
            token: None,
            trusted_device_token: None,
            totp_enrollment_required: policy.require_totp && !analyst.totp_enabled(),
            analyst,
            principal,
        })
    }

    /// Re-checks a session for a long-lived connection (Live Updates, ADR-0022) and
    /// returns the Analyst's current Principal, reflecting changed Roles and Company
    /// access. Unlike [`IdentityService::authenticate`] it does not extend the idle
    /// timeout: an open stream is not activity.
    ///
    /// # Errors
    ///
    /// [`AppError::Unauthenticated`] if the session is unknown, expired or revoked, or
    /// the Analyst is inactive.
    pub async fn revalidate(&self, token: &SessionToken) -> Result<Principal, AppError> {
        let (_, analyst) = self.active_session(token, self.ports.clock.now()).await?;
        self.principal_for(&analyst).await
    }

    /// The valid session behind `token` and its active Analyst.
    async fn active_session(
        &self,
        token: &SessionToken,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<(Session, Analyst), AppError> {
        let session = self
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
        Ok((session, analyst))
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
        self.ports
            .analysts
            .update(&analyst, &audit, Revocations::ALL)
            .await?;
        Ok(())
    }

    /// Builds the principal for an active Analyst.
    pub(super) async fn principal_for(&self, analyst: &Analyst) -> Result<Principal, AppError> {
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
                ..SignIn::default()
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
                    ..SignIn::default()
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
    async fn revalidating_a_live_stream_does_not_extend_the_idle_timeout() {
        let fakes = Fakes::new();
        let svc = fakes.service();
        svc.complete_setup(setup_input()).await.unwrap();
        let signed_in = fakes.sign_in_admin(&svc).await;
        let token = signed_in.token.clone().unwrap();

        fakes.clock.advance(Duration::hours(5));
        let principal = svc.revalidate(&token).await.unwrap();
        assert_eq!(principal, signed_in.principal);

        fakes.clock.advance(Duration::hours(5));
        assert_eq!(
            svc.revalidate(&token).await.unwrap_err(),
            AppError::Unauthenticated,
            "idle for 10 hours although the stream re-checked after 5"
        );

        let again = fakes.sign_in_admin(&svc).await;
        let token = again.token.clone().unwrap();
        svc.sign_out(&again.session).await.unwrap();
        assert_eq!(
            svc.revalidate(&token).await.unwrap_err(),
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
