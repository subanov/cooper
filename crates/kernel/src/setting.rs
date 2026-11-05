//! System Setting definitions (FR-SYS-001).
//!
//! Each context declares the settings it reads as [`SettingDefinition`] constants. The
//! administration context stores values, validates changes against the definition and
//! falls back to the default when nothing is stored.

use serde_json::Value;
use thiserror::Error;

/// The type and bounds of a setting's value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingKind {
    /// `true` or `false`.
    Bool,
    /// A whole number within bounds.
    Integer {
        /// Smallest allowed value.
        min: i64,
        /// Largest allowed value.
        max: i64,
    },
    /// Text up to `max_chars` characters.
    Text {
        /// Longest allowed value.
        max_chars: usize,
    },
    /// One of a fixed list of strings.
    Choice(&'static [&'static str]),
}

/// A setting a context reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettingDefinition {
    /// Dotted key, for example `security.require_totp`.
    pub key: &'static str,
    /// Explanation shown to Administrators.
    pub description: &'static str,
    /// Type and bounds.
    pub kind: SettingKind,
    /// Default as JSON text, used when nothing is stored, for example `"false"`.
    pub default: &'static str,
}

/// A value does not fit its definition.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{key}: {reason}")]
pub struct InvalidSetting {
    /// The setting.
    pub key: &'static str,
    /// Why the value is rejected.
    pub reason: String,
}

impl SettingDefinition {
    /// The default value.
    #[must_use]
    pub fn default_value(&self) -> Value {
        serde_json::from_str(self.default).unwrap_or(Value::Null)
    }

    /// Checks `value` against the kind and returns it normalised (text trimmed).
    ///
    /// # Errors
    ///
    /// [`InvalidSetting`] when the value has the wrong type or is out of bounds.
    pub fn validate(&self, value: &Value) -> Result<Value, InvalidSetting> {
        let fail = |reason: String| InvalidSetting {
            key: self.key,
            reason,
        };
        match (self.kind, value) {
            (SettingKind::Bool, Value::Bool(_)) => Ok(value.clone()),
            (SettingKind::Integer { min, max }, Value::Number(n)) => match n.as_i64() {
                Some(i) if (min..=max).contains(&i) => Ok(value.clone()),
                _ => Err(fail(format!("must be a whole number from {min} to {max}"))),
            },
            (SettingKind::Text { max_chars }, Value::String(s)) => {
                let s = s.trim();
                if s.chars().count() > max_chars || s.chars().any(char::is_control) {
                    Err(fail(format!(
                        "must be at most {max_chars} characters without control characters"
                    )))
                } else {
                    Ok(Value::String(s.to_owned()))
                }
            }
            (SettingKind::Choice(choices), Value::String(s)) if choices.contains(&s.as_str()) => {
                Ok(value.clone())
            }
            (SettingKind::Choice(choices), _) => {
                Err(fail(format!("must be one of {}", choices.join(", "))))
            }
            (SettingKind::Bool, _) => Err(fail("must be true or false".into())),
            (SettingKind::Integer { .. }, _) => Err(fail("must be a number".into())),
            (SettingKind::Text { .. }, _) => Err(fail("must be text".into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const FLAG: SettingDefinition = SettingDefinition {
        key: "x.flag",
        description: "",
        kind: SettingKind::Bool,
        default: "false",
    };

    #[test]
    fn validates_by_kind() {
        assert_eq!(FLAG.validate(&json!(true)), Ok(json!(true)));
        assert!(FLAG.validate(&json!("yes")).is_err());
        assert_eq!(FLAG.default_value(), json!(false));

        let n = SettingDefinition {
            kind: SettingKind::Integer { min: 1, max: 10 },
            default: "5",
            ..FLAG
        };
        assert!(n.validate(&json!(10)).is_ok());
        assert!(n.validate(&json!(11)).is_err());
        assert!(n.validate(&json!(1.5)).is_err());

        let t = SettingDefinition {
            kind: SettingKind::Text { max_chars: 5 },
            default: "\"\"",
            ..FLAG
        };
        assert_eq!(t.validate(&json!("  abc ")), Ok(json!("abc")));
        assert!(t.validate(&json!("abcdef")).is_err());

        let c = SettingDefinition {
            kind: SettingKind::Choice(&["a", "b"]),
            default: "\"a\"",
            ..FLAG
        };
        assert!(c.validate(&json!("b")).is_ok());
        assert!(c.validate(&json!("c")).is_err());
    }
}
