//! Portal sign-in for Requesters (FR-PRT-001, ADR-0021): Portal Invitations, reset and
//! Self-Registration links, Portal sessions and switching between the Requesters of one
//! Portal Account.

use std::collections::BTreeSet;
use std::net::IpAddr;

use cooper_kernel::{Actor, AuditAction, Principal, RequesterId, TenantScope};

use super::{AppError, IdentityService, capabilities, require};
use crate::domain::{
    EmailAddress, IdentityError, Name, NewPortalSession, NewRequester, Password, PortalAccount,
    PortalLink, PortalLinkEmail, PortalLinkPurpose, PortalSession, Requester, SessionToken,
    audit_record,
};

/// A Portal sign-in request.
#[derive(Debug, Clone, Default)]
pub struct PortalSignIn {
    /// Email address.
    pub email: String,
    /// Password as typed.
    pub password: String,
    /// Client IP address, for IP bans and the session record.
    pub ip_address: Option<IpAddr>,
    /// Client user agent.
    pub user_agent: Option<String>,
}

/// A Self-Registration request.
#[derive(Debug, Clone)]
pub struct PortalRegistration {
    /// Name for the new Requester.
    pub full_name: Name,
    /// Email address at a Company's Sender Domain.
    pub email: EmailAddress,
}

/// What the Portal sign-in screen offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortalSignInOptions {
    /// Whether people may register themselves (`portal.self_registration`).
    pub self_registration: bool,
}

/// One Requester a Portal Account can act as.
#[derive(Debug, Clone)]
pub struct PortalIdentity {
    /// The Requester.
    pub requester: Requester,
    /// Name of the Requester's Company.
    pub company_name: String,
}

/// A successful Portal sign-in or authenticated Portal request.
#[derive(Debug, Clone)]
pub struct PortalSignedIn {
    /// The session.
    pub session: PortalSession,
    /// The raw token for the cookie. Only set right after sign-in.
    pub token: Option<SessionToken>,
    /// The account's email address.
    pub email: EmailAddress,
    /// Every Requester the account can act as; the current one first.
    pub identities: Vec<PortalIdentity>,
    /// The current Requester as a principal: no Capabilities, one Company.
    pub principal: Principal,
}

impl PortalSignedIn {
    /// The Requester the session acts as.
    #[must_use]
    pub fn current(&self) -> Option<&PortalIdentity> {
        self.identities.first()
    }
}

impl IdentityService {
    /// Active Requesters with `email` in active Companies, oldest first.
    async fn portal_identities(&self, email: &str) -> Result<Vec<PortalIdentity>, AppError> {
        let mut identities = Vec::new();
        for requester in self
            .ports
            .requesters
            .find_by_email(&TenantScope::All, email)
            .await?
        {
            if !requester.is_active() {
                continue;
            }
            if let Some(company) = self
                .ports
                .companies
                .get(&TenantScope::All, requester.tenant_id())
                .await?
                .filter(crate::domain::Company::is_active)
            {
                identities.push(PortalIdentity {
                    requester,
                    company_name: company.name().as_str().to_owned(),
                });
            }
        }
        Ok(identities)
    }

    /// Stores a link and emails it.
    async fn send_portal_link(
        &self,
        purpose: PortalLinkPurpose,
        email: &EmailAddress,
        name: &str,
        registration_name: Option<Name>,
    ) -> Result<(), AppError> {
        let now = self.ports.clock.now();
        let token = SessionToken::new(self.ports.tokens.token()?);
        let link = match registration_name {
            Some(full_name) => PortalLink::registration(&token, email.clone(), full_name, now),
            None => PortalLink::new(purpose, &token, email.clone(), now),
        };
        self.ports.portal.issue_link(&link).await?;
        self.ports
            .reset_mailer
            .send_portal_link(&PortalLinkEmail {
                purpose: link.purpose(),
                to: email.as_str().to_owned(),
                name: name.to_owned(),
                token: token.expose().to_owned(),
                expires_at: link.expires_at(),
            })
            .await?;
        tracing::info!(
            purpose = link.purpose().as_str(),
            "Portal link email queued"
        );
        Ok(())
    }

