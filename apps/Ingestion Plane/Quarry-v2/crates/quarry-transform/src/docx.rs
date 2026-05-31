//! DOCX text extraction (OSS-parity P2 3C) — Firecrawl media-parsing parity.
//!
//! A `.docx` is a ZIP container; the body lives in `word/document.xml` as
//! WordML. We unzip that part and strip it to plain text: `<w:t>` runs become
//! text, `</w:p>` paragraph ends + `<w:br>` become newlines. Pure-logic strip
//! (no XML lib needed) keeps the surface small and dependency-light.

use std::io::{Cursor, Read};

use quarry_core::{
    error::{ErrorCode, QuarryError},
    QuarryResult,
};

const MAX_DOCX_BYTES: usize = 25 * 1024 * 1024;

/// Extract plain text from DOCX bytes.
pub fn extract_text(body: &[u8]) -> QuarryResult<String> {
    if body.len() > MAX_DOCX_BYTES {
        return Err(QuarryError::new(
            ErrorCode::BadRequest,
            format!("docx exceeds {MAX_DOCX_BYTES} bytes"),
        ));
    }
    let mut zip = zip::ZipArchive::new(Cursor::new(body)).map_err(|e| {
        QuarryError::new(ErrorCode::BadRequest, format!("not a valid docx (zip): {e}"))
    })?;
    let mut xml = String::new();
    {
        let mut f = zip.by_name("word/document.xml").map_err(|e| {
            QuarryError::new(ErrorCode::BadRequest, format!("docx missing word/document.xml: {e}"))
        })?;
        f.read_to_string(&mut xml)
            .map_err(|e| QuarryError::new(ErrorCode::Internal, format!("read document.xml: {e}")))?;
    }
    Ok(strip_wordml(&xml))
}

/// Strip WordML to plain text. `<w:t>` content is emitted; `</w:p>` and
/// `<w:br>`/`<w:cr>` become newlines; `<w:tab>` becomes a tab. Excess blank
/// lines are collapsed.
pub(crate) fn strip_wordml(xml: &str) -> String {
    let mut out = String::new();
    let mut in_text = false;
    let mut rest = xml;
    while let Some(lt) = rest.find('<') {
        if in_text {
            out.push_str(&unescape_xml(&rest[..lt]));
        }
        rest = &rest[lt..];
        let Some(gt) = rest.find('>') else { break };
        let tag = &rest[1..gt];
        let name = tag.split_whitespace().next().unwrap_or("").trim_end_matches('/');
        match name {
            "w:t" => in_text = true,
            "/w:t" => in_text = false,
            "/w:p" => out.push('\n'),
            "w:br" | "w:cr" => out.push('\n'),
            "w:tab" => out.push('\t'),
            _ => {}
        }
        rest = &rest[gt + 1..];
    }
    collapse_blank_lines(out.trim())
}

fn unescape_xml(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

fn collapse_blank_lines(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut newlines = 0;
    for ch in s.chars() {
        if ch == '\n' {
            newlines += 1;
            if newlines <= 2 {
                out.push('\n');
            }
        } else {
            newlines = 0;
            out.push(ch);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn strips_runs_and_paragraphs() {
        let xml = r#"<w:document><w:body>
            <w:p><w:r><w:t>Hello</w:t></w:r><w:r><w:t xml:space="preserve"> world</w:t></w:r></w:p>
            <w:p><w:r><w:t>Second para</w:t></w:r></w:p>
            </w:body></w:document>"#;
        let text = strip_wordml(xml);
        assert!(text.contains("Hello world"));
        assert!(text.contains("Second para"));
        assert_eq!(text.lines().count(), 2);
    }

    #[test]
    fn unescapes_entities() {
        let xml = "<w:t>A &amp; B &lt;tag&gt;</w:t>";
        assert_eq!(strip_wordml(xml), "A & B <tag>");
    }

    #[test]
    fn br_becomes_newline() {
        let xml = "<w:t>line1</w:t><w:br/><w:t>line2</w:t>";
        assert_eq!(strip_wordml(xml), "line1\nline2");
    }

    #[test]
    fn extract_text_roundtrip_from_zip() {
        // Build a minimal in-memory .docx (zip with word/document.xml).
        let mut buf = Vec::new();
        {
            let mut zw = zip::ZipWriter::new(Cursor::new(&mut buf));
            let opts: zip::write::FileOptions<'_, ()> =
                zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            zw.start_file("word/document.xml", opts).unwrap();
            zw.write_all(
                br#"<w:document><w:body><w:p><w:r><w:t>Contract text</w:t></w:r></w:p></w:body></w:document>"#,
            )
            .unwrap();
            zw.finish().unwrap();
        }
        let text = extract_text(&buf).unwrap();
        assert_eq!(text, "Contract text");
    }

    #[test]
    fn non_docx_bytes_error() {
        let err = extract_text(b"not a zip").unwrap_err();
        assert_eq!(err.code, ErrorCode::BadRequest);
    }
}
