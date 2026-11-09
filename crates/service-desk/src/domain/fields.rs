//! Custom Ticket Fields (FR-TKT-029): typed fields Administrators define, with values
//! per ticket. A field can be required when a ticket is created, required before it is
//! closed (FR-TKT-008), and asked of the Requester on the Portal.

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, NaiveDate, Utc};
use serde_json::{Value, json};
use uuid::Uuid;

use super::TicketError;

/// What kind of value a field holds (fixed set). Cannot change once the field exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FieldType {
    /// One line of text (at most 500 characters).
    Text,
    /// Free text (at most 10,000 characters).
    LongText,
    /// A decimal number.
    Number,
    /// A calendar date (`YYYY-MM-DD`).
    Date,
    /// One of the field's options.
    Dropdown,
    /// Yes or no.
    Checkbox,
}

impl FieldType {
    /// Stable storage and API name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::LongText => "long_text",
            Self::Number => "number",
            Self::Date => "date",
            Self::Dropdown => "dropdown",
            Self::Checkbox => "checkbox",
        }
    }
}

impl FromStr for FieldType {
    type Err = TicketError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "text" => Self::Text,
            "long_text" => Self::LongText,
            "number" => Self::Number,
            "date" => Self::Date,
            "dropdown" => Self::Dropdown,
            "checkbox" => Self::Checkbox,
            _ => {
                return Err(TicketError::invalid(
                    "field_type",
                    "must be text, long_text, number, date, dropdown or checkbox",
                ));
            }
        })
    }
}

impl fmt::Display for FieldType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What an Administrator sets on a field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketFieldInput {
    /// Stable machine key (`lower_snake_case`), used by the API. Never changes.
    pub key: String,
    /// Label shown to people.
    pub label: String,
    /// Value type. Ignored on update: the type of an existing field never changes.
    pub field_type: FieldType,
    /// Options of a dropdown, in order.
    pub options: Vec<String>,
    /// Help text.
    pub help_text: String,
    /// Ticket Types the field applies to; empty means every type.
    pub type_ids: Vec<Uuid>,
    /// Must have a value when a ticket is created by an Analyst or on the Portal.
    pub required_on_create: bool,
    /// Must have a value before the ticket can be closed.
    pub required_on_close: bool,
    /// Asked of the Requester on the Portal's new ticket form.
    pub ask_requester: bool,
    /// Display order.
    pub sort_order: i32,
    /// Inactive (retired) fields are hidden and never required; values are kept.
    pub is_active: bool,
}

/// A custom Ticket Field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketField {
    id: Uuid,
    input: TicketFieldInput,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

