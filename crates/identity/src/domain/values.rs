//! Validated value objects ("parse, don't validate").

use std::fmt;

use serde::{Deserialize, Serialize};

use super::IdentityError;

macro_rules! string_value {
    ($(#[$meta:meta])* $name:ident, $parse:path) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl $name {
            /// The value as a string slice.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl TryFrom<String> for $name {
            type Error = IdentityError;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                $parse(&value).map(Self)
            }
        }

        impl TryFrom<&str> for $name {
            type Error = IdentityError;

            fn try_from(value: &str) -> Result<Self, Self::Error> {
                $parse(value).map(Self)
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

fn invalid(field: &'static str, reason: impl Into<String>) -> IdentityError {
    IdentityError::Invalid {
        field,
        reason: reason.into(),
    }
}

fn parse_name(value: &str) -> Result<String, IdentityError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(invalid("name", "must not be empty"));
    }
    if trimmed.chars().count() > 200 {
        return Err(invalid("name", "must be at most 200 characters"));
    }
    if trimmed.chars().any(char::is_control) {
        return Err(invalid("name", "must not contain control characters"));
    }
    Ok(trimmed.to_owned())
}

fn parse_email(value: &str) -> Result<String, IdentityError> {
    let trimmed = value.trim();
    let Some((local, domain)) = trimmed.rsplit_once('@') else {
        return Err(invalid("email", "must contain @"));
    };
    if local.is_empty() || trimmed.len() > 254 {
        return Err(invalid("email", "is not a valid email address"));
    }
    if trimmed.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(invalid("email", "must not contain spaces"));
    }
    let domain = parse_domain(domain).map_err(|_| invalid("email", "has an invalid domain"))?;
    Ok(format!("{local}@{domain}"))
}

fn parse_username(value: &str) -> Result<String, IdentityError> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.chars().count() > 100 {
        return Err(invalid("username", "must be 1 to 100 characters"));
    }
    if trimmed.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(invalid("username", "must not contain spaces"));
    }
    Ok(trimmed.to_owned())
}

fn parse_ticket_code(value: &str) -> Result<String, IdentityError> {
    let code = value.trim().to_ascii_uppercase();
    if code.is_empty()
        || code.len() > 10
        || !code
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
    {
        return Err(invalid(
            "ticket_code",
            "must be 1 to 10 letters or digits (A-Z, 0-9)",
        ));
    }
    Ok(code)
}

fn parse_domain(value: &str) -> Result<String, IdentityError> {
    let domain = value.trim().trim_end_matches('.').to_ascii_lowercase();
    let labels: Vec<&str> = domain.split('.').collect();
    let label_ok = |l: &&str| {
        !l.is_empty()
            && l.len() <= 63
            && !l.starts_with('-')
            && !l.ends_with('-')
            && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    };
    let tld_ok = labels
        .last()
        .is_some_and(|t| t.len() >= 2 && t.chars().all(|c| c.is_ascii_alphabetic()));
    if labels.len() < 2 || domain.len() > 253 || !labels.iter().all(label_ok) || !tld_ok {
        return Err(invalid("domain", format!("{value:?} is not a domain name")));
    }
    Ok(domain)
}

fn parse_cost_centre_code(value: &str) -> Result<String, IdentityError> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.chars().count() > 50 {
        return Err(invalid("code", "must be 1 to 50 characters"));
    }
    Ok(trimmed.to_owned())
}

string_value!(
    /// A display name: trimmed, 1 to 200 characters, no control characters.
    Name,
    parse_name
);

string_value!(
    /// An email address with a valid, lower-cased domain.
    EmailAddress,
    parse_email
);

string_value!(
    /// A sign-in name: 1 to 100 characters without spaces.
    Username,
    parse_username
);

string_value!(
    /// A Company's Ticket Code: 1 to 10 upper-case letters or digits.
    TicketCode,
    parse_ticket_code
);

string_value!(
    /// A lower-cased DNS domain name, for example `example.com`.
    DomainName,
    parse_domain
);

string_value!(
    /// A Cost Centre code: 1 to 50 characters.
    CostCentreCode,
    parse_cost_centre_code
);

