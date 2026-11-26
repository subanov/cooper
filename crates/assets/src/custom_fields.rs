//! Custom Asset Fields grouped in Field Sets (FR-AST-003, ADR-0041). Pure Rust.

use std::collections::{BTreeMap, BTreeSet};

use chrono::NaiveDate;
use thiserror::Error;
use uuid::Uuid;

/// A custom field problem.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CustomFieldError {
    /// A value failed validation.
    #[error("{field}: {reason}")]
    Invalid {
        /// The field or input.
        field: String,
        /// Why it is not valid.
        reason: String,
    },
}

fn invalid(field: impl Into<String>, reason: impl Into<String>) -> CustomFieldError {
    CustomFieldError::Invalid {
        field: field.into(),
        reason: reason.into(),
    }
}

/// The type of a custom field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CustomKind {
    /// Text.
    Text,
    /// A number.
    Number,
    /// A date (YYYY-MM-DD).
    Date,
    /// true or false.
    Boolean,
    /// One of the options.
    Choice,
}

impl CustomKind {
    /// The stored name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Number => "number",
            Self::Date => "date",
            Self::Boolean => "boolean",
            Self::Choice => "choice",
        }
    }

    /// Parses the stored or API name.
    ///
    /// # Errors
    ///
    /// Not a field type.
    pub fn parse(s: &str) -> Result<Self, CustomFieldError> {
        Ok(match s {
            "text" => Self::Text,
            "number" => Self::Number,
            "date" => Self::Date,
            "boolean" => Self::Boolean,
            "choice" => Self::Choice,
            other => return Err(invalid("kind", format!("{other:?} is not a field type"))),
        })
    }
}

/// What an Administrator enters for a custom field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldInput {
    /// Key: a lower-case letter, then lower-case letters, digits or underscores (50 at most).
    pub key: String,
    /// Label.
    pub label: String,
    /// Type.
    pub kind: CustomKind,
    /// Options of a choice field.
    pub options: Vec<String>,
    /// Must be filled in.
    pub required: bool,
    /// Must be unique across Assets of the Company.
    pub unique: bool,
    /// Can be searched in the Asset list.
    pub searchable: bool,
    /// Shown in Asset lists.
    pub show_in_list: bool,
}

/// A validated custom field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldDefinition {
    /// ID.
    pub id: Uuid,
    /// Key.
    pub key: String,
    /// Label.
    pub label: String,
    /// Type.
    pub kind: CustomKind,
    /// Options (choice fields only).
    pub options: Vec<String>,
    /// Required.
    pub required: bool,
    /// Unique per Company.
    pub unique: bool,
    /// Searchable.
    pub searchable: bool,
    /// Shown in lists.
    pub show_in_list: bool,
    /// Active (inactive fields are kept on Assets but not asked for).
    pub active: bool,
}

