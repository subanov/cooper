//! Confidentiality of a ticket or Department (Vocabulary: Sensitivity).

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Confidentiality level (fixed set). Confidential items are visible only to authorised
/// Analysts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sensitivity {
    /// Visible to every Analyst with access to the Company.
    #[default]
    Normal,
    /// Visible only to authorised Analysts.
    Confidential,
}

/// A string is not a [`Sensitivity`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("unknown sensitivity {0:?}")]
pub struct ParseSensitivityError(pub String);

impl Sensitivity {
    /// Stable lower-case name used in storage and APIs.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Confidential => "confidential",
        }
    }
}

impl FromStr for Sensitivity {
    type Err = ParseSensitivityError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "normal" => Ok(Self::Normal),
            "confidential" => Ok(Self::Confidential),
            other => Err(ParseSensitivityError(other.to_owned())),
        }
    }
}

impl fmt::Display for Sensitivity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_storage_name() {
        for s in [Sensitivity::Normal, Sensitivity::Confidential] {
            assert_eq!(s.as_str().parse::<Sensitivity>(), Ok(s));
        }
        assert!("secret".parse::<Sensitivity>().is_err());
    }
}