impl EmailAddress {
    /// The domain part (always lower case).
    #[must_use]
    pub fn domain(&self) -> &str {
        self.0.rsplit_once('@').map_or("", |(_, d)| d)
    }
}

/// A plaintext password that satisfies the password policy: 12 to 1024 characters.
/// Never logged or serialized.
#[derive(Clone, PartialEq, Eq)]
pub struct Password(String);

impl Password {
    /// Minimum length (OWASP ASVS L2).
    pub const MIN_CHARS: usize = 12;
    /// Maximum length, to bound hashing cost.
    pub const MAX_CHARS: usize = 1024;

    /// Validates a new password against the policy.
    ///
    /// # Errors
    ///
    /// Returns [`IdentityError::Invalid`] if it is too short or too long.
    pub fn new(value: String) -> Result<Self, IdentityError> {
        let chars = value.chars().count();
        if chars < Self::MIN_CHARS {
            return Err(invalid(
                "password",
                format!("must be at least {} characters", Self::MIN_CHARS),
            ));
        }
        if chars > Self::MAX_CHARS {
            return Err(invalid(
                "password",
                format!("must be at most {} characters", Self::MAX_CHARS),
            ));
        }
        Ok(Self(value))
    }

    /// The plaintext, for hashing.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Password {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Password(<redacted>)")
    }
}

/// An Argon2id password hash in PHC string format.
#[derive(Clone, PartialEq, Eq)]
pub struct PasswordHash(String);

impl PasswordHash {
    /// Wraps a stored PHC string.
    #[must_use]
    pub const fn from_phc(phc: String) -> Self {
        Self(phc)
    }

    /// The PHC string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for PasswordHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PasswordHash(<redacted>)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn email_lowercases_domain_only() {
        let email = EmailAddress::try_from(" Jane.Doe@Example.COM ").unwrap();
        assert_eq!(email.as_str(), "Jane.Doe@example.com");
        assert_eq!(email.domain(), "example.com");
        for bad in [
            "",
            "jane",
            "@example.com",
            "jane@",
            "jane@example",
            "ja ne@example.com",
        ] {
            assert!(EmailAddress::try_from(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn ticket_code_is_upper_case_alphanumeric() {
        assert_eq!(TicketCode::try_from("acme1").unwrap().as_str(), "ACME1");
        for bad in ["", "AC-ME", "ABCDEFGHIJK", "Ä"] {
            assert!(TicketCode::try_from(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn domain_validation() {
        assert_eq!(
            DomainName::try_from("Example.CO.uk.").unwrap().as_str(),
            "example.co.uk"
        );
        for bad in [
            "localhost",
            "-a.com",
            "a..com",
            "a.c",
            "exa mple.com",
            "a.123",
        ] {
            assert!(DomainName::try_from(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn name_is_trimmed_and_bounded() {
        assert_eq!(
            Name::try_from("  Service Desk ").unwrap().as_str(),
            "Service Desk"
        );
        assert!(Name::try_from("   ").is_err());
        assert!(Name::try_from("a".repeat(201)).is_err());
        assert!(Name::try_from("bad\nname").is_err());
    }

    #[test]
    fn password_policy_and_redaction() {
        assert!(Password::new("short".into()).is_err());
        let p = Password::new("correct horse battery".into()).unwrap();
        assert_eq!(format!("{p:?}"), "Password(<redacted>)");
        assert!(Password::new("x".repeat(1025)).is_err());
    }

    #[test]
    fn serde_rejects_invalid_values() {
        assert!(serde_json::from_str::<Username>("\"jane doe\"").is_err());
        assert_eq!(
            serde_json::from_str::<Username>("\"jdoe\"")
                .unwrap()
                .as_str(),
            "jdoe"
        );
    }

    proptest! {
        #[test]
        fn valid_ticket_codes_round_trip(code in "[A-Z0-9]{1,10}") {
            let parsed = TicketCode::try_from(code.as_str()).unwrap();
            prop_assert_eq!(parsed.as_str(), code.as_str());
        }

        #[test]
        fn email_parsing_never_panics(s in "\\PC{0,80}") {
            let _ = EmailAddress::try_from(s.as_str());
        }
    }
}
