//! Encryption of secrets at rest (FR-EML-012, ADR-0013).
//!
//! TOTP secrets, mailbox passwords, OAuth tokens and similar values are encrypted with
//! AES-256-GCM before they are stored. Keys are **versioned**: a [`Keyring`] holds one
//! active key used for new values and any number of older keys still accepted for
//! decryption, so keys can be rotated without downtime. Stored values look like
//! `v<key id>.<nonce>.<ciphertext>` (base64url), so a re-encryption job can find values
//! written with an old key.
//!
//! Every value is bound to a **purpose** (associated data such as
//! `analyst.totp:<id>`), so a ciphertext copied into another row or column does not
//! decrypt.

use std::collections::BTreeMap;
use std::fmt;

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use thiserror::Error;

/// Encryption failed or a stored value could not be decrypted.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CryptoError {
    /// `COOPER_ENCRYPTION_KEYS` is missing or malformed.
    #[error("invalid encryption keys: {0}")]
    InvalidKeys(String),
    /// The value was written with a key the keyring does not have.
    #[error("unknown encryption key v{0}")]
    UnknownKey(u16),
    /// The value is not in `v<id>.<nonce>.<ciphertext>` form.
    #[error("malformed encrypted value")]
    Malformed,
    /// Authentication failed: wrong key, wrong purpose or tampered data.
    #[error("encrypted value failed authentication")]
    Authentication,
    /// The operating system has no randomness available.
    #[error("no randomness available")]
    Randomness,
}

/// Versioned AES-256-GCM keys. The first key given is active.
#[derive(Clone)]
pub struct Keyring {
    active: u16,
    keys: BTreeMap<u16, Aes256Gcm>,
}

