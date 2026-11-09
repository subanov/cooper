//! Mailbox Consent (FR-EML-001, FR-EML-012, ADR-0033): an Administrator connects a
//! Microsoft `delegated` or Google Mailbox by signing in to it at the provider.

use cooper_kernel::{Actor, AnalystId, AuditAction, Principal};
use uuid::Uuid;

use super::{AppError, ServiceDesk, capabilities, require};
use crate::domain::{
    CONSENT_CALLBACK_PATH, MailboxConsent, OAuthCredentials, TicketError, audit_record,
    authorization_url, pkce_challenge, state_hash,
};

fn analyst(principal: &Principal) -> Result<AnalystId, AppError> {
    match principal.actor() {
        Actor::Analyst(a) => Ok(a),
        _ => Err(cooper_kernel::Forbidden("an Analyst session".into()).into()),
    }
}

fn refused(reason: impl Into<String>) -> AppError {
    AppError::Rule(TicketError::invalid("consent", reason))
}

impl ServiceDesk {
    async fn consent_redirect_uri(&self) -> String {
        format!(
            "{}{CONSENT_CALLBACK_PATH}",
            self.ports
                .settings
                .settings()
                .await
                .public_url
                .trim_end_matches('/')
        )
    }

    /// Starts connecting a Mailbox: stores a single-use `state` (hashed) and the PKCE
    /// verifier, and returns the provider URL to send the Administrator's browser to.
    ///
    /// # Errors
    ///
    /// Forbidden (Mailbox administration, in an Analyst session); not found; a Mailbox
    /// that is not connected by consent, or without client credentials.
    pub async fn start_mailbox_consent(
        &self,
        principal: &Principal,
        id: Uuid,
    ) -> Result<String, AppError> {
        require(principal, capabilities::CONFIGURE)?;
        let me = analyst(principal)?;
        let mailbox = self.mailbox(id).await?;
        let secret = self.ports.mailboxes.secret(id).await?.ok_or_else(|| {
            TicketError::invalid("credentials", "save client_id and client_secret first")
        })?;
        let creds = OAuthCredentials::parse(mailbox.settings(), &secret)?;
        let token = || {
            self.ports.consent.random_token().map_err(|e| {
                tracing::error!(error = %e, "no randomness for Mailbox Consent");
                AppError::Internal
            })
        };
        let (state, verifier) = (token()?, token()?);
        let consent =
            MailboxConsent::begin(&mailbox, me, &state, verifier, self.ports.clock.now())?;
        self.ports.mailboxes.insert_consent(&consent).await?;
        Ok(authorization_url(
            self.ports.consent.endpoints(),
            &mailbox,
            &creds,
            &self.consent_redirect_uri().await,
            &state,
            &pkce_challenge(&consent.pkce_verifier),
        ))
    }

    /// Finishes connecting a Mailbox when the provider sends the browser back: checks
    /// the state (once, by the Administrator who started, within ten minutes), exchanges
    /// the code with the PKCE verifier and stores the refresh token in the Mailbox's
    /// encrypted credentials (audited). Returns the Mailbox.
    ///
    /// # Errors
    ///
    /// Forbidden; an unknown, used, expired or foreign state; the provider refused.
    pub async fn finish_mailbox_consent(
        &self,
        principal: &Principal,
        state: &str,
        code: Option<&str>,
        provider_error: Option<&str>,
    ) -> Result<Uuid, AppError> {
        require(principal, capabilities::CONFIGURE)?;
        let me = analyst(principal)?;
        let now = self.ports.clock.now();
        let consent = self
            .ports
            .mailboxes
            .take_consent(&state_hash(state))
            .await?
            .filter(|c| c.admits(me, now))
            .ok_or_else(|| refused("the request is unknown, used or expired; start again"))?;
        if let Some(error) = provider_error {
            let error: String = error.chars().take(100).collect();
            return Err(refused(format!("the provider refused: {error}")));
        }
        let code = code
            .filter(|c| !c.is_empty())
            .ok_or_else(|| refused("the provider sent no code"))?;
        let mailbox = self.mailbox(consent.mailbox_id).await?;
        let secret = self
            .ports
            .mailboxes
            .secret(mailbox.id())
            .await?
            .ok_or_else(|| refused("the Mailbox has no client credentials"))?;
        let mut creds = OAuthCredentials::parse(mailbox.settings(), &secret)?;
        let refresh = self
            .ports
            .consent
            .exchange_code(
                &mailbox,
                &creds,
                &self.consent_redirect_uri().await,
                code,
                &consent.pkce_verifier,
            )
            .await
            .map_err(|e| {
                tracing::warn!(mailbox = %mailbox.id(), error = %e, "Mailbox Consent failed");
                refused(e)
            })?;
        creds.refresh_token = Some(refresh);
        let audit = audit_record(
            Some(&mailbox),
            &mailbox,
            AuditAction::Updated,
            principal.actor(),
            now,
        )
        .change(
            "secret",
            Some(&"(unchanged)"),
            Some(&"(account connected by consent)"),
        );
        self.ports
            .mailboxes
            .save(&mailbox, Some(&creds.to_secret()), &audit)
            .await?;
        Ok(mailbox.id())
    }
}
