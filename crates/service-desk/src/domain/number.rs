//! Ticket Number formats (FR-TKT-002).
//!
//! A format is text with tokens: `{COMPANY}` (the Company's Ticket Code), `{TYPE}` (the
//! Ticket Type's number prefix, empty when it has none), `{YEAR}` (four digits, UTC) and
//! exactly one `{SEQ:n}` (the Company's sequence, zero-padded to `n` digits, 1 to 12).
//! For example `{COMPANY}-{SEQ:6}` gives `ACME-000042`.

use std::fmt;

use chrono::{DateTime, Datelike, Utc};

use super::TicketError;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Part {
    Text(String),
    Company,
    Type,
    Year,
    Seq(usize),
}

/// A validated Ticket Number format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NumberFormat {
    source: String,
    parts: Vec<Part>,
}

/// Values for the tokens of a format.
#[derive(Debug, Clone, Copy)]
pub struct NumberContext<'a> {
    /// The Company's Ticket Code.
    pub company_code: &'a str,
    /// The Ticket Type's prefix.
    pub type_prefix: Option<&'a str>,
    /// Creation time.
    pub now: DateTime<Utc>,
    /// The next value of the Company's sequence.
    pub sequence: u64,
}

impl NumberFormat {
    /// The default format.
    pub const DEFAULT: &'static str = "{COMPANY}-{SEQ:6}";

    /// Parses a format.
    ///
    /// # Errors
    ///
    /// Unknown tokens, a missing or repeated `{SEQ:n}`, characters other than letters,
    /// digits and `-_./`, or a format longer than 50 characters.
    pub fn parse(source: &str) -> Result<Self, TicketError> {
        let bad = |reason: &str| TicketError::invalid("number_format", reason);
        if source.is_empty() || source.chars().count() > 50 {
            return Err(bad("must be 1 to 50 characters"));
        }
        let mut parts = Vec::new();
        let mut rest = source;
        while !rest.is_empty() {
            if let Some(after) = rest.strip_prefix('{') {
                let end = after.find('}').ok_or_else(|| bad("has an unclosed {"))?;
                let token = &after[..end];
                parts.push(match token {
                    "COMPANY" => Part::Company,
                    "TYPE" => Part::Type,
                    "YEAR" => Part::Year,
                    t => {
                        let width = t
                            .strip_prefix("SEQ:")
                            .and_then(|w| w.parse::<usize>().ok())
                            .filter(|w| (1..=12).contains(w))
                            .ok_or_else(|| bad("knows only {COMPANY}, {TYPE}, {YEAR} and {SEQ:n} with n from 1 to 12"))?;
                        Part::Seq(width)
                    }
                });
                rest = &after[end + 1..];
            } else {
                let end = rest.find('{').unwrap_or(rest.len());
                let text = &rest[..end];
                if !text
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-_./".contains(c))
                {
                    return Err(bad("may only contain letters, digits and - _ . /"));
                }
                parts.push(Part::Text(text.to_owned()));
                rest = &rest[end..];
            }
        }
        if parts.iter().filter(|p| matches!(p, Part::Seq(_))).count() != 1 {
            return Err(bad("must contain {SEQ:n} exactly once"));
        }
        Ok(Self {
            source: source.to_owned(),
            parts,
        })
    }

    /// Renders a number.
    #[must_use]
    pub fn render(&self, ctx: NumberContext<'_>) -> String {
        self.parts
            .iter()
            .map(|p| match p {
                Part::Text(t) => t.clone(),
                Part::Company => ctx.company_code.to_owned(),
                Part::Type => ctx.type_prefix.unwrap_or_default().to_owned(),
                Part::Year => format!("{:04}", ctx.now.year()),
                Part::Seq(width) => format!("{:0width$}", ctx.sequence, width = *width),
            })
            .collect()
    }
}

impl Default for NumberFormat {
    /// [`NumberFormat::DEFAULT`]: `{COMPANY}-{SEQ:6}`.
    fn default() -> Self {
        Self {
            source: Self::DEFAULT.to_owned(),
            parts: vec![Part::Company, Part::Text("-".to_owned()), Part::Seq(6)],
        }
    }
}

impl fmt::Display for NumberFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.source)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use proptest::prelude::*;

    fn ctx(sequence: u64, type_prefix: Option<&str>) -> NumberContext<'_> {
        NumberContext {
            company_code: "ACME",
            type_prefix,
            now: Utc.with_ymd_and_hms(2026, 5, 1, 0, 0, 0).unwrap(),
            sequence,
        }
    }

    #[test]
    fn renders_tokens() {
        let f = NumberFormat::parse(NumberFormat::DEFAULT).unwrap();
        assert_eq!(f, NumberFormat::default());
        assert_eq!(f.render(ctx(42, None)), "ACME-000042");
        let f = NumberFormat::parse("{TYPE}{YEAR}/{SEQ:4}").unwrap();
        assert_eq!(f.render(ctx(7, Some("INC"))), "INC2026/0007");
        assert_eq!(
            f.render(ctx(123_456, None)),
            "2026/123456",
            "longer numbers are not truncated"
        );
    }

    #[test]
    fn rejects_bad_formats() {
        for bad in [
            "",
            "{COMPANY}",
            "{SEQ:6}{SEQ:2}",
            "{SEQ:0}",
            "{SEQ:13}",
            "{FOO}{SEQ:3}",
            "{SEQ:3",
            "A B{SEQ:3}",
        ] {
            assert!(NumberFormat::parse(bad).is_err(), "{bad:?}");
        }
    }

    proptest! {
        /// Distinct sequence values always give distinct numbers (FR-TKT-002).
        #[test]
        fn distinct_sequences_give_distinct_numbers(a in 1u64..10_000_000, b in 1u64..10_000_000, width in 1usize..12) {
            prop_assume!(a != b);
            let f = NumberFormat::parse(&format!("{{COMPANY}}-{{SEQ:{width}}}")).unwrap();
            prop_assert_ne!(f.render(ctx(a, None)), f.render(ctx(b, None)));
        }
    }
}
