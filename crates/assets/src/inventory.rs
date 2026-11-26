//! Inventory reports, software and licences (FR-AST-008, FR-AST-013, FR-AST-014; ADR-0041).
//! Pure Rust.

use std::collections::BTreeSet;

use thiserror::Error;

/// An inventory problem.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum InventoryError {
    /// A value failed validation.
    #[error("{field}: {reason}")]
    Invalid {
        /// The field.
        field: &'static str,
        /// Why it is not valid.
        reason: String,
    },
}

fn invalid(field: &'static str, reason: impl Into<String>) -> InventoryError {
    InventoryError::Invalid {
        field,
        reason: reason.into(),
    }
}

fn text(field: &'static str, value: Option<String>, max: usize) -> Result<Option<String>, InventoryError> {
    match value.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty()) {
        None => Ok(None),
        Some(v) if v.chars().count() > max || v.chars().any(char::is_control) => {
            Err(invalid(field, format!("at most {max} printable characters")))
        }
        Some(v) => Ok(Some(v)),
    }
}

/// Most software items in one report.
pub const MAX_SOFTWARE: usize = 2000;
/// Most disks, and most network adapters, in one report.
pub const MAX_PARTS: usize = 100;

/// One installed application.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SoftwareItem {
    /// Application name (1 to 255 characters).
    pub name: String,
    /// Publisher (may be empty).
    pub publisher: String,
    /// Installed version (may be empty).
    pub version: String,
}

impl SoftwareItem {
    /// Validates one item.
    ///
    /// # Errors
    ///
    /// An empty or too long name, or a too long publisher or version.
    pub fn new(name: &str, publisher: &str, version: &str) -> Result<Self, InventoryError> {
        let name = text("software.name", Some(name.to_owned()), 255)?
            .ok_or_else(|| invalid("software.name", "is required"))?;
        Ok(Self {
            name,
            publisher: text("software.publisher", Some(publisher.to_owned()), 255)?.unwrap_or_default(),
            version: text("software.version", Some(version.to_owned()), 100)?.unwrap_or_default(),
        })
    }
}

/// Hardware of a report: manufacturer, model, operating system, processor and memory (MB).
pub type HardwareInput = (Option<String>, Option<String>, Option<String>, Option<String>, Option<u32>);

/// A validated inventory report for one Asset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InventoryReport {
    /// Service tag, used to find the Asset.
    pub service_tag: Option<String>,
    /// Hostname, used to find the Asset.
    pub hostname: Option<String>,
    /// Asset tag, used to find the Asset.
    pub asset_tag: Option<String>,
    /// Manufacturer.
    pub manufacturer: Option<String>,
    /// Model.
    pub model: Option<String>,
    /// Operating system.
    pub os: Option<String>,
    /// Processor.
    pub cpu: Option<String>,
    /// Memory, in MB.
    pub ram_mb: Option<u32>,
    /// Disks: name and size in GB.
    pub disks: Vec<(String, u32)>,
    /// Network adapters: name, MAC and IP address.
    pub adapters: Vec<(String, Option<String>, Option<String>)>,
    /// Installed software, without duplicates.
    pub software: BTreeSet<SoftwareItem>,
}

impl InventoryReport {
    /// Validates a report. At least one identifier is needed to find the Asset.
    ///
    /// # Errors
    ///
    /// No identifier, a bad value, or too many items.
    pub fn new(
        ids: (Option<String>, Option<String>, Option<String>),
        hardware: HardwareInput,
        disks: Vec<(String, u32)>,
        adapters: Vec<(String, Option<String>, Option<String>)>,
        software: Vec<SoftwareItem>,
    ) -> Result<Self, InventoryError> {
        let (service_tag, hostname, asset_tag) = (
            text("service_tag", ids.0, 100)?,
            text("hostname", ids.1, 255)?,
            text("asset_tag", ids.2, 50)?,
        );
        if service_tag.is_none() && hostname.is_none() && asset_tag.is_none() {
            return Err(invalid("identifier", "a service tag, hostname or asset tag is required"));
        }
        if disks.len() > MAX_PARTS || adapters.len() > MAX_PARTS {
            return Err(invalid("parts", "at most 100 disks and 100 network adapters"));
        }
        if software.len() > MAX_SOFTWARE {
            return Err(invalid("software", "at most 2000 items"));
        }
        let (manufacturer, model, os, cpu, ram_mb) = hardware;
        let disks = disks
            .into_iter()
            .map(|(name, gb)| Ok((text("disk.name", Some(name), 100)?.unwrap_or_default(), gb)))
            .collect::<Result<Vec<_>, InventoryError>>()?;
        let adapters = adapters
            .into_iter()
            .map(|(name, mac, ip)| {
                Ok((
                    text("adapter.name", Some(name), 100)?.unwrap_or_default(),
                    text("adapter.mac", mac, 17)?,
                    text("adapter.ip", ip, 45)?,
                ))
            })
            .collect::<Result<Vec<_>, InventoryError>>()?;
        Ok(Self {
            service_tag,
            hostname,
            asset_tag,
            manufacturer: text("manufacturer", manufacturer, 100)?,
            model: text("model", model, 100)?,
            os: text("os", os, 200)?,
            cpu: text("cpu", cpu, 200)?,
            ram_mb,
            disks,
            adapters,
            software: software.into_iter().collect(),
        })
    }
}

