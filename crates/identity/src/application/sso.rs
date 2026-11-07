//! OpenID Connect single sign-on for Analysts and Requesters (FR-IAM-012, ADR-0025).
//!
//! Authorization code flow with PKCE, `state` (single use, stored hashed) and `nonce`.
//! A subject is linked once; afterwards the link decides who signs in. A first sign-in
//! is linked to the Analyst or Portal Account with the same address only when the
//! provider says the address is verified and the provider allows it. SSO never creates
//! Analysts. Requesters must already exist (an active Requester with the address).

use std::net::IpAddr;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use cooper_kernel::{AuditAction, Principal};
use sha2::{Digest as _, Sha256};
use uuid::Uuid;

use super::{AppError, IdentityService, PortalSignedIn, SignedIn};
use crate::domain::{
    Analyst, Password, PortalAccount, PortalSession, SecretChange, Session, SessionToken,
    SsoAudience, SsoIdentity, SsoLogin, SsoProvider, SsoProviderInput, audit_record, check_claims,
    safe_return_to,
};

/// The result of a successful callback.
#[derive(Debug)]
pub enum SsoSignedIn {
    /// An Analyst session.
    Analyst(Box<SignedIn>, String),
    /// A Portal session.
    Portal(Box<PortalSignedIn>, String),
}

/// Path of the callback under the public address.
pub const SSO_CALLBACK_PATH: &str = "/api/v1/sso/callback";

fn sha256(s: &str) -> Vec<u8> {
    Sha256::digest(s.as_bytes()).to_vec()
}

impl IdentityService {
    async fn redirect_uri(&self) -> String {
        format!(
            "{}{SSO_CALLBACK_PATH}",
            self.ports.settings.base_url().await.trim_end_matches('/')
        )
    }

    /// Every provider (Administrators).
    ///
    /// # Errors
    ///
    /// Forbidden.
    pub async fn sso_providers(&self, principal: &Principal) -> Result<Vec<SsoProvider>, AppError> {
        principal.require_admin()?;
        Ok(self.ports.sso.providers().await?)
    }

    /// Active providers for a sign-in page (no secrets; public).
    ///
    /// # Errors
    ///
    /// Storage failures.
    pub async fn sso_sign_in_options(
        &self,
        audience: SsoAudience,
    ) -> Result<Vec<SsoProvider>, AppError> {
        Ok(self
            .ports
            .sso
            .providers()
            .await?
            .into_iter()
            .filter(|p| p.input().is_active && p.input().audience.serves(audience))
            .collect())
    }

    /// Registers a provider (Administrators).
    ///
    /// # Errors
    ///
    /// Forbidden or invalid input.
    pub async fn create_sso_provider(
        &self,
        principal: &Principal,
        input: SsoProviderInput,
        client_secret: Option<&str>,
    ) -> Result<SsoProvider, AppError> {
        principal.require_admin()?;
        let now = self.ports.clock.now();
        let provider = SsoProvider::create(input, now)?;
        let audit = audit_record(
            None,
            &provider,
            AuditAction::Created,
            principal.actor(),
            now,
        );
        let secret = client_secret
            .filter(|s| !s.is_empty())
            .map_or(SecretChange::Clear, SecretChange::Set);
        self.ports
            .sso
            .save_provider(&provider, secret, &audit)
            .await?;
        Ok(provider)
    }

    /// Changes a provider; `client_secret` `None` keeps the stored secret, `Some("")`
    /// removes it (Administrators).
    ///
    /// # Errors
    ///
    /// Forbidden, not found or invalid input.
    pub async fn update_sso_provider(
        &self,
        principal: &Principal,
        id: Uuid,
        input: SsoProviderInput,
        client_secret: Option<&str>,
    ) -> Result<SsoProvider, AppError> {
        principal.require_admin()?;
        let mut provider = self.sso_provider(id).await?;
        let before = provider.clone();
        let now = self.ports.clock.now();
        provider.update(input, now)?;
        let audit = audit_record(
            Some(&before),
            &provider,
            AuditAction::Updated,
            principal.actor(),
            now,
        );
        let secret = match client_secret {
            None => SecretChange::Keep,
            Some("") => SecretChange::Clear,
            Some(s) => SecretChange::Set(s),
        };
        self.ports
            .sso
            .save_provider(&provider, secret, &audit)
            .await?;
        Ok(provider)
    }

