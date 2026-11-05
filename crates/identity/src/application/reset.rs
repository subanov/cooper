//! Password reset by emailed one-time token (FR-IAM-004) and own Signature.

use chrono::Duration;
use cooper_kernel::{Actor, AuditAction, Principal};

use super::{AppError, IdentityService};
use crate::domain::{Password, Revocations, SessionToken, audit_record};

/// How long a reset link works.
pub const RESET_TOKEN_MINUTES: i64 = 60;

impl IdentityService {
    /// Starts a password reset for a username or email. Always succeeds from the caller's
    /// point of view, so the response does not reveal which accounts exist; only active
    /// Analysts with a password get an email.
    ///
    /// # Errors
    ///
    /// Storage failures only.
    pub async fn request_password_reset(&self, login: &str) -> Result<(), AppError> {
        let Some(analyst) = self
            .ports
            .analysts
            .find_by_login(login.trim())
            .await?
            .filter(|a| a.is_active())
        else {
            tracing::info!("password reset requested for an unknown or inactive login");
            return Ok(());
        };
        let now = self.ports.clock.now();
        let token = SessionToken::new(self.ports.tokens.token()?);
        let expires_at = now + Duration::minutes(RESET_TOKEN_MINUTES);
        self.ports
            .resets
            .issue(analyst.id(), &token.hash(), now, expires_at)
            .await?;
        self.ports
            .reset_mailer
            .send_reset(
                analyst.email().as_str(),
                analyst.full_name().as_str(),
                token.expose(),
                expires_at,
            )
            .await?;
        tracing::info!(analyst_id = %analyst.id(), "password reset email queued");
        Ok(())
    }

    /// Sets a new password with a reset token. The token works once; every session and
    /// trusted device of the Analyst ends.
    ///
    /// # Errors
    ///
    /// [`AppError::InvalidCredentials`] for an unknown, used or expired token.
    pub async fn confirm_password_reset(
        &self,
        token: &str,
        password: Password,
    ) -> Result<(), AppError> {
        let now = self.ports.clock.now();
        let id = self
            .ports
            .resets
            .consume(&SessionToken::new(token.trim().to_owned()).hash(), now)
            .await?
            .ok_or(AppError::InvalidCredentials)?;
        let mut analyst = self
            .ports
            .analysts
            .get(id)
            .await?
            .filter(|a| a.is_active())
            .ok_or(AppError::InvalidCredentials)?;
        let before = analyst.clone();
        analyst.set_password(self.ports.hasher.hash(&password).await?, now);
        let audit = audit_record(
            Some(&before),
            &analyst,
            AuditAction::Updated,
            Actor::Analyst(id),
            now,
        );
        self.ports
            .analysts
            .update(&analyst, &audit, Revocations::ALL)
            .await?;
        tracing::info!(analyst_id = %id, "password reset completed");
        Ok(())
    }

    /// Changes the signed-in Analyst's email Signature.
    ///
    /// # Errors
    ///
    /// Not an Analyst or a Signature over 2000 characters.
    pub async fn set_own_signature(
        &self,
        principal: &Principal,
        signature: &str,
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
        let before = analyst.clone();
        let now = self.ports.clock.now();
        analyst.set_signature(signature, now)?;
        let audit = audit_record(
            Some(&before),
            &analyst,
            AuditAction::Updated,
            principal.actor(),
            now,
        );
        if audit.is_meaningful() {
            self.ports
                .analysts
                .update(&analyst, &audit, Revocations::NONE)
                .await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::SignIn;
    use crate::application::fakes::{Fakes, setup_input};

    #[tokio::test]
    async fn reset_tokens_work_once_and_expire() {
        let fakes = Fakes::new();
        let svc = fakes.service();
        svc.complete_setup(setup_input()).await.unwrap();
        svc.request_password_reset("nobody").await.unwrap();
        assert!(
            fakes.state().reset_mails.is_empty(),
            "unknown logins get no email"
        );
        svc.request_password_reset("ADMIN@example.com")
            .await
            .unwrap();
        let (to, token) = fakes.state().reset_mails[0].clone();
        assert_eq!(to, "admin@example.com");

        let new = || Password::new("a brand new password".into()).unwrap();
        svc.confirm_password_reset(&token, new()).await.unwrap();
        assert_eq!(
            svc.confirm_password_reset(&token, new()).await.unwrap_err(),
            AppError::InvalidCredentials,
            "single use"
        );
        assert!(
            svc.sign_in(SignIn {
                login: "admin".into(),
                password: "a brand new password".into(),
                ..SignIn::default()
            })
            .await
            .is_ok()
        );

        svc.request_password_reset("admin").await.unwrap();
        let (_, late) = fakes.state().reset_mails[1].clone();
        fakes
            .clock
            .advance(Duration::minutes(RESET_TOKEN_MINUTES + 1));
        assert_eq!(
            svc.confirm_password_reset(&late, new()).await.unwrap_err(),
            AppError::InvalidCredentials,
            "expired"
        );
    }
}