    /// Sends a Requester a Portal Invitation: a link to set a Portal password, valid
    /// 7 days (FR-PRT-001).
    ///
    /// # Errors
    ///
    /// Missing `requesters.manage`, an unknown Requester, or an inactive one.
    pub async fn invite_to_portal(
        &self,
        principal: &Principal,
        id: RequesterId,
    ) -> Result<(), AppError> {
        require(principal, capabilities::REQUESTERS_MANAGE)?;
        let requester = self
            .ports
            .requesters
            .get(principal.scope(), id)
            .await?
            .ok_or(AppError::NotFound("Requester"))?;
        if !requester.is_active() {
            return Err(IdentityError::InvalidState("the Requester is inactive").into());
        }
        self.send_portal_link(
            PortalLinkPurpose::Invitation,
            requester.email(),
            requester.full_name().as_str(),
            None,
        )
        .await
    }

    /// Emails a reset link to an address that has an active Requester. Always succeeds
    /// from the caller's point of view, so the answer reveals nothing.
    ///
    /// # Errors
    ///
    /// Storage failures only.
    pub async fn request_portal_reset(&self, email: &str) -> Result<(), AppError> {
        let Ok(email) = EmailAddress::try_from(email.trim().to_owned()) else {
            return Ok(());
        };
        let identities = self.portal_identities(email.as_str()).await?;
        let Some(first) = identities.first() else {
            tracing::info!("Portal reset requested for an address without an active Requester");
            return Ok(());
        };
        let name = first.requester.full_name().as_str().to_owned();
        self.send_portal_link(PortalLinkPurpose::Reset, &email, &name, None)
            .await
    }

    /// What the Portal sign-in screen offers. Public: needs no session.
    pub async fn portal_sign_in_options(&self) -> PortalSignInOptions {
        PortalSignInOptions {
            self_registration: self.ports.policy.policy().await.portal_self_registration,
        }
    }

    /// Self-Registration (when `portal.self_registration` is on): emails a link that
    /// confirms the address, sets the password and creates the Requester. If an active
    /// Requester already has the address, a reset link is sent instead.
    ///
    /// # Errors
    ///
    /// Self-Registration is off, or the address is not at a Company's Sender Domain.
    pub async fn register_for_portal(&self, input: PortalRegistration) -> Result<(), AppError> {
        if !self.ports.policy.policy().await.portal_self_registration {
            return Err(IdentityError::InvalidState("Self-Registration is turned off").into());
        }
        self.registration_company(&input.email).await?;
        let identities = self.portal_identities(input.email.as_str()).await?;
        if let Some(first) = identities.first() {
            let name = first.requester.full_name().as_str().to_owned();
            return self
                .send_portal_link(PortalLinkPurpose::Reset, &input.email, &name, None)
                .await;
        }
        let name = input.full_name.as_str().to_owned();
        self.send_portal_link(
            PortalLinkPurpose::Registration,
            &input.email,
            &name,
            Some(input.full_name),
        )
        .await
    }

    /// The Company whose Sender Domain matches `email` (never a Freemail Domain).
    async fn registration_company(
        &self,
        email: &EmailAddress,
    ) -> Result<cooper_kernel::TenantId, AppError> {
        let domain = email.domain();
        let not_allowed = || {
            AppError::Rule(IdentityError::Invalid {
                field: "email",
                reason: "must be an address at a Company's Sender Domain".into(),
            })
        };
        if self
            .ports
            .companies
            .freemail_domains()
            .await?
            .contains(domain)
        {
            return Err(not_allowed());
        }
        let tenant = self
            .ports
            .companies
            .find_by_sender_domain(domain)
            .await?
            .ok_or_else(not_allowed)?;
        let active = self
            .ports
            .companies
            .get(&TenantScope::All, tenant)
            .await?
            .is_some_and(|c| c.is_active());
        if active {
            Ok(tenant)
        } else {
            Err(not_allowed())
        }
    }

