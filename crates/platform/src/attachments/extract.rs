//! Text extraction from attachments for search (FR-EML-007, ADR-0033).
//!
//! Pure, synchronous and bounded: the caller runs [`extract_text`] in
//! `tokio::task::spawn_blocking` with a timeout. Nothing in an attachment is ever
//! executed or followed (no macros, scripts, external entities or links). Supported:
//! plain text, HTML (tags, scripts and styles removed with `ammonia`) and Office Open
//! XML documents (`.docx`, `.xlsx`, `.pptx`). Everything else, including PDF and images,
//! is `unsupported`.

use std::io::{Cursor, Read};

/// How much work one extraction may do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExtractionLimits {
    /// Larger attachments are not read at all (`too_large`).
    pub max_input_bytes: u64,
    /// Text kept per attachment; longer text is cut (`truncated`).
    pub max_chars: usize,
    /// Total bytes decompressed from an Office document (zip bomb guard).
    pub max_unpacked_bytes: u64,
    /// Wall-clock limit of one extraction.
    pub timeout: std::time::Duration,
}

impl Default for ExtractionLimits {
    fn default() -> Self {
        Self {
            max_input_bytes: 20 * 1024 * 1024,
            max_chars: 100_000,
            max_unpacked_bytes: 64 * 1024 * 1024,
            timeout: std::time::Duration::from_secs(30),
        }
    }
}

/// What reading one attachment gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Extraction {
    /// Text (normalised whitespace), cut at the limit when `truncated`.
    Text {
        /// The text.
        text: String,
        /// Whether text was cut.
        truncated: bool,
    },
    /// The format is not read.
    Unsupported,
    /// The content could not be read (corrupt, encrypted, over the unpack limit).
    Failed(String),
}

/// Extracts searchable text from `bytes`. `media_type` is the type sniffed from the
/// content when it was stored; `file_name` only tells HTML from plain text.
#[must_use]
pub fn extract_text(
    bytes: &[u8],
    media_type: &str,
    file_name: &str,
    limits: &ExtractionLimits,
) -> Extraction {
    let lower = file_name.to_ascii_lowercase();
    match media_type {
        "text/plain" => {
            let text = String::from_utf8_lossy(bytes);
            let head = text
                .trim_start()
                .get(..15)
                .unwrap_or_default()
                .to_ascii_lowercase();
            if lower.ends_with(".html")
                || lower.ends_with(".htm")
                || head.starts_with("<!doctype html")
                || head.starts_with("<html")
            {
                finish(&html_to_text(&text), limits)
            } else {
                finish(&text, limits)
            }
        }
        "application/zip" => office_text(bytes, limits),
        _ => Extraction::Unsupported,
    }
}