/// Whether a licence is in force.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LicenceStatus {
    /// In force.
    Active,
    /// Ran out.
    Expired,
    /// Cancelled.
    Cancelled,
}

impl LicenceStatus {
    /// The stored name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Expired => "expired",
            Self::Cancelled => "cancelled",
        }
    }

    /// Parses the stored or API name.
    ///
    /// # Errors
    ///
    /// Not a licence status.
    pub fn parse(s: &str) -> Result<Self, InventoryError> {
        match s {
            "active" => Ok(Self::Active),
            "expired" => Ok(Self::Expired),
            "cancelled" => Ok(Self::Cancelled),
            other => Err(invalid("status", format!("{other:?} is not a licence status"))),
        }
    }
}

/// Compliance of a licence with its installations (FR-AST-014).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compliance {
    /// Installations within the quantity.
    Compliant,
    /// More installations than licences.
    OverUsed,
    /// No application is linked, so there is nothing to count.
    NoApplications,
}

/// Compares installations with the quantity of a licence.
#[must_use]
pub const fn compliance(quantity: u32, installations: u32, linked_applications: u32) -> Compliance {
    if linked_applications == 0 {
        Compliance::NoApplications
    } else if installations > quantity {
        Compliance::OverUsed
    } else {
        Compliance::Compliant
    }
}

/// The last four characters of a licence key, for display (the key itself is never shown).
#[must_use]
pub fn key_hint(key: &str) -> Option<String> {
    let chars: Vec<char> = key.trim().chars().collect();
    (chars.len() >= 4).then(|| chars[chars.len() - 4..].iter().collect())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)] // test code

    use super::*;

    #[test]
    fn a_report_needs_an_identifier_and_bounded_parts() {
        let none = InventoryReport::new((None, None, None), (None, None, None, None, None), vec![], vec![], vec![]);
        assert!(none.is_err());
        let by_host = InventoryReport::new(
            (None, Some(" lt-42 ".into()), None),
            (None, None, None, None, Some(16384)),
            vec![("C:".into(), 512)],
            vec![("eth0".into(), Some("00:11:22:33:44:55".into()), None)],
            vec![SoftwareItem::new("Git", "Software Freedom", "2.45").unwrap()],
        )
        .unwrap();
        assert_eq!(by_host.hostname.as_deref(), Some("lt-42"));
        let too_many: Vec<(String, u32)> = (0..=MAX_PARTS).map(|i| (format!("d{i}"), 1)).collect();
        assert!(InventoryReport::new((Some("S1".into()), None, None), (None, None, None, None, None), too_many, vec![], vec![]).is_err());
    }

    #[test]
    fn software_is_identified_by_name_and_publisher_and_duplicates_collapse() {
        assert!(SoftwareItem::new("  ", "x", "1").is_err());
        let a = SoftwareItem::new("Git", "SFC", "2.45").unwrap();
        let b = SoftwareItem::new(" Git ", "SFC", "2.45").unwrap();
        assert_eq!(a, b);
        let report = InventoryReport::new(
            (Some("S1".into()), None, None),
            (None, None, None, None, None),
            vec![],
            vec![],
            vec![a, b],
        )
        .unwrap();
        assert_eq!(report.software.len(), 1);
    }

    #[test]
    fn compliance_counts_installations_against_quantity() {
        assert_eq!(compliance(5, 3, 1), Compliance::Compliant);
        assert_eq!(compliance(2, 3, 1), Compliance::OverUsed);
        assert_eq!(compliance(2, 0, 0), Compliance::NoApplications);
    }

    #[test]
    fn only_the_last_four_characters_of_a_key_are_shown() {
        assert_eq!(key_hint("ABCD-EFGH-1234").as_deref(), Some("1234"));
        assert_eq!(key_hint("12"), None);
        assert_eq!(LicenceStatus::parse("expired").unwrap(), LicenceStatus::Expired);
        assert!(LicenceStatus::parse("gone").is_err());
    }
}