    /// Sets a Portal password with the token of an emailed link. Creates the Portal
    /// Account on first use and, for Self-Registration, the Requester. Every other
    /// Portal session of the account ends.
    ///
    /// # Errors
    ///
    /// [`AppError::InvalidCredentials`] for an unknown, used or expired token, or when
    /// the address's Company no longer accepts Self-Registration.
    pub async fn set_portal_password(
        &self,
        token: &str,
        password: Password,
    ) -> Result<(), AppError> {
        let now = self.ports.clock.now();
        let link = self
            .ports
            .portal
            .consume_link(&SessionToken::new(token.trim().to_owned()).hash(), now)
            .await?
            .ok_or(AppError::InvalidCredentials)?;
        if let Some(full_name) = link.full_name() {
            self.complete_registration(link.email(), full_name.clone())
                .await?;
        }
        let hash = self.ports.hasher.hash(&password).await?;
        let (before, account) = match self
            .ports
            .portal
            .account_by_email(link.email().as_str())
            .await?
        {
            Some(existing) => {
                let mut changed = existing.clone();
                changed.set_password(hash, now);
                (Some(existing), changed)
            }
            None => (None, PortalAccount::create(link.email().clone(), hash, now)),
        };
        let action = if before.is_some() {
            AuditAction::Updated
        } else {
            AuditAction::Created
        };
        let audit = audit_record(before.as_ref(), &account, action, Actor::Automation, now);
        self.ports.portal.save_account(&account, &audit).await?;
        tracing::info!(portal_account_id = %account.id(), purpose = link.purpose().as_str(), "Portal password set");
        Ok(())
    }

    /// Creates the Requester of a confirmed Self-Registration, unless one exists.
    async fn complete_registration(
        &self,
        email: &EmailAddress,
        full_name: Name,
    ) -> Result<(), AppError> {
        let tenant = self
            .registration_company(email)
            .await
            .map_err(|_| AppError::InvalidCredentials)?;
        let scope = TenantScope::single(tenant);
        if !self
            .ports
            .requesters
            .find_by_email(&scope, email.as_str())
            .await?
            .is_empty()
        {
            return Ok(());
        }
        let now = self.ports.clock.now();
        let requester = Requester::create(
            tenant,
            NewRequester {
                full_name,
                email: email.clone(),
                username: None,
                phone: None,
                manager_id: None,
                cost_centre_id: None,
            },
            now,
        )?;
        let audit = audit_record(
            None,
            &requester,
            AuditAction::Created,
            Actor::Requester(requester.id()),
            now,
        );
        self.ports
            .requesters
            .insert(&scope, &requester, &audit)
            .await?;
        tracing::info!(requester_id = %requester.id(), "Requester created by Self-Registration");
        Ok(())
    }