    async fn sso_provider(&self, id: Uuid) -> Result<SsoProvider, AppError> {
        self.ports
            .sso
            .providers()
            .await?
            .into_iter()
            .find(|p| p.id() == id)
            .ok_or(AppError::NotFound("SSO provider"))
    }

    /// Starts a sign-in: stores state, nonce and PKCE verifier and returns the URL to
    /// send the browser to. `who` is [`SsoAudience::Analysts`] or
    /// [`SsoAudience::Requesters`].
    ///
    /// # Errors
    ///
    /// Unknown or inactive provider, a provider not for `who`, or an unreachable
    /// provider.
    pub async fn start_sso(
        &self,
        provider: Uuid,
        who: SsoAudience,
        return_to: Option<&str>,
    ) -> Result<String, AppError> {
        let provider = self.sso_provider(provider).await?;
        if who == SsoAudience::Both
            || !provider.input().is_active
            || !provider.input().audience.serves(who)
        {
            return Err(AppError::NotFound("SSO provider"));
        }
        let state = self.ports.tokens.token()?;
        let nonce = self.ports.tokens.token()?;
        let verifier = self.ports.tokens.token()?;
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let fallback = if who == SsoAudience::Requesters {
            "/portal"
        } else {
            "/"
        };
        let login = SsoLogin {
            state_hash: sha256(&state),
            provider_id: provider.id(),
            audience: who,
            nonce: nonce.clone(),
            pkce_verifier: verifier,
            return_to: safe_return_to(return_to, fallback),
            expires_at: SsoLogin::expiry(self.ports.clock.now()),
        };
        self.ports.sso.insert_login(&login).await?;
        let redirect_uri = self.redirect_uri().await;
        self.ports
            .oidc
            .authorization_url(&provider, &redirect_uri, &state, &nonce, &challenge)
            .await
            .map_err(|e| {
                tracing::warn!(error = %e, provider = %provider.id(), "SSO provider unreachable");
                AppError::SsoFailed
            })
    }

