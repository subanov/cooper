//! Two-factor enrollment and reset (FR-IAM-003).

use cooper_kernel::{Actor, AnalystId, AuditAction, Principal};

use super::{AppError, IdentityService, capabilities, require};
use crate::domain::{Analyst, IdentityError, Revocations, TotpSecret, audit_record};

/// A started enrollment: what the Analyst scans or types into the authenticator app.
#[derive(Debug, Clone)]
pub struct TotpEnrollment {
    /// The secret in base32, for manual entry.
    pub secret_base32: String,
    /// `otpauth://` URI, for a QR code.
    pub provisioning_uri: String,
}

/// Issuer shown in authenticator apps.
const ISSUER: &str = "Cooper";

impl IdentityService {
    async fn me(&self, principal: &Principal) -> Result<Analyst, AppError> {
        let Actor::Analyst(id) = principal.actor() else {
            return Err(AppError::Unauthenticated);
        };
        self.ports
            .analysts
            .get(id)
            .await?
            .ok_or(AppError::Unauthenticated)
    }

    async fn save_self(
        &self,
        before: &Analyst,
        after: &Analyst,
        revoke: Revocations,
    ) -> Result<(), AppError> {
        let audit = audit_record(
            Some(before),
            after,
            AuditAction::Updated,
            Actor::Analyst(after.id()),
            self.ports.clock.now(),
        );
        self.ports.analysts.update(after, &audit, revoke).await?;
        Ok(())
    }

    /// Starts two-factor enrollment for the signed-in Analyst with a new secret.
    ///
    /// # Errors
    ///
    /// [`IdentityError::InvalidState`] when two-factor sign-in is already on.
    pub async fn begin_totp_enrollment(
        &self,
        principal: &Principal,
    ) -> Result<TotpEnrollment, AppError> {
        let mut me = self.me(principal).await?;
        if me.totp_enabled() {
            return Err(IdentityError::InvalidState("two-factor sign-in is already on").into());
        }
        let bytes = hex_bytes(&self.ports.tokens.token()?);
        let secret = TotpSecret::from_bytes(bytes.into_iter().take(20).collect());
        let enrollment = TotpEnrollment {
            secret_base32: secret.to_base32(),
            provisioning_uri: secret.provisioning_uri(ISSUER, me.username().as_str()),
        };
        let before = me.clone();
        me.begin_totp_enrollment(secret, self.ports.clock.now());
        self.save_self(&before, &me, Revocations::NONE).await?;
        Ok(enrollment)
    }

    /// Confirms enrollment with a code; two-factor sign-in is on afterwards.
    ///
    /// # Errors
    ///
    /// No enrollment started, or a wrong code.
    pub async fn confirm_totp_enrollment(
        &self,
        principal: &Principal,
        code: &str,
    ) -> Result<(), AppError> {
        let mut me = self.me(principal).await?;
        let before = me.clone();
        me.confirm_totp(code, self.ports.clock.now())?;
        self.save_self(&before, &me, Revocations::NONE).await?;
        tracing::info!(analyst_id = %me.id(), "two-factor sign-in turned on");
        Ok(())
    }

    /// Turns the signed-in Analyst's two-factor sign-in off, after checking a current
    /// code. Forgets their trusted devices.
    ///
    /// # Errors
    ///
    /// Required by policy, not on, or a wrong code.
    pub async fn disable_own_totp(
        &self,
        principal: &Principal,
        code: &str,
    ) -> Result<(), AppError> {
        if self.ports.policy.policy().await.require_totp {
            return Err(IdentityError::TotpRequiredByPolicy.into());
        }
        let mut me = self.me(principal).await?;
        if !me.totp_enabled() {
            return Err(IdentityError::InvalidState("two-factor sign-in is not on").into());
        }
        let before = me.clone();
        let now = self.ports.clock.now();
        if !me.verify_totp(code, now) {
            return Err(IdentityError::InvalidTotpCode.into());
        }
        me.disable_totp(now);
        self.save_self(
            &before,
            &me,
            Revocations {
                sessions: false,
                trusted_devices: true,
            },
        )
        .await
    }

    /// Resets another Analyst's two-factor sign-in, for a lost phone. Ends their
    /// sessions and forgets their trusted devices. Administrators only.
    ///
    /// # Errors
    ///
    /// Forbidden or not found.
    pub async fn reset_analyst_totp(
        &self,
        principal: &Principal,
        id: AnalystId,
    ) -> Result<Analyst, AppError> {
        require(principal, capabilities::ANALYSTS_MANAGE)?;
        principal.require_admin()?;
        let mut analyst = self.get_analyst(principal, id).await?;
        let before = analyst.clone();
        let now = self.ports.clock.now();
        analyst.disable_totp(now);
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
        tracing::info!(analyst_id = %id, "two-factor sign-in reset by an Administrator");
        Ok(analyst)
    }
}