fn valid_key(key: &str) -> bool {
    let mut chars = key.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && key.len() <= 63
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

impl TicketFieldInput {
    fn validate(mut self) -> Result<Self, TicketError> {
        self.key = self.key.trim().to_owned();
        if !valid_key(&self.key) {
            return Err(TicketError::invalid(
                "key",
                "must be lower_snake_case, start with a letter and be at most 63 characters",
            ));
        }
        self.label = self.label.trim().to_owned();
        if self.label.is_empty() || self.label.chars().count() > 150 {
            return Err(TicketError::invalid("label", "must be 1 to 150 characters"));
        }
        if self.help_text.chars().count() > 500 {
            return Err(TicketError::invalid(
                "help_text",
                "must be at most 500 characters",
            ));
        }
        let mut options = Vec::new();
        for option in self.options.iter().map(|o| o.trim()) {
            if option.is_empty() || option.chars().count() > 255 {
                return Err(TicketError::invalid(
                    "options",
                    "each option must be 1 to 255 characters",
                ));
            }
            if options
                .iter()
                .any(|o: &String| o.eq_ignore_ascii_case(option))
            {
                return Err(TicketError::invalid("options", "options must be unique"));
            }
            options.push(option.to_owned());
        }
        match (self.field_type, options.is_empty()) {
            (FieldType::Dropdown, true) => {
                return Err(TicketError::invalid(
                    "options",
                    "a dropdown needs at least one option",
                ));
            }
            (FieldType::Dropdown, false) | (_, true) => {}
            (_, false) => {
                return Err(TicketError::invalid(
                    "options",
                    "only dropdown fields have options",
                ));
            }
        }
        if options.len() > 200 {
            return Err(TicketError::invalid("options", "at most 200 options"));
        }
        self.options = options;
        self.type_ids.sort_unstable();
        self.type_ids.dedup();
        Ok(self)
    }
}

impl TicketField {
    /// Defines a field.
    ///
    /// # Errors
    ///
    /// Invalid key, label, help text or options.
    pub fn define(input: TicketFieldInput, now: DateTime<Utc>) -> Result<Self, TicketError> {
        Ok(Self {
            id: Uuid::now_v7(),
            input: input.validate()?,
            created_at: now,
            updated_at: now,
        })
    }

    /// Rebuilds a field from storage.
    #[must_use]
    pub const fn restore(
        id: Uuid,
        input: TicketFieldInput,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            input,
            created_at,
            updated_at,
        }
    }

    /// Changes the field. The key and the type never change: the API and stored values
    /// depend on them. Removing a dropdown option keeps existing values readable.
    ///
    /// # Errors
    ///
    /// A changed key or type, or invalid input.
    pub fn update(
        &mut self,
        input: TicketFieldInput,
        now: DateTime<Utc>,
    ) -> Result<(), TicketError> {
        if input.key.trim() != self.input.key {
            return Err(TicketError::invalid("key", "cannot change"));
        }
        if input.field_type != self.input.field_type {
            return Err(TicketError::invalid("field_type", "cannot change"));
        }
        self.input = input.validate()?;
        self.updated_at = now;
        Ok(())
    }

    /// Whether the field applies to tickets of `ticket_type`.
    #[must_use]
    pub fn applies_to(&self, ticket_type: Option<Uuid>) -> bool {
        self.input.is_active
            && (self.input.type_ids.is_empty()
                || ticket_type.is_some_and(|t| self.input.type_ids.contains(&t)))
    }

    /// Checks and normalises a value for this field. `null` clears the value.
    ///
    /// # Errors
    ///
    /// A value of the wrong type, out of range, or not one of the options.
    pub fn parse_value(&self, value: &Value) -> Result<Option<Value>, TicketError> {
        let wrong = |what: &str| {
            TicketError::invalid("fields", format!("{} must be {what}", self.input.key))
        };
        Ok(Some(match (self.input.field_type, value) {
            (_, Value::Null) => return Ok(None),
            (FieldType::Text, Value::String(s)) => {
                let s = s.trim();
                if s.is_empty() {
                    return Ok(None);
                }
                if s.chars().count() > 500 {
                    return Err(wrong("at most 500 characters"));
                }
                json!(s)
            }
            (FieldType::LongText, Value::String(s)) => {
                if s.trim().is_empty() {
                    return Ok(None);
                }
                if s.chars().count() > 10_000 {
                    return Err(wrong("at most 10,000 characters"));
                }
                json!(s)
            }
            (FieldType::Number, Value::Number(n)) if n.as_f64().is_some_and(f64::is_finite) => {
                json!(n)
            }
            (FieldType::Date, Value::String(s)) => {
                let date = NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d")
                    .map_err(|_| wrong("a date (YYYY-MM-DD)"))?;
                json!(date.format("%Y-%m-%d").to_string())
            }
            (FieldType::Dropdown, Value::String(s)) => {
                let option = self
                    .input
                    .options
                    .iter()
                    .find(|o| o.as_str() == s.trim())
                    .ok_or_else(|| wrong("one of its options"))?;
                json!(option)
            }
            (FieldType::Checkbox, Value::Bool(b)) => json!(b),
            (FieldType::Text | FieldType::LongText, _) => return Err(wrong("text")),
            (FieldType::Number, _) => return Err(wrong("a number")),
            (FieldType::Date, _) => return Err(wrong("a date (YYYY-MM-DD)")),
            (FieldType::Dropdown, _) => return Err(wrong("one of its options")),
            (FieldType::Checkbox, _) => return Err(wrong("true or false")),
        }))
    }

    /// ID.
    #[must_use]
    pub const fn id(&self) -> Uuid {
        self.id
    }
    /// Key.
    #[must_use]
    pub fn key(&self) -> &str {
        &self.input.key
    }
    /// Everything the Administrator set.
    #[must_use]
    pub const fn input(&self) -> &TicketFieldInput {
        &self.input
    }
    /// Created.
    #[must_use]
    pub const fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }
    /// Last changed.
    #[must_use]
    pub const fn updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }
}

