//! Argon2id password hashing and OS-random tokens.

use argon2::Argon2;
use argon2::password_hash::phc::PasswordHash as Phc;
use argon2::password_hash::{PasswordHasher as _, PasswordVerifier as _};
use async_trait::async_trait;

use crate::domain::{Password, PasswordHash, PasswordHasher, RepoError, TokenGenerator};

/// Argon2id with the crate's recommended parameters. Hashing runs on a blocking thread
/// because it is deliberately slow (AGENTS.md §4).
#[derive(Clone)]
pub struct Argon2Hasher {
    /// Verified when an account does not exist, so the response time does not reveal it.
    dummy: String,
}

fn hash_blocking(password: &[u8]) -> Result<String, RepoError> {
    let hash: Phc = Argon2::default()
        .hash_password(password)
        .map_err(|e| RepoError::Storage(format!("password hashing failed: {e}")))?;
    Ok(hash.to_string())
}

fn verify_blocking(candidate: &[u8], phc: &str) -> bool {
    Phc::new(phc).is_ok_and(|parsed| {
        Argon2::default()
            .verify_password(candidate, &parsed)
            .is_ok()
    })
}

impl Argon2Hasher {
    /// Creates the hasher and its dummy hash.
    ///
    /// # Errors
    ///
    /// Fails if randomness is unavailable.
    pub fn new() -> Result<Self, RepoError> {
        let secret = OsTokenGenerator.token()?;
        Ok(Self {
            dummy: hash_blocking(secret.as_bytes())?,
        })
    }
}

#[async_trait]
impl PasswordHasher for Argon2Hasher {
    async fn hash(&self, password: &Password) -> Result<PasswordHash, RepoError> {
        let plain = password.expose().to_owned();
        tokio::task::spawn_blocking(move || hash_blocking(plain.as_bytes()))
            .await
            .map_err(|e| RepoError::Storage(e.to_string()))?
            .map(PasswordHash::from_phc)
    }

    async fn verify(&self, candidate: &str, hash: Option<&PasswordHash>) -> bool {
        let known = hash.is_some();
        let phc = hash.map_or_else(|| self.dummy.clone(), |h| h.as_str().to_owned());
        let candidate = candidate.to_owned();
        let ok = tokio::task::spawn_blocking(move || verify_blocking(candidate.as_bytes(), &phc))
            .await
            .unwrap_or(false);
        known && ok
    }
}

/// Tokens from the operating system's secure random generator.
#[derive(Debug, Clone, Copy, Default)]
pub struct OsTokenGenerator;

impl TokenGenerator for OsTokenGenerator {
    fn token(&self) -> Result<String, RepoError> {
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes)
            .map_err(|e| RepoError::Storage(format!("no randomness: {e}")))?;
        Ok(hex::encode(bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn argon2id_round_trip_and_unknown_user() {
        let hasher = Argon2Hasher::new().unwrap();
        let password = Password::new("correct horse battery".into()).unwrap();
        let hash = hasher.hash(&password).await.unwrap();
        assert!(hash.as_str().starts_with("$argon2id$"));
        assert!(hasher.verify("correct horse battery", Some(&hash)).await);
        assert!(!hasher.verify("wrong horse battery", Some(&hash)).await);
        assert!(!hasher.verify("anything", None).await);
        assert!(
            !hasher
                .verify("x", Some(&PasswordHash::from_phc("garbage".into())))
                .await
        );
    }

    #[test]
    fn tokens_are_256_bit_hex_and_unique() {
        let a = OsTokenGenerator.token().unwrap();
        let b = OsTokenGenerator.token().unwrap();
        assert_eq!(a.len(), 64);
        assert_ne!(a, b);
    }
}
