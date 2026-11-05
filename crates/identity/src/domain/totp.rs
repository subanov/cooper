//! Time-based one-time passwords (RFC 6238) for two-factor sign-in (FR-IAM-003).
//!
//! Six digits, 30-second steps, HMAC-SHA1: what every authenticator app supports. One
//! step of clock skew either way is accepted, and a code is never accepted twice: the
//! last used step is remembered (replay protection).

use std::fmt;

use hmac::{Hmac, KeyInit, Mac};
use sha1::Sha1;

/// Seconds per step.
pub const STEP_SECONDS: i64 = 30;
/// Digits per code.
const DIGITS: u32 = 6;

/// A TOTP shared secret (160 bits). Never logged.
#[derive(Clone, PartialEq, Eq)]
pub struct TotpSecret(Vec<u8>);

impl fmt::Debug for TotpSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TotpSecret(<redacted>)")
    }
}

impl TotpSecret {
    /// Wraps secret bytes (from the random generator or decrypted storage).
    #[must_use]
    pub const fn from_bytes(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    /// The raw secret, for encryption at rest.
    #[must_use]
    pub fn expose(&self) -> &[u8] {
        &self.0
    }

    /// RFC 4648 base32 without padding, as typed into authenticator apps.
    #[must_use]
    pub fn to_base32(&self) -> String {
        const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
        let mut out = String::new();
        let mut buffer: u32 = 0;
        let mut bits = 0;
        for byte in &self.0 {
            buffer = (buffer << 8) | u32::from(*byte);
            bits += 8;
            while bits >= 5 {
                bits -= 5;
                out.push(char::from(ALPHABET[((buffer >> bits) & 31) as usize]));
            }
        }
        if bits > 0 {
            out.push(char::from(ALPHABET[((buffer << (5 - bits)) & 31) as usize]));
        }
        out
    }

    /// Parses RFC 4648 base32 (case-insensitive, padding and spaces ignored).
    #[must_use]
    pub fn from_base32(text: &str) -> Option<Self> {
        const ALPHABET: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
        let mut out = Vec::new();
        let mut buffer: u32 = 0;
        let mut bits = 0;
        for c in text.chars().filter(|c| !c.is_whitespace() && *c != '=') {
            let value = ALPHABET.find(c.to_ascii_uppercase())?;
            buffer = (buffer << 5) | u32::try_from(value).ok()?;
            bits += 5;
            if bits >= 8 {
                bits -= 8;
                out.push(u8::try_from((buffer >> bits) & 0xff).ok()?);
            }
        }
        Some(Self(out))
    }

    /// `otpauth://` URI for QR codes.
    #[must_use]
    pub fn provisioning_uri(&self, issuer: &str, account: &str) -> String {
        let enc = |s: &str| {
            s.bytes()
                .map(|b| match b {
                    b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'@' => {
                        char::from(b).to_string()
                    }
                    _ => format!("%{b:02X}"),
                })
                .collect::<String>()
        };
        format!(
            "otpauth://totp/{}:{}?secret={}&issuer={}&algorithm=SHA1&digits=6&period=30",
            enc(issuer),
            enc(account),
            self.to_base32(),
            enc(issuer)
        )
    }

    /// The code for time step `step`.
    #[must_use]
    pub fn code_at_step(&self, step: i64) -> String {
        let Ok(mut mac) = <Hmac<Sha1> as KeyInit>::new_from_slice(&self.0) else {
            return String::new();
        };
        mac.update(&step.to_be_bytes());
        let hash = mac.finalize().into_bytes();
        let offset = usize::from(hash[hash.len() - 1] & 0x0f);
        let binary = (u32::from(hash[offset] & 0x7f) << 24)
            | (u32::from(hash[offset + 1]) << 16)
            | (u32::from(hash[offset + 2]) << 8)
            | u32::from(hash[offset + 3]);
        format!(
            "{:0width$}",
            binary % 10u32.pow(DIGITS),
            width = DIGITS as usize
        )
    }

    /// Checks `code` at Unix time `now_secs`, allowing one step of skew. Returns the
    /// matching step if it is later than `last_used_step` (codes are single-use).
    #[must_use]
    pub fn verify(&self, code: &str, now_secs: i64, last_used_step: Option<i64>) -> Option<i64> {
        let code: String = code.chars().filter(|c| !c.is_whitespace()).collect();
        if code.len() != DIGITS as usize || !code.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        let current = now_secs.div_euclid(STEP_SECONDS);
        [current - 1, current, current + 1]
            .into_iter()
            .filter(|step| last_used_step.is_none_or(|last| *step > last))
            .find(|step| constant_time_eq(self.code_at_step(*step).as_bytes(), code.as_bytes()))
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 6238 Appendix B test secret (SHA1).
    fn rfc_secret() -> TotpSecret {
        TotpSecret::from_bytes(b"12345678901234567890".to_vec())
    }

    #[test]
    fn matches_rfc_6238_vectors() {
        // RFC values are 8 digits; the last 6 digits are the 6-digit code.
        for (time, expected8) in [
            (59_i64, "94287082"),
            (1_111_111_109, "07081804"),
            (1_234_567_890, "89005924"),
            (2_000_000_000, "69279037"),
        ] {
            assert_eq!(
                rfc_secret().code_at_step(time / STEP_SECONDS),
                &expected8[2..],
                "t={time}"
            );
        }
    }

    #[test]
    fn accepts_one_step_of_skew_and_never_twice() {
        let s = rfc_secret();
        let now = 1_234_567_890;
        let previous = s.code_at_step(now / STEP_SECONDS - 1);
        let step = s.verify(&previous, now, None).unwrap();
        assert_eq!(
            s.verify(&previous, now, Some(step)),
            None,
            "replay rejected"
        );
        let two_back = s.code_at_step(now / STEP_SECONDS - 2);
        assert_eq!(s.verify(&two_back, now, None), None);
        assert_eq!(s.verify("12345", now, None), None);
        assert_eq!(s.verify("abcdef", now, None), None);
        let current = s.code_at_step(now / STEP_SECONDS);
        assert!(
            s.verify(&format!("{} {}", &current[..3], &current[3..]), now, None)
                .is_some()
        );
    }

    #[test]
    fn base32_and_uri() {
        assert_eq!(
            TotpSecret::from_bytes(b"foobar".to_vec()).to_base32(),
            "MZXW6YTBOI"
        );
        assert_eq!(
            TotpSecret::from_base32("mzxw 6ytb oi").unwrap().expose(),
            b"foobar"
        );
        assert!(TotpSecret::from_base32("1!").is_none());
        let uri = rfc_secret().provisioning_uri("Acme IT", "jane@acme.example");
        assert!(uri.starts_with(
            "otpauth://totp/Acme%20IT:jane@acme.example?secret=GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ"
        ));
        assert_eq!(format!("{:?}", rfc_secret()), "TotpSecret(<redacted>)");
    }
}
