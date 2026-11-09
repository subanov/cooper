//! CSV exports (RFC 4180) that are safe to open in a spreadsheet.
//!
//! Every cell is escaped (quoted when it holds a comma, quote or line break; quotes
//! doubled), and a cell that a spreadsheet would run as a formula (one starting with
//! `=`, `+`, `-`, `@`, a tab or a carriage return) is prefixed with `'` so it is shown as
//! text (CSV injection).

use std::borrow::Cow;

use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};

/// The media type of CSV responses.
pub const TEXT_CSV: &str = "text/csv; charset=utf-8";

/// Characters that make a spreadsheet read a cell as a formula.
const FORMULA_START: [char; 6] = ['=', '+', '-', '@', '\t', '\r'];

/// One cell, escaped and protected against formula injection.
#[must_use]
pub fn cell(value: &str) -> Cow<'_, str> {
    let guarded: Cow<'_, str> = if value.starts_with(FORMULA_START) {
        Cow::Owned(format!("'{value}"))
    } else {
        Cow::Borrowed(value)
    };
    if guarded.contains([',', '"', '\n', '\r']) {
        Cow::Owned(format!("\"{}\"", guarded.replace('"', "\"\"")))
    } else {
        guarded
    }
}

/// Builds a CSV document row by row (lines end with CRLF).
#[derive(Debug, Default, Clone)]
pub struct CsvWriter {
    buf: String,
}

impl CsvWriter {
    /// An empty document.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends one row.
    pub fn row<I, S>(&mut self, cells: I)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        for (i, c) in cells.into_iter().enumerate() {
            if i > 0 {
                self.buf.push(',');
            }
            self.buf.push_str(&cell(c.as_ref()));
        }
        self.buf.push_str("\r\n");
    }

    /// The document.
    #[must_use]
    pub fn finish(self) -> String {
        self.buf
    }
}

/// A CSV download: `text/csv`, never sniffed, saved as `file_name` (ASCII letters,
/// digits, `.`, `-` and `_`; anything else becomes `_`).
#[must_use]
pub fn download(file_name: &str, body: String) -> Response {
    let safe: String = file_name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let mut response = (StatusCode::OK, body).into_response();
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(TEXT_CSV));
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    if let Ok(v) = HeaderValue::from_str(&format!("attachment; filename=\"{safe}\"")) {
        headers.insert(header::CONTENT_DISPOSITION, v);
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_cells_are_written_as_they_are() {
        assert_eq!(cell("Backups completed"), "Backups completed");
        assert_eq!(cell(""), "");
        assert_eq!(cell("2026-11-09"), "2026-11-09");
    }

    #[test]
    fn cells_with_separators_quotes_or_line_breaks_are_quoted() {
        assert_eq!(cell("a,b"), "\"a,b\"");
        assert_eq!(cell("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(cell("line 1\nline 2"), "\"line 1\nline 2\"");
        assert_eq!(cell("a\r\nb"), "\"a\r\nb\"");
    }

    #[test]
    fn cells_a_spreadsheet_would_run_as_formulas_are_prefixed() {
        assert_eq!(cell("=HYPERLINK(\"x\")"), "\"'=HYPERLINK(\"\"x\"\")\"");
        assert_eq!(cell("+1"), "'+1");
        assert_eq!(cell("-2+3"), "'-2+3");
        assert_eq!(cell("@SUM(A1)"), "'@SUM(A1)");
        assert_eq!(cell("\tx"), "'\tx");
        assert_eq!(cell("\rx"), "\"'\rx\"");
        assert_eq!(cell("a=b"), "a=b", "only a leading sign matters");
    }

    #[test]
    fn rows_are_comma_separated_and_end_with_crlf() {
        let mut w = CsvWriter::new();
        w.row(["Check Date", "Check", "Notes"]);
        w.row(["2026-11-09", "Disk space", "=cmd|' /C calc'!A0, oops"]);
        assert_eq!(
            w.finish(),
            "Check Date,Check,Notes\r\n2026-11-09,Disk space,\"'=cmd|' /C calc'!A0, oops\"\r\n"
        );
    }

    #[test]
    fn downloads_are_csv_attachments_with_a_safe_name() {
        let r = download("morning checks/2026\".csv", "a\r\n".into());
        assert_eq!(r.headers()[header::CONTENT_TYPE], TEXT_CSV);
        assert_eq!(
            r.headers()[header::CONTENT_DISPOSITION],
            "attachment; filename=\"morning_checks_2026_.csv\""
        );
        assert_eq!(r.headers()[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
    }
}