    /// Completes a sign-in: checks the state, exchanges the code, verifies the ID token
    /// and starts a session for the linked (or matching) Analyst or Portal Account.
    ///
    /// # Errors
    ///
    /// [`AppError::SsoFailed`] for anything that does not check out (details are logged,
    /// not returned).
    pub async fn finish_sso(
        &self,
        state: &str,
        code: &str,
        ip_address: Option<IpAddr>,
        user_agent: Option<String>,
    ) -> Result<SsoSignedIn, AppError> {
        let now = self.ports.clock.now();
        let fail = |why: &str| {
            tracing::warn!(reason = why, "SSO sign-in refused");
            AppError::SsoFailed
        };
        let login = self
            .ports
            .sso
            .take_login(&sha256(state))
            .await?
            .filter(|l| l.is_valid(now))
            .ok_or_else(|| fail("unknown, used or expired state"))?;
        let provider = self
            .sso_provider(login.provider_id)
            .await
            .map_err(|_| fail("provider removed"))?;
        if !provider.input().is_active {
            return Err(fail("provider inactive"));
        }
        let secret = self.ports.sso.client_secret(provider.id()).await?;
        let redirect_uri = self.redirect_uri().await;
        let claims = self
            .ports
            .oidc
            .exchange(
                &provider,
                secret.as_deref(),
                &redirect_uri,
                code,
                &login.pkce_verifier,
            )
            .await
            .map_err(|e| fail(&e))?;
        check_claims(
            &claims,
            &provider.input().issuer_url,
            &provider.input().client_id,
            &login.nonce,
            now,
        )
        .map_err(|e| fail(&e.to_string()))?;
        let verified_email = claims
            .email
            .as_deref()
            .filter(|_| claims.email_verified && provider.input().link_by_verified_email)
            .map(|e| e.trim().to_lowercase());
        let linked = self
            .ports
            .sso
            .identity(provider.id(), login.audience, &claims.sub)
            .await?;
        match login.audience {
            SsoAudience::Requesters => {
                let email = match linked {
                    Some(SsoIdentity::PortalEmail(e)) => e,
                    Some(SsoIdentity::Analyst(_)) => {
                        return Err(fail("subject linked to an Analyst"));
                    }
                    None => verified_email.ok_or_else(|| fail("no link and no verified email"))?,
                };
                let identities = self.portal_identities(&email).await?;
                if identities.is_empty() {
                    return Err(fail("no active Requester with this address"));
                }
                let account = match self.ports.portal.account_by_email(&email).await? {
                    Some(a) => a,
                    None => {
                        // An account the person can only reach through SSO or a reset link.
                        let random = Password::new(self.ports.tokens.token()?)?;
                        let hash = self.ports.hasher.hash(&random).await?;
                        let account = PortalAccount::create(
                            identities[0].requester.email().clone(),
                            hash,
                            now,
                        );
                        let audit = audit_record(
                            None,
                            &account,
                            AuditAction::Created,
                            cooper_kernel::Actor::Automation,
                            now,
                        );
                        self.ports.portal.save_account(&account, &audit).await?;
                        account
                    }
                };
                self.ports
                    .sso
                    .link(
                        provider.id(),
                        login.audience,
                        &claims.sub,
                        &SsoIdentity::PortalEmail(email.clone()),
                        now,
                    )
                    .await?;
                let token = SessionToken::new(self.ports.tokens.token()?);
                let session = PortalSession::start(
                    crate::domain::NewPortalSession {
                        account_id: account.id(),
                        requester_id: identities[0].requester.id(),
                        token: &token,
                        csrf_token: self.ports.tokens.token()?,
                        ip_address: ip_address.map(|ip| ip.to_string()),
                        user_agent,
                    },
                    self.session_policy,
                    now,
                );
                self.ports.portal.insert_session(&session).await?;
                tracing::info!(portal_account_id = %account.id(), provider = %provider.id(), "signed in to the Portal with SSO");
                Ok(SsoSignedIn::Portal(
                    Box::new(super::portal::signed_in(
                        session,
                        Some(token),
                        account.email().clone(),
                        identities,
                    )),
                    login.return_to,
                ))
            }
            _ => {
                let analyst = match linked {
                    Some(SsoIdentity::Analyst(a)) => self.ports.analysts.get(a).await?,
                    Some(SsoIdentity::PortalEmail(_)) => {
                        return Err(fail("subject linked to a Portal Account"));
                    }
                    None => match verified_email {
                        Some(email) => self
                            .ports
                            .analysts
                            .find_by_login(&email)
                            .await?
                            .filter(|a| a.email().as_str().eq_ignore_ascii_case(&email)),
                        None => None,
                    },
                }
                .filter(Analyst::is_active)
                .ok_or_else(|| fail("no active Analyst for this subject"))?;
                self.ports
                    .sso
                    .link(
                        provider.id(),
                        login.audience,
                        &claims.sub,
                        &SsoIdentity::Analyst(analyst.id()),
                        now,
                    )
                    .await?;
                let token = SessionToken::new(self.ports.tokens.token()?);
                let session = Session::start(
                    analyst.id(),
                    &token,
                    self.ports.tokens.token()?,
                    self.session_policy,
                    now,
                    ip_address.map(|ip| ip.to_string()),
                    user_agent,
                );
                self.ports.sessions.insert(&session).await?;
                let principal = self.principal_for(&analyst).await?;
                tracing::info!(analyst_id = %analyst.id(), provider = %provider.id(), "signed in with SSO");
                Ok(SsoSignedIn::Analyst(
                    Box::new(SignedIn {
                        session,
                        token: Some(token),
                        trusted_device_token: None,
                        // The identity provider is responsible for the second factor.
                        totp_enrollment_required: false,
                        analyst,
                        principal,
                    }),
                    login.return_to,
                ))
            }
        }
    }
}
