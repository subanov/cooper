//! Environment-variable configuration.
//!
//! Configuration is read once at start-up (AGENTS.md §1, 12-factor). Each binary builds
//! its own config struct from these helpers; invalid values stop start-up with a clear
//! message instead of falling back silently.

use std::fmt;
use std::str::FromStr;

use thiserror::Error;

/// A configuration value is missing or invalid.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ConfigError {
    /// A required variable is not set.
    #[error("environment variable {0} is required")]
    Missing(&'static str),
    /// A variable is set but cannot be parsed.
    #[error("environment variable {name} is invalid: {reason}")]
    Invalid {
        /// Variable name.
        name: &'static str,
        /// Why the value was rejected. Never contains the value of a secret.
        reason: String,
    },
}

/// Reads a variable, treating an empty value as unset.
///
/// # Errors
///
/// Fails if the value is not valid Unicode.
pub fn var(name: &'static str) -> Result<Option<String>, ConfigError> {
    match std::env::var(name) {
        Ok(value) if value.trim().is_empty() => Ok(None),
        Ok(value) => Ok(Some(value)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => Err(ConfigError::Invalid {
            name,
            reason: "not valid UTF-8".to_owned(),
        }),
    }
}

/// Reads a required variable.
///
/// # Errors
///
/// Fails if the variable is unset or empty.
pub fn required(name: &'static str) -> Result<String, ConfigError> {
    var(name)?.ok_or(ConfigError::Missing(name))
}

/// Parses a variable, or returns `default` when it is unset.
///
/// # Errors
///
/// Fails if the value cannot be parsed as `T`.
pub fn parse_or<T>(name: &'static str, default: T) -> Result<T, ConfigError>
where
    T: FromStr,
    T::Err: fmt::Display,
{
    match var(name)? {
        None => Ok(default),
        Some(raw) => raw
            .trim()
            .parse()
            .map_err(|e: T::Err| ConfigError::Invalid {
                name,
                reason: e.to_string(),
            }),
    }
}

/// A secret string whose `Debug` output is redacted.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    /// Wraps a secret value.
    #[must_use]
    pub const fn new(value: String) -> Self {
        Self(value)
    }

    /// The secret value. Do not log it.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret([redacted])")
    }
}

/// Database connection settings.
#[derive(Debug, Clone)]
pub struct DatabaseConfig {
    /// Connection URL. Contains the password, so it is a [`Secret`].
    pub url: Secret,
    /// Maximum pool size.
    pub max_connections: u32,
}

impl DatabaseConfig {
    /// Reads `COOPER_DATABASE_URL` (falling back to `DATABASE_URL`) and
    /// `COOPER_DATABASE_MAX_CONNECTIONS` (default 10).
    ///
    /// # Errors
    ///
    /// Fails if no URL is set or the pool size is not a positive number.
    pub fn from_env() -> Result<Self, ConfigError> {
        let url = match var("COOPER_DATABASE_URL")? {
            Some(url) => url,
            None => required("DATABASE_URL")?,
        };
        let max_connections = parse_or("COOPER_DATABASE_MAX_CONNECTIONS", 10_u32)?;
        if max_connections == 0 {
            return Err(ConfigError::Invalid {
                name: "COOPER_DATABASE_MAX_CONNECTIONS",
                reason: "must be at least 1".to_owned(),
            });
        }
        Ok(Self {
            url: Secret::new(url),
            max_connections,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_debug_is_redacted() {
        let secret = Secret::new("postgres://u:hunter2@db/cooper".to_owned());
        assert!(!format!("{secret:?}").contains("hunter2"));
        assert_eq!(secret.expose(), "postgres://u:hunter2@db/cooper");
    }

    #[test]
    fn invalid_error_names_the_variable() {
        let err = ConfigError::Invalid {
            name: "COOPER_X",
            reason: "bad".to_owned(),
        };
        assert_eq!(
            err.to_string(),
            "environment variable COOPER_X is invalid: bad"
        );
    }
}
