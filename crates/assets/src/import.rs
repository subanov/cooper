//! CSV import of Assets (FR-AST-007, ADR-0041). Pure Rust: parsing, column mapping and the
//! per-row plan. Writing the Assets is the application's job.

use std::collections::BTreeMap;

use thiserror::Error;

/// Most data rows in one import.
pub const MAX_ROWS: usize = 2000;

/// An import problem that stops the whole file.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ImportError {
    /// The file has no header or no rows.
    #[error("the file needs a header row and at least one row")]
    Empty,
    /// Too many rows.
    #[error("at most 2000 rows per import")]
    TooManyRows,
    /// A mapping names a column the file does not have, or a field that cannot be mapped.
    #[error("mapping: {0}")]
    Mapping(String),
    /// The identifying columns are missing.
    #[error("the mapping needs at least one of hostname, service_tag or asset_tag")]
    NoIdentifier,
}

/// Splits CSV text into a header and rows. Handles quoted fields with commas and doubled
/// quotes; a quoted field may not span lines.
///
/// # Errors
///
/// No header, no rows, or too many rows.
pub fn parse_csv(text: &str) -> Result<(Vec<String>, Vec<Vec<String>>), ImportError> {
    let mut records: Vec<Vec<String>> = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let mut fields = Vec::new();
        let mut field = String::new();
        let mut quoted = false;
        let mut chars = line.chars().peekable();
        while let Some(c) = chars.next() {
            match (c, quoted) {
                ('"', true) if chars.peek() == Some(&'"') => {
                    field.push('"');
                    chars.next();
                }
                ('"', _) => quoted = !quoted,
                (',', false) => fields.push(std::mem::take(&mut field)),
                (c, _) => field.push(c),
            }
        }
        fields.push(field);
        records.push(fields.into_iter().map(|f| f.trim().to_owned()).collect());
    }
    let mut iter = records.into_iter();
    let header = iter.next().ok_or(ImportError::Empty)?;
    let rows: Vec<Vec<String>> = iter.collect();
    if rows.is_empty() {
        return Err(ImportError::Empty);
    }
    if rows.len() > MAX_ROWS {
        return Err(ImportError::TooManyRows);
    }
    Ok((header, rows))
}

/// A field of an Asset that an import can set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ImportField {
    /// Hostname.
    Hostname,
    /// Service tag.
    ServiceTag,
    /// Asset tag.
    AssetTag,
    /// Purchase date.
    PurchaseDate,
    /// Warranty expiry.
    WarrantyExpiresOn,
    /// Lease start.
    LeaseStartsOn,
    /// Lease expiry.
    LeaseExpiresOn,
}

impl ImportField {
    /// The name used in mappings.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Hostname => "hostname",
            Self::ServiceTag => "service_tag",
            Self::AssetTag => "asset_tag",
            Self::PurchaseDate => "purchase_date",
            Self::WarrantyExpiresOn => "warranty_expires_on",
            Self::LeaseStartsOn => "lease_starts_on",
            Self::LeaseExpiresOn => "lease_expires_on",
        }
    }

    /// Parses a mapping target.
    ///
    /// # Errors
    ///
    /// Not an importable field.
    pub fn parse(s: &str) -> Result<Self, ImportError> {
        Ok(match s {
            "hostname" => Self::Hostname,
            "service_tag" => Self::ServiceTag,
            "asset_tag" => Self::AssetTag,
            "purchase_date" => Self::PurchaseDate,
            "warranty_expires_on" => Self::WarrantyExpiresOn,
            "lease_starts_on" => Self::LeaseStartsOn,
            "lease_expires_on" => Self::LeaseExpiresOn,
            other => return Err(ImportError::Mapping(format!("{other:?} cannot be imported"))),
        })
    }

    /// Whether the field identifies an Asset.
    #[must_use]
    pub const fn identifies(self) -> bool {
        matches!(self, Self::Hostname | Self::ServiceTag | Self::AssetTag)
    }
}

/// Maps file columns to fields. Columns not in the mapping are ignored.
///
/// # Errors
///
/// A mapped column the header does not have, or no identifying field.
pub fn check_mapping(
    header: &[String],
    mapping: &BTreeMap<String, ImportField>,
) -> Result<Vec<(usize, ImportField)>, ImportError> {
    let mut out = Vec::new();
    for (column, field) in mapping {
        let idx = header
            .iter()
            .position(|h| h == column)
            .ok_or_else(|| ImportError::Mapping(format!("the file has no column {column:?}")))?;
        out.push((idx, *field));
    }
    if !out.iter().any(|(_, f)| f.identifies()) {
        return Err(ImportError::NoIdentifier);
    }
    Ok(out)
}