fn valid_key(key: &str) -> bool {
    let mut chars = key.chars();
    key.len() <= 50
        && matches!(chars.next(), Some(c) if c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

impl FieldDefinition {
    /// Validates a field.
    ///
    /// # Errors
    ///
    /// A bad key or label, options that do not fit the type, or a required and unique
    /// flag on the same field is fine; options on a non-choice field are not.
    pub fn new(id: Uuid, input: FieldInput) -> Result<Self, CustomFieldError> {
        let key = input.key.trim().to_owned();
        if !valid_key(&key) {
            return Err(invalid("key", "must be a lower-case identifier of 50 characters at most"));
        }
        let label = input.label.trim().to_owned();
        if label.is_empty() || label.chars().count() > 255 {
            return Err(invalid("label", "must be 1 to 255 characters"));
        }
        let mut options = Vec::new();
        if input.kind == CustomKind::Choice {
            let mut seen = BTreeSet::new();
            for o in input.options {
                let o = o.trim().to_owned();
                if o.is_empty() || o.chars().count() > 100 || !seen.insert(o.to_lowercase()) {
                    return Err(invalid("options", "options must be unique and 1 to 100 characters"));
                }
                options.push(o);
            }
            if options.is_empty() || options.len() > 50 {
                return Err(invalid("options", "a choice field needs 1 to 50 options"));
            }
        } else if !input.options.is_empty() {
            return Err(invalid("options", "only a choice field takes options"));
        }
        Ok(Self {
            id,
            key,
            label,
            kind: input.kind,
            options,
            required: input.required,
            unique: input.unique,
            searchable: input.searchable,
            show_in_list: input.show_in_list,
            active: true,
        })
    }

    /// Checks one value (already trimmed and not empty).
    fn check(&self, value: &str) -> Result<(), CustomFieldError> {
        let bad = |reason: &str| Err(invalid(self.key.clone(), reason));
        match self.kind {
            CustomKind::Text => {
                if value.chars().count() > 2000 {
                    return bad("is longer than 2000 characters");
                }
                Ok(())
            }
            CustomKind::Number => match value.parse::<f64>() {
                Ok(n) if n.is_finite() => Ok(()),
                _ => bad("is not a number"),
            },
            CustomKind::Date => match NaiveDate::parse_from_str(value, "%Y-%m-%d") {
                Ok(_) => Ok(()),
                Err(_) => bad("is not a date (YYYY-MM-DD)"),
            },
            CustomKind::Boolean => match value {
                "true" | "false" => Ok(()),
                _ => bad("is not true or false"),
            },
            CustomKind::Choice => {
                if self.options.iter().any(|o| o == value) {
                    Ok(())
                } else {
                    bad("is not one of the options")
                }
            }
        }
    }
}

/// A Field Set: a group of custom fields that applies to some Asset Types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldSet {
    /// ID.
    pub id: Uuid,
    /// Name (unique, 1 to 100 characters).
    pub name: String,
    /// Asset Types it applies to.
    pub asset_type_ids: BTreeSet<Uuid>,
    /// Fields in display order.
    pub fields: Vec<FieldDefinition>,
    /// Active.
    pub active: bool,
}

impl FieldSet {
    /// Validates a Field Set.
    ///
    /// # Errors
    ///
    /// An empty or too long name.
    pub fn new(id: Uuid, name: &str, asset_type_ids: BTreeSet<Uuid>) -> Result<Self, CustomFieldError> {
        let name = name.trim().to_owned();
        if name.is_empty() || name.chars().count() > 100 {
            return Err(invalid("name", "must be 1 to 100 characters"));
        }
        Ok(Self {
            id,
            name,
            asset_type_ids,
            fields: Vec::new(),
            active: true,
        })
    }

    /// Checks that a key is not used by a field of this set already.
    ///
    /// # Errors
    ///
    /// The key is already used here.
    pub fn check_new_key(&self, key: &str) -> Result<(), CustomFieldError> {
        if self.fields.iter().any(|f| f.key == key) {
            return Err(invalid("key", "is already a field of this Field Set"));
        }
        Ok(())
    }
}

/// Checks the values of an Asset against the fields that apply to its Asset Type.
///
/// Unknown keys and inactive fields are refused; blank values are dropped; required fields
/// must end up filled in. Returns the values to store.
///
/// # Errors
///
/// An unknown key, a bad value, or a missing required value.
pub fn check_values(
    applicable: &[FieldDefinition],
    values: &BTreeMap<String, String>,
) -> Result<BTreeMap<Uuid, String>, CustomFieldError> {
    let mut out = BTreeMap::new();
    for (key, raw) in values {
        let Some(field) = applicable.iter().find(|f| &f.key == key) else {
            return Err(invalid(key.clone(), "is not a custom field of this Asset's type"));
        };
        if !field.active {
            return Err(invalid(key.clone(), "is an inactive field"));
        }
        let value = raw.trim();
        if value.is_empty() {
            continue;
        }
        field.check(value)?;
        out.insert(field.id, value.to_owned());
    }
    for field in applicable.iter().filter(|f| f.active && f.required) {
        if !out.contains_key(&field.id) {
            return Err(invalid(field.key.clone(), "is required"));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)] // test code

    use super::*;

    fn input(key: &str, kind: CustomKind) -> FieldInput {
        FieldInput {
            key: key.into(),
            label: "Label".into(),
            kind,
            options: Vec::new(),
            required: false,
            unique: false,
            searchable: false,
            show_in_list: false,
        }
    }

    fn values(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect()
    }

    #[test]
    fn keys_and_choice_options_are_checked() {
        assert!(FieldDefinition::new(Uuid::now_v7(), input("rack_unit", CustomKind::Number)).is_ok());
        assert!(FieldDefinition::new(Uuid::now_v7(), input("Rack", CustomKind::Number)).is_err());
        let mut choice = input("colour", CustomKind::Choice);
        assert!(FieldDefinition::new(Uuid::now_v7(), choice.clone()).is_err(), "a choice needs options");
        choice.options = vec!["Red".into(), "red".into()];
        assert!(FieldDefinition::new(Uuid::now_v7(), choice.clone()).is_err(), "options are unique");
        choice.options = vec!["Red".into(), "Blue".into()];
        assert!(FieldDefinition::new(Uuid::now_v7(), choice).is_ok());
        let mut text = input("note", CustomKind::Text);
        text.options = vec!["x".into()];
        assert!(FieldDefinition::new(Uuid::now_v7(), text).is_err());
    }

    #[test]
    fn a_field_set_needs_a_name_and_keys_are_unique_within_it() {
        assert!(FieldSet::new(Uuid::now_v7(), "  ", BTreeSet::new()).is_err());
        let mut set = FieldSet::new(Uuid::now_v7(), "Servers", BTreeSet::new()).unwrap();
        set.fields.push(FieldDefinition::new(Uuid::now_v7(), input("rack", CustomKind::Text)).unwrap());
        assert!(set.check_new_key("rack").is_err());
        assert!(set.check_new_key("rack_unit").is_ok());
    }

    #[test]
    fn values_are_typed_and_required_values_must_be_present() {
        let mut date = input("warranty_plan", CustomKind::Date);
        date.required = true;
        let mut flag = input("backed_up", CustomKind::Boolean);
        flag.required = false;
        let fields = vec![
            FieldDefinition::new(Uuid::now_v7(), date).unwrap(),
            FieldDefinition::new(Uuid::now_v7(), flag).unwrap(),
        ];
        assert!(check_values(&fields, &values(&[("backed_up", "true")])).is_err(), "required date missing");
        let ok = check_values(&fields, &values(&[("warranty_plan", "2027-01-31"), ("backed_up", " true ")])).unwrap();
        assert_eq!(ok.len(), 2);
        assert!(check_values(&fields, &values(&[("warranty_plan", "31/01/2027")])).is_err());
        assert!(check_values(&fields, &values(&[("warranty_plan", "2027-01-31"), ("other", "x")])).is_err());
        let blank = check_values(&fields, &values(&[("warranty_plan", "2027-01-31"), ("backed_up", "  ")])).unwrap();
        assert_eq!(blank.len(), 1, "blank values are not stored");
    }
}
