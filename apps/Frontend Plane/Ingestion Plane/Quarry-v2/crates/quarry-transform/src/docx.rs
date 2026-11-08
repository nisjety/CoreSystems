//! DOCX text extraction (OSS-parity P2 3C).
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
/// A DOCX that unpacks past this many bytes of WordML is a zip bomb, not a
/// document: a 100 KB file can inflate to gigabytes.
const MAX_DOCUMENT_XML_BYTES: u64 = 32 * 1024 * 1024;
/// More entries than any real document has.
const MAX_DOCX_ENTRIES: usize = 4096;
/// Most bytes all the parts of one DOCX may declare together (the sum of their
/// uncompressed sizes, read from the central directory). A cap per part is not
/// enough: a bomb can sit in a part the extractor never reads.
const MAX_DOCX_DECLARED_BYTES: u64 = 256 * 1024 * 1024;

/// Extract plain text from DOCX bytes.
pub fn extract_text(body: &[u8]) -> QuarryResult<String> {
    if body.len() > MAX_DOCX_BYTES {
        return Err(QuarryError::new(
            ErrorCode::BadRequest,
            format!("docx exceeds {MAX_DOCX_BYTES} bytes"),
        ));
    }
    let mut zip = zip::ZipArchive::new(Cursor::new(body)).map_err(|e| {
        QuarryError::new(
            ErrorCode::BadRequest,
            format!("not a valid docx (zip): {e}"),
        )
    })?;
    if zip.len() > MAX_DOCX_ENTRIES {
        return Err(QuarryError::new(
            ErrorCode::BadRequest,
            format!(
                "docx has {} entries, which exceeds {MAX_DOCX_ENTRIES}",
                zip.len()
            ),
        ));
    }
    // The declared sizes come from the central directory: nothing is inflated yet.
    // A part that declares a huge word/document.xml is refused here, and the parts
    // together are capped, so a bomb hidden in a part the extractor never reads is
    // refused too. The streamed cap below stays the real limit: a declared size can lie.
    let mut declared_total: u64 = 0;
    for index in 0..zip.len() {
        let part = zip.by_index_raw(index).map_err(|e| {
            QuarryError::new(
                ErrorCode::BadRequest,
                format!("not a valid docx (zip): {e}"),
            )
        })?;
        if part.name() == "word/document.xml" && part.size() > MAX_DOCUMENT_XML_BYTES {
            return Err(QuarryError::new(
                ErrorCode::BadRequest,
                format!(
                    "docx word/document.xml declares {} bytes, which exceeds {MAX_DOCUMENT_XML_BYTES}",
                    part.size()
                ),
            ));
        }
        declared_total = declared_total.saturating_add(part.size());
    }
    if declared_total > MAX_DOCX_DECLARED_BYTES {
        return Err(QuarryError::new(
            ErrorCode::BadRequest,
            format!(
                "docx parts declare {declared_total} bytes in total, which exceeds {MAX_DOCX_DECLARED_BYTES}"
            ),
        ));
    }
    let mut xml = String::new();
    {
        let f = zip.by_name("word/document.xml").map_err(|e| {
            QuarryError::new(
                ErrorCode::BadRequest,
                format!("docx missing word/document.xml: {e}"),
            )
        })?;
        // Read one byte past the cap so "exactly at the cap" is not mistaken
        // for "over it"; decompression is streamed, so a bomb stops here.
        f.take(MAX_DOCUMENT_XML_BYTES + 1)
            .read_to_string(&mut xml)
            .map_err(|e| {
                QuarryError::new(ErrorCode::Internal, format!("read document.xml: {e}"))
            })?;
    }
    if xml.len() as u64 > MAX_DOCUMENT_XML_BYTES {
        return Err(QuarryError::new(
            ErrorCode::BadRequest,
            format!("docx word/document.xml exceeds {MAX_DOCUMENT_XML_BYTES} bytes"),
        ));
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
        let name = tag
            .split_whitespace()
            .next()
            .unwrap_or("")
            .trim_end_matches('/');
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
            let opts: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
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
    fn an_inflating_document_xml_is_rejected_at_the_cap() {
        // ~33 MiB of zeros deflates to a few KiB.
        let mut payload = b"<w:document><w:body><w:p><w:r><w:t>".to_vec();
        payload.resize(MAX_DOCUMENT_XML_BYTES as usize + 1024, b'a');
        let mut buf = Vec::new();
        {
            let mut zw = zip::ZipWriter::new(Cursor::new(&mut buf));
            let opts: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            zw.start_file("word/document.xml", opts).unwrap();
            zw.write_all(&payload).unwrap();
            zw.finish().unwrap();
        }
        assert!(
            buf.len() < 256 * 1024,
            "the bomb itself is small: {}",
            buf.len()
        );
        let error = extract_text(&buf).unwrap_err();
        assert_eq!(error.code, ErrorCode::BadRequest);
        assert!(error.message.contains("exceeds"), "{}", error.message);
    }

    #[test]
    fn an_archive_with_thousands_of_entries_is_rejected() {
        let mut buf = Vec::new();
        {
            let mut zw = zip::ZipWriter::new(Cursor::new(&mut buf));
            let opts: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
            for i in 0..MAX_DOCX_ENTRIES + 5 {
                zw.start_file(format!("f{i}.txt"), opts).unwrap();
                zw.write_all(b"x").unwrap();
            }
            zw.finish().unwrap();
        }
        let error = extract_text(&buf).unwrap_err();
        assert_eq!(error.code, ErrorCode::BadRequest);
        assert!(error.message.contains("entries"), "{}", error.message);
    }

    /// A zip holding `parts` as `(name, bytes)`, each deflated.
    fn docx_with_parts(parts: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut zw = zip::ZipWriter::new(Cursor::new(&mut buf));
            let opts: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            for (name, bytes) in parts {
                zw.start_file(*name, opts).unwrap();
                zw.write_all(bytes).unwrap();
            }
            zw.finish().unwrap();
        }
        buf
    }

    const BODY_XML: &[u8] = br#"<w:document><w:body><w:p><w:r><w:t>Contract text</w:t></w:r></w:p></w:body></w:document>"#;

    #[test]
    fn parts_that_declare_too_much_in_total_are_refused_before_any_is_inflated() {
        // Two 150 MiB parts beside a real body: 300 MiB declared, past the 256 MiB total.
        // The body is read from its own part, so the total is what refuses this.
        let bomb_a = vec![0u8; 150 * 1024 * 1024];
        let bomb_b = vec![0u8; 150 * 1024 * 1024];
        let docx = docx_with_parts(&[
            ("word/document.xml", BODY_XML),
            ("word/embeddings/one.bin", bomb_a.as_slice()),
            ("word/embeddings/two.bin", bomb_b.as_slice()),
        ]);
        assert!(docx.len() < 4 * 1024 * 1024, "the bomb is small: {}", docx.len());
        let error = extract_text(&docx).expect_err("the parts together declare too much");
        assert_eq!(error.code, ErrorCode::BadRequest);
        assert!(error.message.contains("in total"), "{}", error.message);
    }

    #[test]
    fn a_document_xml_that_declares_too_much_is_refused_before_it_is_read() {
        // 33 MiB of zeros in a deflated part: the declared size alone refuses it.
        let payload = vec![0u8; MAX_DOCUMENT_XML_BYTES as usize + 1024];
        let docx = docx_with_parts(&[("word/document.xml", payload.as_slice())]);
        let error = extract_text(&docx).expect_err("the declared size is past the cap");
        assert_eq!(error.code, ErrorCode::BadRequest);
        assert!(error.message.contains("declares"), "{}", error.message);
    }

    #[test]
    fn a_docx_with_a_media_part_within_the_budgets_still_extracts() {
        // A real document carries media parts; they count toward the total but not toward
        // the body's cap, and a document of this size is well within both.
        let media: Vec<u8> = (0..2 * 1024 * 1024u32).map(|i| (i.wrapping_mul(31) % 251) as u8).collect();
        let docx = docx_with_parts(&[("word/document.xml", BODY_XML), ("word/media/image1.png", media.as_slice())]);
        assert_eq!(extract_text(&docx).expect("within the budgets"), "Contract text");
    }

    #[test]
    fn non_docx_bytes_error() {
        let err = extract_text(b"not a zip").unwrap_err();
        assert_eq!(err.code, ErrorCode::BadRequest);
    }
}