    /// Signs a Requester in to the Portal. The session acts as the oldest active
    /// Requester with the address; [`IdentityService::switch_portal_requester`] changes it.
    ///
    /// # Errors
    ///
    /// [`AppError::TooManyAttempts`] while the IP is banned; otherwise
    /// [`AppError::InvalidCredentials`] for an unknown address, a wrong password or an
    /// address without an active Requester, without saying which.
    pub async fn portal_sign_in(&self, request: PortalSignIn) -> Result<PortalSignedIn, AppError> {
        let now = self.ports.clock.now();
        let policy = self.ports.policy.policy().await;
        if let Some(ip) = request.ip_address
            && let Some(until) = self.ports.throttle.banned_until(ip, now).await?
        {
            return Err(AppError::TooManyAttempts { retry_after: until });
        }
        let login = request.email.trim();
        let account = self.ports.portal.account_by_email(login).await?;
        let password_ok = self
            .ports
            .hasher
            .verify(
                &request.password,
                account.as_ref().map(|a| a.password_hash()),
            )
            .await;
        let identities = match &account {
            Some(a) if password_ok => self.portal_identities(a.email().as_str()).await?,
            _ => Vec::new(),
        };
        let (Some(account), Some(first)) = (account, identities.first()) else {
            return Err(self
                .failed_sign_in(
                    login,
                    request.ip_address,
                    &policy,
                    AppError::InvalidCredentials,
                )
                .await);
        };
        if let Some(ip) = request.ip_address {
            self.ports.throttle.clear(ip).await?;
        }
        let token = SessionToken::new(self.ports.tokens.token()?);
        let session = PortalSession::start(
            NewPortalSession {
                account_id: account.id(),
                requester_id: first.requester.id(),
                token: &token,
                csrf_token: self.ports.tokens.token()?,
                ip_address: request.ip_address.map(|ip| ip.to_string()),
                user_agent: request.user_agent,
            },
            self.session_policy,
            now,
        );
        self.ports.portal.insert_session(&session).await?;
        tracing::info!(portal_account_id = %account.id(), session_id = %session.id(), "signed in to the Portal");
        Ok(signed_in(
            session,
            Some(token),
            account.email().clone(),
            identities,
        ))
    }

    /// Authenticates a Portal request by its session token.
    ///
    /// # Errors
    ///
    /// [`AppError::Unauthenticated`] if the session is unknown, expired or revoked, or
    /// its Requester is no longer active.
    pub async fn portal_authenticate(
        &self,
        token: &SessionToken,
    ) -> Result<PortalSignedIn, AppError> {
        let now = self.ports.clock.now();
        let mut session = self
            .ports
            .portal
            .session_by_token_hash(&token.hash())
            .await?
            .filter(|s| s.is_valid(now))
            .ok_or(AppError::Unauthenticated)?;
        let account = self
            .ports
            .portal
            .account(session.account_id())
            .await?
            .ok_or(AppError::Unauthenticated)?;
        let identities = self.portal_identities(account.email().as_str()).await?;
        if !identities
            .iter()
            .any(|i| i.requester.id() == session.requester_id())
        {
            return Err(AppError::Unauthenticated);
        }
        if session.touch(self.session_policy, now) {
            self.ports.portal.update_session(&session).await?;
        }
        Ok(signed_in(
            session,
            None,
            account.email().clone(),
            identities,
        ))
    }

    /// Makes the session act as another active Requester with the account's address.
    ///
    /// # Errors
    ///
    /// [`AppError::NotFound`] if the Requester is not one of the account's.
    pub async fn switch_portal_requester(
        &self,
        current: &PortalSignedIn,
        requester_id: RequesterId,
    ) -> Result<PortalSignedIn, AppError> {
        let identities = self.portal_identities(current.email.as_str()).await?;
        if !identities.iter().any(|i| i.requester.id() == requester_id) {
            return Err(AppError::NotFound("Requester"));
        }
        let mut session = current.session.clone();
        session.switch_requester(requester_id);
        self.ports.portal.update_session(&session).await?;
        Ok(signed_in(session, None, current.email.clone(), identities))
    }

    /// Ends a Portal session.
    ///
    /// # Errors
    ///
    /// Storage failures.
    pub async fn portal_sign_out(&self, session: &PortalSession) -> Result<(), AppError> {
        let mut session = session.clone();
        session.revoke(self.ports.clock.now());
        self.ports.portal.update_session(&session).await?;
        Ok(())
    }
}