/// When values are checked for required fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldCheck {
    /// An Analyst (or API client) creates a ticket: fields *required on create*.
    AnalystCreate,
    /// A Requester raises a ticket on the Portal: fields *required on create* that are
    /// also *ask requester* (the others cannot be asked).
    PortalCreate,
    /// The ticket is being closed: fields *required on close* (FR-TKT-008).
    Close,
}

/// Validates `submitted` values (by field key) against the field definitions for a
/// ticket of `ticket_type`. Returns the values to store by field ID (`None` clears).
///
/// # Errors
///
/// Unknown or inapplicable keys, or invalid values. On the Portal, keys of fields that
/// are not asked of the Requester are refused.
pub fn parse_values(
    fields: &[TicketField],
    ticket_type: Option<Uuid>,
    submitted: &BTreeMap<String, Value>,
    portal: bool,
) -> Result<BTreeMap<Uuid, Option<Value>>, TicketError> {
    let mut out = BTreeMap::new();
    for (key, value) in submitted {
        let field = fields
            .iter()
            .find(|f| f.key() == key && f.applies_to(ticket_type))
            .filter(|f| !portal || f.input.ask_requester)
            .ok_or_else(|| {
                TicketError::invalid(
                    "fields",
                    format!("{key} is not a Ticket Field of this ticket"),
                )
            })?;
        out.insert(field.id(), field.parse_value(value)?);
    }
    Ok(out)
}