fn finish(text: &str, limits: &ExtractionLimits) -> Extraction {
    let mut out = String::new();
    let mut chars = 0usize;
    let mut truncated = false;
    for word in text.split_whitespace() {
        let needed = word.chars().count() + usize::from(!out.is_empty());
        if chars + needed > limits.max_chars {
            truncated = true;
            break;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
        chars += needed;
    }
    Extraction::Text {
        text: out,
        truncated,
    }
}

/// HTML to text: tags removed, `<script>` and `<style>` dropped with their content,
/// entities decoded.
#[must_use]
pub fn html_to_text(html: &str) -> String {
    // A space before every tag keeps words in adjacent blocks apart.
    let spaced = html.replace('<', " <");
    let cleaned = ammonia::Builder::empty().clean(&spaced).to_string();
    unescape(&cleaned)
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
}

/// XML text: paragraph, row and cell ends become spaces, other tags vanish.
fn xml_text(xml: &str) -> String {
    let mut out = String::with_capacity(xml.len() / 2);
    let mut rest = xml;
    while let Some(start) = rest.find('<') {
        out.push_str(&rest[..start]);
        let Some(end) = rest[start..].find('>') else {
            rest = "";
            break;
        };
        let tag = &rest[start + 1..start + end];
        if tag.starts_with('/') || tag.ends_with('/') {
            out.push(' ');
        }
        rest = &rest[start + end + 1..];
    }
    out.push_str(rest);
    unescape(&out)
}

/// Parts of an Office Open XML package that hold its text.
fn is_text_part(name: &str) -> bool {
    name == "word/document.xml"
        || name.starts_with("word/header") && name.ends_with(".xml")
        || name.starts_with("word/footer") && name.ends_with(".xml")
        || name == "word/footnotes.xml"
        || name == "xl/sharedStrings.xml"
        || name.starts_with("ppt/slides/slide") && name.ends_with(".xml")
}

fn office_text(bytes: &[u8], limits: &ExtractionLimits) -> Extraction {
    let Ok(mut archive) = zip::ZipArchive::new(Cursor::new(bytes)) else {
        return Extraction::Failed("not a readable zip archive".into());
    };
    let mut names: Vec<String> = archive
        .file_names()
        .filter_map(Result::ok)
        .filter(|n| is_text_part(n))
        .map(std::borrow::Cow::into_owned)
        .collect();
    if names.is_empty() {
        // A zip, but not a Word, Excel or PowerPoint document.
        return Extraction::Unsupported;
    }
    names.sort_by_key(|n| natural_key(n));
    let mut budget = limits.max_unpacked_bytes;
    let mut text = String::new();
    for name in names {
        let Ok(part) = archive.by_name(&name) else {
            return Extraction::Failed(format!("cannot read {name}"));
        };
        let mut buf = Vec::new();
        // One byte over the budget tells a part that is too big from one that fits.
        if part.take(budget + 1).read_to_end(&mut buf).is_err() {
            return Extraction::Failed(format!("cannot unpack {name}"));
        }
        if buf.len() as u64 > budget {
            return Extraction::Failed("the document unpacks to more than the limit".into());
        }
        budget -= buf.len() as u64;
        text.push_str(&xml_text(&String::from_utf8_lossy(&buf)));
        text.push(' ');
    }
    finish(&text, limits)
}

/// Sorts `slide2.xml` before `slide10.xml`.
fn natural_key(name: &str) -> (String, u32) {
    let stem = name.strip_suffix(".xml").unwrap_or(name);
    let split = stem.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    (stem[..split].to_owned(), stem[split..].parse().unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    fn limits() -> ExtractionLimits {
        ExtractionLimits {
            max_chars: 40,
            max_unpacked_bytes: 4096,
            ..ExtractionLimits::default()
        }
    }

    fn zip(parts: &[(&str, &str)]) -> Vec<u8> {
        let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, content) in parts {
            w.start_file(
                *name,
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated),
            )
            .unwrap();
            w.write_all(content.as_bytes()).unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    #[test]
    fn plain_text_is_normalised_and_truncated_at_a_word() {
        assert_eq!(
            extract_text(b"Printer  on\nfloor 3", "text/plain", "log.txt", &limits()),
            Extraction::Text {
                text: "Printer on floor 3".into(),
                truncated: false
            }
        );
        let long = "word ".repeat(20);
        let Extraction::Text { text, truncated } =
            extract_text(long.as_bytes(), "text/plain", "a.txt", &limits())
        else {
            panic!("text expected");
        };
        assert!(truncated);
        assert!(text.chars().count() <= 40 && text.ends_with("word"));
    }

    #[test]
    fn html_loses_tags_scripts_and_styles() {
        let html = "<html><head><style>p{color:red}</style><script>alert('x')</script></head>\
                    <body><p>VPN&nbsp;fails</p><p>since &lt;Monday&gt; &amp; today</p></body></html>";
        let Extraction::Text { text, .. } = extract_text(
            html.as_bytes(),
            "text/plain",
            "mail.html",
            &ExtractionLimits::default(),
        ) else {
            panic!("text expected");
        };
        assert_eq!(text, "VPN fails since <Monday> & today");
    }

    #[test]
    fn office_documents_are_read_in_order() {
        let docx = zip(&[
            ("[Content_Types].xml", "<Types/>"),
            (
                "word/document.xml",
                "<w:document><w:body><w:p><w:r><w:t>Quarterly</w:t></w:r></w:p><w:p><w:r><w:t>budget &amp; plan</w:t></w:r></w:p></w:body></w:document>",
            ),
        ]);
        assert_eq!(
            extract_text(&docx, "application/zip", "plan.docx", &limits()),
            Extraction::Text {
                text: "Quarterly budget & plan".into(),
                truncated: false
            }
        );
        let pptx = zip(&[
            ("ppt/slides/slide10.xml", "<p:sld><a:t>ten</a:t></p:sld>"),
            ("ppt/slides/slide2.xml", "<p:sld><a:t>two</a:t></p:sld>"),
        ]);
        assert_eq!(
            extract_text(&pptx, "application/zip", "deck.pptx", &limits()),
            Extraction::Text {
                text: "two ten".into(),
                truncated: false
            }
        );
    }

    #[test]
    fn unknown_formats_bombs_and_corrupt_files_are_not_read() {
        assert_eq!(
            extract_text(b"%PDF-1.7", "application/pdf", "a.pdf", &limits()),
            Extraction::Unsupported
        );
        assert_eq!(
            extract_text(b"\x89PNG", "image/png", "a.png", &limits()),
            Extraction::Unsupported
        );
        assert_eq!(
            extract_text(
                &zip(&[("readme.txt", "hi")]),
                "application/zip",
                "a.zip",
                &limits()
            ),
            Extraction::Unsupported
        );
        let bomb = zip(&[("word/document.xml", &"a".repeat(10_000))]);
        assert!(matches!(
            extract_text(&bomb, "application/zip", "bomb.docx", &limits()),
            Extraction::Failed(_)
        ));
        assert!(matches!(
            extract_text(b"PK\x03\x04garbage", "application/zip", "x.docx", &limits()),
            Extraction::Failed(_)
        ));
    }
}