/// Orders the identities current-first and builds the Requester principal.
fn signed_in(
    session: PortalSession,
    token: Option<SessionToken>,
    email: EmailAddress,
    mut identities: Vec<PortalIdentity>,
) -> PortalSignedIn {
    if let Some(i) = identities
        .iter()
        .position(|i| i.requester.id() == session.requester_id())
    {
        identities[..=i].rotate_right(1);
    }
    let tenant = identities.first().map(|i| i.requester.tenant_id());
    let principal = Principal::new(
        Actor::Requester(session.requester_id()),
        false,
        BTreeSet::new(),
        tenant.map_or_else(
            || TenantScope::Tenants(BTreeSet::new()),
            TenantScope::single,
        ),
    );
    PortalSignedIn {
        session,
        token,
        email,
        identities,
        principal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::fakes::{Fakes, setup_input};
    use crate::application::{RequesterInput, SignIn};
    use crate::domain::SecurityPolicy;
    use cooper_kernel::TenantId;

    async fn admin(fakes: &Fakes, svc: &IdentityService) -> Principal {
        svc.complete_setup(setup_input()).await.unwrap();
        fakes.sign_in_admin(svc).await.principal
    }

    async fn requester(
        svc: &IdentityService,
        admin: &Principal,
        tenant: TenantId,
        email: &str,
    ) -> Requester {
        svc.create_requester(
            admin,
            tenant,
            RequesterInput {
                full_name: Name::try_from("Sam Smith").unwrap(),
                email: EmailAddress::try_from(email).unwrap(),
                username: None,
                phone: None,
                manager_id: None,
                cost_centre_id: None,
            },
        )
        .await
        .unwrap()
    }

    fn password() -> Password {
        Password::new("a portal password".into()).unwrap()
    }

    fn sign_in(email: &str) -> PortalSignIn {
        PortalSignIn {
            email: email.into(),
            password: "a portal password".into(),
            ..PortalSignIn::default()
        }
    }

    #[tokio::test]
    async fn invited_requester_sets_a_password_and_signs_in_without_capabilities() {
        let fakes = Fakes::new();
        let svc = fakes.service();
        let admin = admin(&fakes, &svc).await;
        let sam = requester(&svc, &admin, fakes.default_company_id(), "sam@acme.example").await;
        assert_eq!(
            svc.portal_sign_in(sign_in("sam@acme.example"))
                .await
                .unwrap_err(),
            AppError::InvalidCredentials,
            "no password yet"
        );
        svc.invite_to_portal(&admin, sam.id()).await.unwrap();
        let mail = fakes.state().portal_mails[0].clone();
        assert_eq!(mail.purpose, PortalLinkPurpose::Invitation);
        assert_eq!(mail.to, "sam@acme.example");
        svc.set_portal_password(&mail.token, password())
            .await
            .unwrap();
        assert_eq!(
            svc.set_portal_password(&mail.token, password())
                .await
                .unwrap_err(),
            AppError::InvalidCredentials,
            "links work once"
        );

        let signed_in = svc
            .portal_sign_in(sign_in("SAM@acme.example"))
            .await
            .unwrap();
        assert_eq!(signed_in.principal.actor(), Actor::Requester(sam.id()));
        assert!(signed_in.principal.capabilities().is_empty());
        assert!(!signed_in.principal.is_admin());
        assert_eq!(
            signed_in.principal.scope(),
            &TenantScope::single(sam.tenant_id())
        );
        let token = signed_in.token.clone().unwrap();
        assert!(svc.portal_authenticate(&token).await.is_ok());
        assert!(
            svc.sign_in(SignIn {
                login: "sam@acme.example".into(),
                password: "a portal password".into(),
                ..SignIn::default()
            })
            .await
            .is_err(),
            "a Portal Account is not an Analyst"
        );

        let mut changed = sam.clone();
        changed
            .deactivate(cooper_kernel::Clock::now(&*fakes.clock))
            .unwrap();
        fakes.state().requesters.insert(sam.id(), changed);
        assert_eq!(
            svc.portal_authenticate(&token).await.unwrap_err(),
            AppError::Unauthenticated,
            "deactivating the Requester ends Portal access"
        );
    }

    #[tokio::test]
    async fn one_account_switches_between_companies() {
        let fakes = Fakes::new();
        let svc = fakes.service();
        let admin = admin(&fakes, &svc).await;
        let other = svc
            .create_company(
                &admin,
                crate::application::CompanyInput {
                    name: Name::try_from("Globex").unwrap(),
                    ticket_code: crate::domain::TicketCode::try_from("GLX").unwrap(),
                    sender_domains: BTreeSet::new(),
                },
            )
            .await
            .unwrap();
        let first = requester(&svc, &admin, fakes.default_company_id(), "pat@x.example").await;
        let second = requester(&svc, &admin, other.id(), "pat@x.example").await;
        svc.request_portal_reset("pat@x.example").await.unwrap();
        let token = fakes.state().portal_mails[0].token.clone();
        svc.set_portal_password(&token, password()).await.unwrap();

        let signed_in = svc.portal_sign_in(sign_in("pat@x.example")).await.unwrap();
        assert_eq!(signed_in.identities.len(), 2);
        assert_eq!(signed_in.principal.actor(), Actor::Requester(first.id()));
        let switched = svc
            .switch_portal_requester(&signed_in, second.id())
            .await
            .unwrap();
        assert_eq!(switched.principal.actor(), Actor::Requester(second.id()));
        assert_eq!(switched.current().unwrap().company_name, "Globex");
        let again = svc
            .portal_authenticate(signed_in.token.as_ref().unwrap())
            .await
            .unwrap();
        assert_eq!(again.principal.scope(), &TenantScope::single(other.id()));
        let stranger = requester(&svc, &admin, other.id(), "lee@x.example").await;
        assert_eq!(
            svc.switch_portal_requester(&again, stranger.id())
                .await
                .unwrap_err(),
            AppError::NotFound("Requester")
        );
    }

    #[tokio::test]
    async fn self_registration_is_off_by_default_and_limited_to_sender_domains() {
        let fakes = Fakes::new();
        let svc = fakes.service();
        let mut setup = setup_input();
        setup.sender_domains =
            BTreeSet::from([crate::domain::DomainName::try_from("acme.example").unwrap()]);
        svc.complete_setup(setup).await.unwrap();
        let register = |email: &str| PortalRegistration {
            full_name: Name::try_from("New Person").unwrap(),
            email: EmailAddress::try_from(email).unwrap(),
        };
        assert!(matches!(
            svc.register_for_portal(register("new@acme.example")).await,
            Err(AppError::Rule(IdentityError::InvalidState(_)))
        ));
        fakes.set_policy(SecurityPolicy {
            portal_self_registration: true,
            ..SecurityPolicy::default()
        });
        assert!(matches!(
            svc.register_for_portal(register("new@elsewhere.example"))
                .await,
            Err(AppError::Rule(IdentityError::Invalid {
                field: "email",
                ..
            }))
        ));
        svc.register_for_portal(register("new@acme.example"))
            .await
            .unwrap();
        let mail = fakes.state().portal_mails[0].clone();
        assert_eq!(mail.purpose, PortalLinkPurpose::Registration);
        assert!(
            fakes.state().requesters.is_empty(),
            "nothing is created before the address is confirmed"
        );
        svc.set_portal_password(&mail.token, password())
            .await
            .unwrap();
        let created = fakes.state().requesters.values().next().cloned().unwrap();
        assert_eq!(created.full_name().as_str(), "New Person");
        assert_eq!(created.tenant_id(), fakes.default_company_id());
        assert!(
            svc.portal_sign_in(sign_in("new@acme.example"))
                .await
                .is_ok()
        );

        svc.register_for_portal(register("new@acme.example"))
            .await
            .unwrap();
        assert_eq!(
            fakes.state().portal_mails[1].purpose,
            PortalLinkPurpose::Reset,
            "a known address gets a reset link, not a second Requester"
        );
    }

    #[tokio::test]
    async fn reset_requests_never_reveal_unknown_addresses() {
        let fakes = Fakes::new();
        let svc = fakes.service();
        svc.complete_setup(setup_input()).await.unwrap();
        svc.request_portal_reset("nobody@acme.example")
            .await
            .unwrap();
        svc.request_portal_reset("not an address").await.unwrap();
        assert!(fakes.state().portal_mails.is_empty());
    }
}