/// Keys of the fields that `check` requires but `values` (by field ID) leaves empty.
#[must_use]
pub fn missing_required(
    fields: &[TicketField],
    ticket_type: Option<Uuid>,
    values: &BTreeMap<Uuid, Value>,
    check: FieldCheck,
) -> Vec<String> {
    fields
        .iter()
        .filter(|f| f.applies_to(ticket_type))
        .filter(|f| match check {
            FieldCheck::AnalystCreate => f.input.required_on_create,
            FieldCheck::PortalCreate => f.input.required_on_create && f.input.ask_requester,
            FieldCheck::Close => f.input.required_on_close,
        })
        .filter(|f| !values.contains_key(&f.id()))
        .map(|f| f.key().to_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(key: &str, field_type: FieldType) -> TicketFieldInput {
        TicketFieldInput {
            key: key.into(),
            label: key.into(),
            field_type,
            options: if field_type == FieldType::Dropdown {
                vec!["Laptop".into(), "Desktop".into()]
            } else {
                Vec::new()
            },
            help_text: String::new(),
            type_ids: Vec::new(),
            required_on_create: false,
            required_on_close: false,
            ask_requester: false,
            sort_order: 0,
            is_active: true,
        }
    }

    fn field(key: &str, field_type: FieldType) -> TicketField {
        TicketField::define(input(key, field_type), Utc::now()).unwrap()
    }

    #[test]
    fn keys_options_and_types_are_validated() {
        assert!(TicketField::define(input("Asset Tag", FieldType::Text), Utc::now()).is_err());
        let mut no_options = input("device", FieldType::Dropdown);
        no_options.options.clear();
        assert!(TicketField::define(no_options, Utc::now()).is_err());
        let mut text_with_options = input("note", FieldType::Text);
        text_with_options.options = vec!["x".into()];
        assert!(TicketField::define(text_with_options, Utc::now()).is_err());
        let mut dupes = input("device", FieldType::Dropdown);
        dupes.options = vec!["A".into(), "a".into()];
        assert!(TicketField::define(dupes, Utc::now()).is_err());
    }

    #[test]
    fn key_and_type_never_change() {
        let mut f = field("asset_tag", FieldType::Text);
        assert!(
            f.update(input("asset_tag", FieldType::Number), Utc::now())
                .is_err()
        );
        assert!(f.update(input("tag", FieldType::Text), Utc::now()).is_err());
        let mut relabelled = input("asset_tag", FieldType::Text);
        relabelled.label = "Asset tag".into();
        f.update(relabelled, Utc::now()).unwrap();
        assert_eq!(f.input().label, "Asset tag");
    }

    #[test]
    fn values_are_typed() {
        let number = field("cost", FieldType::Number);
        assert_eq!(number.parse_value(&json!(12.5)).unwrap(), Some(json!(12.5)));
        assert!(number.parse_value(&json!("12")).is_err());
        let date = field("due", FieldType::Date);
        assert_eq!(
            date.parse_value(&json!("2026-11-07")).unwrap(),
            Some(json!("2026-11-07"))
        );
        assert!(date.parse_value(&json!("07/11/2026")).is_err());
        let dropdown = field("device", FieldType::Dropdown);
        assert_eq!(
            dropdown.parse_value(&json!("Laptop")).unwrap(),
            Some(json!("Laptop"))
        );
        assert!(dropdown.parse_value(&json!("Tablet")).is_err());
        let checkbox = field("vip", FieldType::Checkbox);
        assert!(checkbox.parse_value(&json!("yes")).is_err());
        let text = field("tag", FieldType::Text);
        assert_eq!(
            text.parse_value(&json!("  ")).unwrap(),
            None,
            "blank clears"
        );
        assert_eq!(text.parse_value(&Value::Null).unwrap(), None);
    }

    #[test]
    fn requirements_depend_on_the_moment_and_the_ticket_type() {
        let incident = Uuid::now_v7();
        let mut on_create = input("impact", FieldType::Text);
        on_create.required_on_create = true;
        let mut asked = input("device", FieldType::Dropdown);
        asked.required_on_create = true;
        asked.ask_requester = true;
        let mut on_close = input("root_cause", FieldType::LongText);
        on_close.required_on_close = true;
        on_close.type_ids = vec![incident];
        let fields: Vec<_> = [on_create, asked, on_close]
            .into_iter()
            .map(|i| TicketField::define(i, Utc::now()).unwrap())
            .collect();
        let none = BTreeMap::new();
        assert_eq!(
            missing_required(&fields, None, &none, FieldCheck::AnalystCreate),
            ["impact", "device"]
        );
        assert_eq!(
            missing_required(&fields, None, &none, FieldCheck::PortalCreate),
            ["device"]
        );
        assert!(missing_required(&fields, None, &none, FieldCheck::Close).is_empty());
        assert_eq!(
            missing_required(&fields, Some(incident), &none, FieldCheck::Close),
            ["root_cause"]
        );

        let submitted = BTreeMap::from([("impact".to_owned(), json!("High"))]);
        assert!(
            parse_values(&fields, None, &submitted, true).is_err(),
            "the Portal may only set fields asked of the Requester"
        );
        assert!(parse_values(&fields, None, &submitted, false).is_ok());
        let unknown = BTreeMap::from([("root_cause".to_owned(), json!("x"))]);
        assert!(
            parse_values(&fields, None, &unknown, false).is_err(),
            "root_cause applies to incidents only"
        );
    }
}