impl fmt::Debug for Keyring {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Keyring")
            .field("active", &self.active)
            .field("key_ids", &self.keys.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl Keyring {
    /// Parses `COOPER_ENCRYPTION_KEYS`: comma-separated `<id>:<base64 of 32 bytes>`,
    /// active key first, for example `2:…,1:…`. Generate a key with
    /// `openssl rand -base64 32`.
    ///
    /// # Errors
    ///
    /// [`CryptoError::InvalidKeys`] when empty, malformed, duplicated or not 32 bytes.
    pub fn parse(spec: &str) -> Result<Self, CryptoError> {
        let mut active = None;
        let mut keys = BTreeMap::new();
        for entry in spec.split(',').map(str::trim).filter(|e| !e.is_empty()) {
            let (id, key) = entry
                .split_once(':')
                .ok_or_else(|| CryptoError::InvalidKeys("expected <id>:<base64 key>".into()))?;
            let id: u16 = id
                .trim()
                .parse()
                .map_err(|_| CryptoError::InvalidKeys(format!("key id {id:?} is not a number")))?;
            let bytes = STANDARD
                .decode(key.trim())
                .map_err(|_| CryptoError::InvalidKeys(format!("key v{id} is not base64")))?;
            let cipher = Aes256Gcm::new_from_slice(&bytes)
                .map_err(|_| CryptoError::InvalidKeys(format!("key v{id} must be 32 bytes")))?;
            if keys.insert(id, cipher).is_some() {
                return Err(CryptoError::InvalidKeys(format!("key v{id} appears twice")));
            }
            active.get_or_insert(id);
        }
        let active = active.ok_or_else(|| CryptoError::InvalidKeys("no key given".into()))?;
        Ok(Self { active, keys })
    }

    /// A keyring with one random key, for tests and local experiments only.
    ///
    /// # Errors
    ///
    /// [`CryptoError::Randomness`].
    pub fn ephemeral() -> Result<Self, CryptoError> {
        let mut key = [0u8; 32];
        getrandom::fill(&mut key).map_err(|_| CryptoError::Randomness)?;
        Self::parse(&format!("1:{}", STANDARD.encode(key)))
    }

    /// ID of the key used for new values.
    #[must_use]
    pub const fn active_key_id(&self) -> u16 {
        self.active
    }

    /// Encrypts `plaintext` for `purpose` with the active key.
    ///
    /// # Errors
    ///
    /// [`CryptoError::Randomness`] if no nonce can be generated.
    pub fn encrypt(&self, purpose: &str, plaintext: &[u8]) -> Result<String, CryptoError> {
        let cipher = self
            .keys
            .get(&self.active)
            .ok_or(CryptoError::UnknownKey(self.active))?;
        let mut nonce_bytes = [0u8; 12];
        getrandom::fill(&mut nonce_bytes).map_err(|_| CryptoError::Randomness)?;
        let nonce = Nonce::try_from(&nonce_bytes[..]).map_err(|_| CryptoError::Malformed)?;
        let ciphertext = cipher
            .encrypt(
                &nonce,
                Payload {
                    msg: plaintext,
                    aad: purpose.as_bytes(),
                },
            )
            .map_err(|_| CryptoError::Authentication)?;
        Ok(format!(
            "v{}.{}.{}",
            self.active,
            URL_SAFE_NO_PAD.encode(nonce_bytes),
            URL_SAFE_NO_PAD.encode(ciphertext)
        ))
    }

    /// Decrypts a value written by [`Keyring::encrypt`] for the same `purpose`.
    ///
    /// # Errors
    ///
    /// [`CryptoError::Malformed`], [`CryptoError::UnknownKey`] or
    /// [`CryptoError::Authentication`].
    pub fn decrypt(&self, purpose: &str, stored: &str) -> Result<Vec<u8>, CryptoError> {
        let (id, nonce, ciphertext) = split(stored)?;
        let cipher = self.keys.get(&id).ok_or(CryptoError::UnknownKey(id))?;
        let nonce = Nonce::try_from(&nonce[..]).map_err(|_| CryptoError::Malformed)?;
        cipher
            .decrypt(
                &nonce,
                Payload {
                    msg: &ciphertext,
                    aad: purpose.as_bytes(),
                },
            )
            .map_err(|_| CryptoError::Authentication)
    }

    /// Whether `stored` was written with a key other than the active one and should be
    /// re-encrypted.
    #[must_use]
    pub fn needs_reencryption(&self, stored: &str) -> bool {
        key_id(stored).is_some_and(|id| id != self.active)
    }
}

/// The key ID of a stored value, if it is well formed.
#[must_use]
pub fn key_id(stored: &str) -> Option<u16> {
    stored.strip_prefix('v')?.split('.').next()?.parse().ok()
}

fn split(stored: &str) -> Result<(u16, Vec<u8>, Vec<u8>), CryptoError> {
    let mut parts = stored
        .strip_prefix('v')
        .ok_or(CryptoError::Malformed)?
        .split('.');
    let (Some(id), Some(nonce), Some(ct), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(CryptoError::Malformed);
    };
    let id = id.parse().map_err(|_| CryptoError::Malformed)?;
    let nonce = URL_SAFE_NO_PAD
        .decode(nonce)
        .map_err(|_| CryptoError::Malformed)?;
    let ct = URL_SAFE_NO_PAD
        .decode(ct)
        .map_err(|_| CryptoError::Malformed)?;
    Ok((id, nonce, ct))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(byte: u8) -> String {
        STANDARD.encode([byte; 32])
    }

    #[test]
    fn round_trip_is_bound_to_purpose() {
        let ring = Keyring::parse(&format!("1:{}", key(1))).unwrap();
        let stored = ring.encrypt("analyst.totp:a", b"secret").unwrap();
        assert!(stored.starts_with("v1."));
        assert_eq!(ring.decrypt("analyst.totp:a", &stored).unwrap(), b"secret");
        assert_eq!(
            ring.decrypt("analyst.totp:b", &stored),
            Err(CryptoError::Authentication)
        );
        assert_ne!(
            stored,
            ring.encrypt("analyst.totp:a", b"secret").unwrap(),
            "fresh nonce each time"
        );
        assert!(!format!("{ring:?}").contains(&key(1)));
    }

    #[test]
    fn rotation_keeps_old_values_readable() {
        let old = Keyring::parse(&format!("1:{}", key(1))).unwrap();
        let stored = old.encrypt("p", b"x").unwrap();
        let rotated = Keyring::parse(&format!("2:{}, 1:{}", key(2), key(1))).unwrap();
        assert_eq!(rotated.active_key_id(), 2);
        assert_eq!(rotated.decrypt("p", &stored).unwrap(), b"x");
        assert!(rotated.needs_reencryption(&stored));
        assert!(!rotated.needs_reencryption(&rotated.encrypt("p", b"x").unwrap()));
        let only_new = Keyring::parse(&format!("2:{}", key(2))).unwrap();
        assert_eq!(
            only_new.decrypt("p", &stored),
            Err(CryptoError::UnknownKey(1))
        );
    }

    #[test]
    fn rejects_bad_key_specs_and_values() {
        for spec in [
            "",
            "1",
            "x:abc",
            &format!("1:{}", STANDARD.encode([0u8; 16])),
            &format!("1:{0},1:{0}", key(1)),
        ] {
            assert!(Keyring::parse(spec).is_err(), "{spec:?}");
        }
        let ring = Keyring::ephemeral().unwrap();
        for bad in ["", "v1", "v1.a", "1.a.b", "vX.a.b", "v1.a.b.c"] {
            assert!(ring.decrypt("p", bad).is_err(), "{bad:?}");
        }
    }
}