/// Decodes the hex token from the generator into bytes.
fn hex_bytes(hex_token: &str) -> Vec<u8> {
    (0..hex_token.len() / 2)
        .filter_map(|i| u8::from_str_radix(hex_token.get(2 * i..2 * i + 2)?, 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::SignIn;
    use crate::application::fakes::{Fakes, setup_input};
    use crate::domain::STEP_SECONDS;
    use chrono::Duration;
    use cooper_kernel::Clock as _;

    fn code(secret_base32: &str, at: chrono::DateTime<chrono::Utc>) -> String {
        let bytes = base32_decode(secret_base32);
        TotpSecret::from_bytes(bytes).code_at_step(at.timestamp() / STEP_SECONDS)
    }

    fn base32_decode(s: &str) -> Vec<u8> {
        let alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
        let (mut buffer, mut bits, mut out) = (0u32, 0, Vec::new());
        for c in s.chars() {
            buffer = (buffer << 5) | alphabet.find(c).unwrap() as u32;
            bits += 5;
            if bits >= 8 {
                bits -= 8;
                out.push((buffer >> bits) as u8);
            }
        }
        out
    }

    fn sign_in(
        code: Option<String>,
        remember: bool,
        device: Option<crate::domain::SessionToken>,
    ) -> SignIn {
        SignIn {
            login: "admin".into(),
            password: "correct horse battery".into(),
            totp_code: code,
            remember_device: remember,
            trusted_device: device,
            ..SignIn::default()
        }
    }

    #[tokio::test]
    async fn enrolled_analyst_needs_a_fresh_code_or_trusted_device() {
        let fakes = Fakes::new();
        let svc = fakes.service();
        svc.complete_setup(setup_input()).await.unwrap();
        let me = fakes.sign_in_admin(&svc).await.principal;
        let enrollment = svc.begin_totp_enrollment(&me).await.unwrap();
        assert!(
            enrollment
                .provisioning_uri
                .starts_with("otpauth://totp/Cooper:admin?")
        );
        assert_eq!(
            svc.confirm_totp_enrollment(&me, "000000")
                .await
                .unwrap_err(),
            AppError::Rule(IdentityError::InvalidTotpCode)
        );
        let now = fakes.clock.now();
        svc.confirm_totp_enrollment(&me, &code(&enrollment.secret_base32, now))
            .await
            .unwrap();

        assert_eq!(
            svc.sign_in(sign_in(None, false, None)).await.unwrap_err(),
            AppError::TotpRequired
        );
        fakes.clock.advance(Duration::seconds(STEP_SECONDS));
        let fresh = code(&enrollment.secret_base32, fakes.clock.now());
        let signed = svc
            .sign_in(sign_in(Some(fresh.clone()), true, None))
            .await
            .unwrap();
        let device = signed.trusted_device_token.clone().unwrap();
        // The same code cannot be used twice.
        assert_eq!(
            svc.sign_in(sign_in(Some(fresh), false, None))
                .await
                .unwrap_err(),
            AppError::Rule(IdentityError::InvalidTotpCode)
        );
        // The remembered browser skips the code.
        assert!(
            svc.sign_in(sign_in(None, false, Some(device)))
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn policy_requires_enrollment_and_blocks_disabling() {
        let fakes = Fakes::new();
        let svc = fakes.service();
        svc.complete_setup(setup_input()).await.unwrap();
        fakes.set_policy(crate::domain::SecurityPolicy {
            require_totp: true,
            ..Default::default()
        });
        let signed = fakes.sign_in_admin(&svc).await;
        assert!(signed.totp_enrollment_required);
        let e = svc.begin_totp_enrollment(&signed.principal).await.unwrap();
        svc.confirm_totp_enrollment(
            &signed.principal,
            &code(&e.secret_base32, fakes.clock.now()),
        )
        .await
        .unwrap();
        fakes.clock.advance(Duration::seconds(STEP_SECONDS));
        let again = svc
            .sign_in(sign_in(
                Some(code(&e.secret_base32, fakes.clock.now())),
                false,
                None,
            ))
            .await
            .unwrap();
        assert!(!again.totp_enrollment_required);
        fakes.clock.advance(Duration::seconds(STEP_SECONDS));
        assert_eq!(
            svc.disable_own_totp(&again.principal, &code(&e.secret_base32, fakes.clock.now()))
                .await
                .unwrap_err(),
            AppError::Rule(IdentityError::TotpRequiredByPolicy)
        );
    }

    #[tokio::test]
    async fn repeated_failures_ban_the_ip() {
        let fakes = Fakes::new();
        let svc = fakes.service();
        svc.complete_setup(setup_input()).await.unwrap();
        fakes.set_policy(crate::domain::SecurityPolicy {
            max_failed_sign_ins: 3,
            ..Default::default()
        });
        let ip = Some("203.0.113.9".parse().unwrap());
        let wrong = || SignIn {
            login: "admin".into(),
            password: "wrong password!".into(),
            ip_address: ip,
            ..SignIn::default()
        };
        assert_eq!(
            svc.sign_in(wrong()).await.unwrap_err(),
            AppError::InvalidCredentials
        );
        assert_eq!(
            svc.sign_in(wrong()).await.unwrap_err(),
            AppError::InvalidCredentials
        );
        assert!(matches!(
            svc.sign_in(wrong()).await.unwrap_err(),
            AppError::TooManyAttempts { .. }
        ));
        // Even the right password is refused while banned.
        let right = SignIn {
            ip_address: ip,
            ..sign_in(None, false, None)
        };
        assert!(matches!(
            svc.sign_in(right.clone()).await.unwrap_err(),
            AppError::TooManyAttempts { .. }
        ));
        fakes.clock.advance(Duration::minutes(16));
        assert!(svc.sign_in(right).await.is_ok());
    }
}