/// The values of one row, by field. Blank cells are absent.
#[must_use]
pub fn row_values(mapped: &[(usize, ImportField)], row: &[String]) -> BTreeMap<ImportField, String> {
    mapped
        .iter()
        .filter_map(|(idx, field)| {
            row.get(*idx)
                .map(|v| v.trim().to_owned())
                .filter(|v| !v.is_empty())
                .map(|v| (*field, v))
        })
        .collect()
}

/// What happened to one row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowOutcome {
    /// A new Asset was created.
    Created,
    /// An existing Asset was updated.
    Updated,
    /// Nothing needed changing.
    Unchanged,
    /// The row could not be imported; the message says why.
    Failed,
}

impl RowOutcome {
    /// The name in the outcome log.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Updated => "updated",
            Self::Unchanged => "unchanged",
            Self::Failed => "failed",
        }
    }
}

/// What to do with Assets of the import's type that the file does not mention.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MissingPolicy {
    /// Leave them alone.
    Ignore,
    /// List them in the outcome log.
    Flag,
    /// Retire them (reason: missing from the import).
    Deactivate,
}

impl MissingPolicy {
    /// Parses the API name.
    ///
    /// # Errors
    ///
    /// Not a policy.
    pub fn parse(s: &str) -> Result<Self, ImportError> {
        match s {
            "ignore" => Ok(Self::Ignore),
            "flag" => Ok(Self::Flag),
            "deactivate" => Ok(Self::Deactivate),
            other => Err(ImportError::Mapping(format!("{other:?} is not a missing-asset policy"))),
        }
    }
}

/// How existing Assets are changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteMode {
    /// Only fill empty fields.
    Fill,
    /// Set every mapped field the file gives.
    Overwrite,
}

impl WriteMode {
    /// Parses the API name.
    ///
    /// # Errors
    ///
    /// Not a mode.
    pub fn parse(s: &str) -> Result<Self, ImportError> {
        match s {
            "fill" => Ok(Self::Fill),
            "overwrite" => Ok(Self::Overwrite),
            other => Err(ImportError::Mapping(format!("{other:?} is not a write mode"))),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)] // test code

    use super::*;

    #[test]
    fn csv_with_quotes_and_blank_lines_is_read() {
        let text = "hostname,asset_tag,note\n\"lt-1\",AST-1,\"a, b\"\n\n lt-2 ,AST-2, \"say \"\"hi\"\"\"\n";
        let (header, rows) = parse_csv(text).unwrap();
        assert_eq!(header, ["hostname", "asset_tag", "note"]);
        assert_eq!(rows[0][2], "a, b");
        assert_eq!(rows[1][2], "say \"hi\"");
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn an_empty_file_and_too_many_rows_are_refused() {
        assert_eq!(parse_csv("hostname
"), Err(ImportError::Empty));
        let mut big = String::from("hostname
");
        for i in 0..=MAX_ROWS {
            big.push_str(&format!("h{i}
"));
        }
        assert_eq!(parse_csv(&big), Err(ImportError::TooManyRows));
    }

    #[test]
    fn mappings_must_name_real_columns_and_an_identifier() {
        let header: Vec<String> = ["Host", "Tag", "Price"].iter().map(|s| (*s).to_owned()).collect();
        let mut m = BTreeMap::new();
        m.insert("Host".to_owned(), ImportField::Hostname);
        assert!(check_mapping(&header, &m).is_ok());
        m.insert("Nope".to_owned(), ImportField::AssetTag);
        assert!(check_mapping(&header, &m).is_err());
        let mut only_date = BTreeMap::new();
        only_date.insert("Price".to_owned(), ImportField::PurchaseDate);
        assert_eq!(check_mapping(&header, &only_date), Err(ImportError::NoIdentifier));
        assert!(ImportField::parse("cost").is_err());
    }

    #[test]
    fn row_values_skip_blank_cells() {
        let header: Vec<String> = ["Host", "Tag"].iter().map(|s| (*s).to_owned()).collect();
        let mut m = BTreeMap::new();
        m.insert("Host".to_owned(), ImportField::Hostname);
        m.insert("Tag".to_owned(), ImportField::AssetTag);
        let mapped = check_mapping(&header, &m).unwrap();
        let values = row_values(&mapped, &["lt-1".into(), "  ".into()]);
        assert_eq!(values.len(), 1);
        assert_eq!(values.get(&ImportField::Hostname).map(String::as_str), Some("lt-1"));
    }
}
