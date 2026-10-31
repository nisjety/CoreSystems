//! Content-kind detection from `Content-Type` and the first bytes of a body.
//!
//! The scrape pipeline used to run charset decoding and readability over every
//! body, so a PDF URL produced mojibake "markdown". A large share of
//! public-sector document links are served as `application/octet-stream`, or
//! from a misconfigured handler as `text/html`, with the real type visible only
//! in the body, so the bytes decide first and the header is the fallback:
//!
//! 1. magic bytes in the first [`SNIFF_WINDOW`] bytes (a ZIP container is
//!    classified through its central directory and a bounded
//!    `[Content_Types].xml`, never by extension);
//! 2. the declared `Content-Type`;
//! 3. a default: valid UTF-8 without NUL bytes stays on the text path, anything
//!    else is [`ContentKind::Binary`]; a response with no header at all is HTML,
//!    as it always was.
//!
//! Detection is pure and allocation-light; the only parsing is the bounded ZIP
//! directory read.

use std::io::{Cursor, Read};

use serde::Serialize;

/// How far into the body magic numbers are looked for.
pub const SNIFF_WINDOW: usize = 1024;
/// Upper bound on bytes read from a container's `[Content_Types].xml` or
/// `mimetype` entry while classifying it.
pub const CONTENT_TYPES_MAX_BYTES: u64 = 256 * 1024;
/// A ZIP with more entries than this is an archive, not a document.
pub const ZIP_ENTRY_LIMIT: usize = 4096;

/// What a response body is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentKind {
    Html,
    Xml,
    Text,
    Json,
    Pdf,
    Docx,
    Xlsx,
    Pptx,
    /// OpenDocument (odt, ods, odp).
    Odf,
    Epub,
    /// Legacy OLE2 compound file (doc, xls, ppt).
    OleCompound,
    Image,
    Audio,
    Video,
    /// zip, gzip, tar, 7z, rar and any container that is not a known document.
    Archive,
    Executable,
    Rtf,
    /// Not text and not recognised.
    Binary,
}

impl ContentKind {
    /// The wire label (`snake_case`), as written to `metadata.content_kind`.
    pub fn label(self) -> &'static str {
        match self {
            Self::Html => "html",
            Self::Xml => "xml",
            Self::Text => "text",
            Self::Json => "json",
            Self::Pdf => "pdf",
            Self::Docx => "docx",
            Self::Xlsx => "xlsx",
            Self::Pptx => "pptx",
            Self::Odf => "odf",
            Self::Epub => "epub",
            Self::OleCompound => "ole_compound",
            Self::Image => "image",
            Self::Audio => "audio",
            Self::Video => "video",
            Self::Archive => "archive",
            Self::Executable => "executable",
            Self::Rtf => "rtf",
            Self::Binary => "binary",
        }
    }

    /// Kinds the HTML/text pipeline (charset decode, readability) is meant for.
    pub fn is_text_pipeline(self) -> bool {
        matches!(self, Self::Html | Self::Xml | Self::Text | Self::Json)
    }

    /// Kinds Quarry has a text extractor for.
    pub fn is_parseable_document(self) -> bool {
        matches!(self, Self::Pdf | Self::Docx)
    }
}

/// Where the verdict came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum KindSource {
    Header,
    Magic,
    Default,
}

impl KindSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::Header => "header",
            Self::Magic => "magic",
            Self::Default => "default",
        }
    }
}

/// A detection result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Detected {
    pub kind: ContentKind,
    pub source: KindSource,
    /// A finer label where one is known (`png`, `gzip`, `zip`, ...).
    pub subtype: Option<&'static str>,
}

impl Detected {
    fn magic(kind: ContentKind, subtype: Option<&'static str>) -> Self {
        Self {
            kind,
            source: KindSource::Magic,
            subtype,
        }
    }
    fn header(kind: ContentKind, subtype: Option<&'static str>) -> Self {
        Self {
            kind,
            source: KindSource::Header,
            subtype,
        }
    }
    fn default_kind(kind: ContentKind) -> Self {
        Self {
            kind,
            source: KindSource::Default,
            subtype: None,
        }
    }
}

/// Detect what `body` is. `content_type` is the raw header value, if any.
pub fn detect(content_type: Option<&str>, body: &[u8]) -> Detected {
    let head = &body[..body.len().min(SNIFF_WINDOW)];
    if let Some(found) = detect_magic(head, body) {
        return found;
    }
    if let Some(found) = detect_header(content_type) {
        return found;
    }
    // No magic and no usable header: text stays on the text path (a missing
    // header is HTML, as it always was), anything with NUL bytes is binary.
    if looks_like_text(head) {
        let kind = if content_type.is_none() {
            ContentKind::Html
        } else {
            ContentKind::Text
        };
        Detected::default_kind(kind)
    } else {
        Detected::default_kind(ContentKind::Binary)
    }
}

/// True when the body is a PDF: a `%PDF-` marker in the first kilobyte (real
/// exporters prepend junk and Acrobat accepts it there too) that is not inside
/// an HTML or XML document, or a declared `application/pdf`.
pub fn is_pdf(content_type: Option<&str>, body: &[u8]) -> bool {
    let declared = content_type
        .map(|ct| ct.to_ascii_lowercase().contains("application/pdf"))
        .unwrap_or(false);
    declared || has_pdf_marker(&body[..body.len().min(SNIFF_WINDOW)])
}

fn has_pdf_marker(head: &[u8]) -> bool {
    if !head.windows(5).any(|window| window == b"%PDF-") {
        return false;
    }
    // A markup document that merely contains the literal (an HTML page about
    // PDFs) starts with `<` once leading whitespace is skipped.
    let first = head.iter().find(|byte| !byte.is_ascii_whitespace());
    first != Some(&b'<')
}

fn detect_magic(head: &[u8], body: &[u8]) -> Option<Detected> {
    if has_pdf_marker(head) {
        return Some(Detected::magic(ContentKind::Pdf, None));
    }
    if head.starts_with(b"PK\x03\x04") {
        return Some(classify_zip(body));
    }
    if head.starts_with(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]) {
        return Some(Detected::magic(ContentKind::OleCompound, None));
    }
    if head.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some(Detected::magic(ContentKind::Image, Some("png")));
    }
    if head.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some(Detected::magic(ContentKind::Image, Some("jpeg")));
    }
    if head.starts_with(b"GIF87a") || head.starts_with(b"GIF89a") {
        return Some(Detected::magic(ContentKind::Image, Some("gif")));
    }
    if head.len() >= 12 && &head[..4] == b"RIFF" {
        match &head[8..12] {
            b"WEBP" => return Some(Detected::magic(ContentKind::Image, Some("webp"))),
            b"WAVE" => return Some(Detected::magic(ContentKind::Audio, Some("wav"))),
            b"AVI " => return Some(Detected::magic(ContentKind::Video, Some("avi"))),
            _ => {}
        }
    }
    if head.starts_with(b"ID3") {
        return Some(Detected::magic(ContentKind::Audio, Some("mp3")));
    }
    if head.starts_with(b"OggS") {
        return Some(Detected::magic(ContentKind::Audio, Some("ogg")));
    }
    if head.starts_with(b"fLaC") {
        return Some(Detected::magic(ContentKind::Audio, Some("flac")));
    }
    if head.len() >= 12 && &head[4..8] == b"ftyp" {
        return Some(Detected::magic(ContentKind::Video, Some("mp4")));
    }
    if head.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        return Some(Detected::magic(ContentKind::Video, Some("matroska")));
    }
    if head.starts_with(&[0x1F, 0x8B]) {
        return Some(Detected::magic(ContentKind::Archive, Some("gzip")));
    }
    if head.starts_with(b"BZh") && head.get(3).is_some_and(|b| (b'1'..=b'9').contains(b)) {
        return Some(Detected::magic(ContentKind::Archive, Some("bzip2")));
    }
    if head.starts_with(&[0xFD, b'7', b'z', b'X', b'Z', 0x00]) {
        return Some(Detected::magic(ContentKind::Archive, Some("xz")));
    }
    if head.starts_with(&[b'7', b'z', 0xBC, 0xAF, 0x27, 0x1C]) {
        return Some(Detected::magic(ContentKind::Archive, Some("7z")));
    }
    if head.starts_with(b"Rar!\x1a\x07") {
        return Some(Detected::magic(ContentKind::Archive, Some("rar")));
    }
    if head.len() > 262 && &head[257..262] == b"ustar" {
        return Some(Detected::magic(ContentKind::Archive, Some("tar")));
    }
    if head.starts_with(b"\x7fELF") {
        return Some(Detected::magic(ContentKind::Executable, Some("elf")));
    }
    // `MZ` is two letters a text file can start with; a DOS/PE header also has
    // NUL bytes in its first 64 bytes.
    if head.starts_with(b"MZ") && head.iter().take(64).any(|byte| *byte == 0) {
        return Some(Detected::magic(ContentKind::Executable, Some("pe")));
    }
    if head.starts_with(b"{\\rtf") {
        return Some(Detected::magic(ContentKind::Rtf, None));
    }
    None
}

/// Classify a `PK\x03\x04` container through its central directory.
fn classify_zip(body: &[u8]) -> Detected {
    let archive = || Detected::magic(ContentKind::Archive, Some("zip"));
    let mut zip = match zip::ZipArchive::new(Cursor::new(body)) {
        Ok(zip) => zip,
        // Truncated or damaged: not a document we can open.
        Err(_) => return archive(),
    };
    if zip.len() > ZIP_ENTRY_LIMIT {
        return archive();
    }
    if let Some(types) = read_entry(&mut zip, "[Content_Types].xml") {
        let kind = if types.contains("wordprocessingml") {
            Some((ContentKind::Docx, "docx"))
        } else if types.contains("spreadsheetml") {
            Some((ContentKind::Xlsx, "xlsx"))
        } else if types.contains("presentationml") {
            Some((ContentKind::Pptx, "pptx"))
        } else {
            None
        };
        return match kind {
            Some((kind, subtype)) => Detected::magic(kind, Some(subtype)),
            None => archive(),
        };
    }
    if let Some(mimetype) = read_entry(&mut zip, "mimetype") {
        let mimetype = mimetype.trim();
        if mimetype.starts_with("application/vnd.oasis.opendocument") {
            return Detected::magic(ContentKind::Odf, Some("odf"));
        }
        if mimetype == "application/epub+zip" {
            return Detected::magic(ContentKind::Epub, Some("epub"));
        }
    }
    archive()
}

fn read_entry(zip: &mut zip::ZipArchive<Cursor<&[u8]>>, name: &str) -> Option<String> {
    let file = zip.by_name(name).ok()?;
    let mut text = String::new();
    // `take` bounds what a hostile entry can make us read; decompression is
    // streamed, so a bomb stops at the cap too.
    file.take(CONTENT_TYPES_MAX_BYTES)
        .read_to_string(&mut text)
        .ok()?;
    Some(text)
}

fn detect_header(content_type: Option<&str>) -> Option<Detected> {
    let raw = content_type?;
    let mime = raw
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if mime.is_empty() {
        return None;
    }
    let found = match mime.as_str() {
        "text/html" | "application/xhtml+xml" => Detected::header(ContentKind::Html, None),
        "application/pdf" | "application/x-pdf" => Detected::header(ContentKind::Pdf, None),
        "application/json" | "text/json" => Detected::header(ContentKind::Json, None),
        "application/xml" | "text/xml" => Detected::header(ContentKind::Xml, None),
        "application/rtf" | "text/rtf" => Detected::header(ContentKind::Rtf, None),
        "application/epub+zip" => Detected::header(ContentKind::Epub, Some("epub")),
        "application/msword" | "application/vnd.ms-excel" | "application/vnd.ms-powerpoint" => {
            Detected::header(ContentKind::OleCompound, None)
        }
        "application/zip"
        | "application/x-zip-compressed"
        | "application/gzip"
        | "application/x-gzip"
        | "application/x-tar"
        | "application/x-7z-compressed"
        | "application/x-rar-compressed"
        | "application/vnd.rar"
        | "application/x-bzip2"
        | "application/x-xz" => Detected::header(ContentKind::Archive, None),
        "application/x-msdownload" | "application/x-dosexec" | "application/x-executable" => {
            Detected::header(ContentKind::Executable, None)
        }
        // A generic or missing-detail type says nothing: look at the bytes.
        "application/octet-stream" | "binary/octet-stream" | "application/x-binary" => return None,
        other
            if other
                .starts_with("application/vnd.openxmlformats-officedocument.wordprocessingml") =>
        {
            Detected::header(ContentKind::Docx, Some("docx"))
        }
        other
            if other.starts_with("application/vnd.openxmlformats-officedocument.spreadsheetml") =>
        {
            Detected::header(ContentKind::Xlsx, Some("xlsx"))
        }
        other
            if other
                .starts_with("application/vnd.openxmlformats-officedocument.presentationml") =>
        {
            Detected::header(ContentKind::Pptx, Some("pptx"))
        }
        other if other.starts_with("application/vnd.oasis.opendocument") => {
            Detected::header(ContentKind::Odf, Some("odf"))
        }
        other if other.starts_with("image/") && other != "image/svg+xml" => {
            Detected::header(ContentKind::Image, None)
        }
        "image/svg+xml" => Detected::header(ContentKind::Xml, Some("svg")),
        other if other.starts_with("audio/") => Detected::header(ContentKind::Audio, None),
        other if other.starts_with("video/") => Detected::header(ContentKind::Video, None),
        other if other.ends_with("+json") => Detected::header(ContentKind::Json, None),
        other if other.ends_with("+xml") => Detected::header(ContentKind::Xml, None),
        other if other.starts_with("text/") => Detected::header(ContentKind::Text, None),
        _ => return None,
    };
    Some(found)
}

/// True when the sniffed prefix has no NUL byte and is valid UTF-8 apart from a
/// multi-byte character cut by the window.
fn looks_like_text(head: &[u8]) -> bool {
    if head.contains(&0) {
        return false;
    }
    match std::str::from_utf8(head) {
        Ok(_) => true,
        Err(error) => {
            // A character split at the window edge is fine; anything earlier is
            // not UTF-8. Latin-1 pages still pass: they contain no NULs and the
            // charset stage handles them, so only reject when a run of control
            // bytes makes it clearly binary.
            error.error_len().is_none()
                || head
                    .iter()
                    .filter(|b| **b < 0x09 || (0x0E..0x20).contains(*b))
                    .count()
                    == 0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn kind(content_type: Option<&str>, body: &[u8]) -> ContentKind {
        detect(content_type, body).kind
    }

    fn zip_with(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut zw = zip::ZipWriter::new(Cursor::new(&mut buf));
            let opts: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            for (name, data) in entries {
                zw.start_file(*name, opts).unwrap();
                zw.write_all(data).unwrap();
            }
            zw.finish().unwrap();
        }
        buf
    }

    #[test]
    fn pdf_is_detected_by_header_alone() {
        let d = detect(Some("application/pdf"), b"");
        assert_eq!((d.kind, d.source), (ContentKind::Pdf, KindSource::Header));
        assert_eq!(
            kind(Some("application/pdf; charset=binary"), b""),
            ContentKind::Pdf
        );
    }

    #[test]
    fn pdf_magic_beats_a_wrong_header() {
        for header in [Some("application/octet-stream"), Some("text/html"), None] {
            let d = detect(header, b"%PDF-1.7\n1 0 obj");
            assert_eq!(
                (d.kind, d.source),
                (ContentKind::Pdf, KindSource::Magic),
                "{header:?}"
            );
        }
    }

    #[test]
    fn pdf_with_junk_prefix_inside_the_window_is_a_pdf() {
        let mut body = vec![b'x'; 200];
        body.extend_from_slice(b"%PDF-1.4\n");
        assert_eq!(
            kind(Some("application/octet-stream"), &body),
            ContentKind::Pdf
        );
    }

    #[test]
    fn pdf_marker_after_the_window_is_not_a_pdf() {
        let mut body = vec![b'x'; SNIFF_WINDOW + 10];
        body.extend_from_slice(b"%PDF-1.4\n");
        assert_ne!(
            kind(Some("application/octet-stream"), &body),
            ContentKind::Pdf
        );
    }

    #[test]
    fn html_that_mentions_the_pdf_marker_is_html() {
        let body = b"<html><body>Files start with %PDF- on line one.</body></html>";
        assert_eq!(kind(Some("text/html"), body), ContentKind::Html);
        assert!(!is_pdf(Some("text/html"), body));
        assert!(!is_pdf(
            Some("text/html"),
            b"  \n<!doctype html><p>%PDF-</p>"
        ));
    }

    #[test]
    fn is_pdf_matches_the_legacy_answer_behaviour() {
        assert!(is_pdf(Some("application/pdf"), b""));
        assert!(is_pdf(Some("application/octet-stream"), b"%PDF-1.7\n..."));
        assert!(is_pdf(Some("text/html"), b"%PDF-1.4\n..."));
        assert!(is_pdf(None, b"%PDF-1.4\n..."));
        assert!(!is_pdf(Some("text/html"), b"<html><body>hi</body></html>"));
    }

    #[test]
    fn docx_is_confirmed_by_content_types() {
        let docx = zip_with(&[
            (
                "[Content_Types].xml",
                br#"<Types><Override ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#,
            ),
            ("word/document.xml", b"<w:document/>"),
        ]);
        let d = detect(Some("application/octet-stream"), &docx);
        assert_eq!(
            (d.kind, d.source, d.subtype),
            (ContentKind::Docx, KindSource::Magic, Some("docx"))
        );
        assert_eq!(kind(Some("application/zip"), &docx), ContentKind::Docx);
    }

    #[test]
    fn xlsx_and_pptx_are_their_own_kinds() {
        let xlsx = zip_with(&[(
            "[Content_Types].xml",
            b"<Types>spreadsheetml.sheet.main</Types>",
        )]);
        let pptx = zip_with(&[(
            "[Content_Types].xml",
            b"<Types>presentationml.presentation.main</Types>",
        )]);
        assert_eq!(kind(None, &xlsx), ContentKind::Xlsx);
        assert_eq!(kind(None, &pptx), ContentKind::Pptx);
    }

    #[test]
    fn odf_and_epub_are_confirmed_by_mimetype() {
        let odt = zip_with(&[("mimetype", b"application/vnd.oasis.opendocument.text")]);
        let epub = zip_with(&[("mimetype", b"application/epub+zip")]);
        assert_eq!(kind(None, &odt), ContentKind::Odf);
        assert_eq!(kind(None, &epub), ContentKind::Epub);
    }

    #[test]
    fn a_plain_zip_is_an_archive_never_a_docx() {
        let zip = zip_with(&[("a.txt", b"hello"), ("b.txt", b"world")]);
        let d = detect(Some("application/zip"), &zip);
        assert_eq!((d.kind, d.subtype), (ContentKind::Archive, Some("zip")));
        // Even when the header lies and says it is a Word document, the
        // directory decides.
        let lie = detect(
            Some("application/vnd.openxmlformats-officedocument.wordprocessingml.document"),
            &zip,
        );
        assert_eq!(lie.kind, ContentKind::Archive);
    }

    #[test]
    fn a_truncated_zip_is_an_archive() {
        let mut zip = zip_with(&[("a.txt", b"hello world hello world")]);
        zip.truncate(zip.len() / 2);
        assert_eq!(kind(None, &zip), ContentKind::Archive);
    }

    #[test]
    fn a_zip_with_thousands_of_entries_is_an_archive() {
        let names: Vec<String> = (0..ZIP_ENTRY_LIMIT + 5)
            .map(|i| format!("f{i}.txt"))
            .collect();
        let entries: Vec<(&str, &[u8])> = names.iter().map(|n| (n.as_str(), &b"x"[..])).collect();
        let mut all = vec![(
            "[Content_Types].xml",
            &b"<Types>wordprocessingml.document.main</Types>"[..],
        )];
        all.extend(entries);
        assert_eq!(kind(None, &zip_with(&all)), ContentKind::Archive);
    }

    #[test]
    fn images_are_detected_by_magic() {
        assert_eq!(detect(None, b"\x89PNG\r\n\x1a\n....").subtype, Some("png"));
        assert_eq!(
            detect(None, &[0xFF, 0xD8, 0xFF, 0xE0, 0, 0]).subtype,
            Some("jpeg")
        );
        assert_eq!(detect(None, b"GIF89a....").subtype, Some("gif"));
        assert_eq!(
            detect(None, b"RIFF\x00\x00\x00\x00WEBPVP8 ").subtype,
            Some("webp")
        );
        for body in [&b"\x89PNG\r\n\x1a\n"[..], b"GIF87a"] {
            assert_eq!(kind(Some("text/html"), body), ContentKind::Image);
        }
    }

    #[test]
    fn audio_video_archive_executable_rtf_ole() {
        assert_eq!(kind(None, b"ID3\x03\x00"), ContentKind::Audio);
        assert_eq!(kind(None, b"OggS\x00\x02"), ContentKind::Audio);
        assert_eq!(
            kind(None, b"RIFF\x00\x00\x00\x00WAVEfmt "),
            ContentKind::Audio
        );
        assert_eq!(kind(None, b"\x00\x00\x00\x18ftypmp42"), ContentKind::Video);
        assert_eq!(
            kind(None, &[0x1A, 0x45, 0xDF, 0xA3, 0x01]),
            ContentKind::Video
        );
        assert_eq!(kind(None, &[0x1F, 0x8B, 0x08, 0x00]), ContentKind::Archive);
        assert_eq!(kind(None, b"Rar!\x1a\x07\x00"), ContentKind::Archive);
        assert_eq!(kind(None, b"\x7fELF\x02\x01"), ContentKind::Executable);
        assert_eq!(kind(None, b"{\\rtf1\\ansi"), ContentKind::Rtf);
        assert_eq!(
            kind(
                None,
                &[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1, 0, 0]
            ),
            ContentKind::OleCompound
        );
    }

    #[test]
    fn a_pe_header_is_executable_but_text_starting_with_mz_is_not() {
        let mut pe = b"MZ".to_vec();
        pe.extend_from_slice(&[0x90, 0x00, 0x03, 0x00, 0x00, 0x00]);
        assert_eq!(kind(None, &pe), ContentKind::Executable);
        assert_eq!(
            kind(Some("text/plain"), b"MZ is a code for Zimbabwe"),
            ContentKind::Text
        );
    }

    #[test]
    fn headers_decide_when_there_is_no_magic() {
        assert_eq!(
            kind(Some("text/html; charset=utf-8"), b"<p>hi</p>"),
            ContentKind::Html
        );
        assert_eq!(
            kind(Some("application/xhtml+xml"), b"<html/>"),
            ContentKind::Html
        );
        assert_eq!(kind(Some("application/json"), b"{}"), ContentKind::Json);
        assert_eq!(kind(Some("application/ld+json"), b"{}"), ContentKind::Json);
        assert_eq!(
            kind(Some("application/rss+xml"), b"<rss/>"),
            ContentKind::Xml
        );
        assert_eq!(kind(Some("text/plain"), b"hello"), ContentKind::Text);
        assert_eq!(kind(Some("image/svg+xml"), b"<svg/>"), ContentKind::Xml);
        assert_eq!(kind(Some("image/png"), b""), ContentKind::Image);
        assert_eq!(kind(Some("video/mp4"), b""), ContentKind::Video);
        assert_eq!(kind(Some("audio/mpeg"), b""), ContentKind::Audio);
    }

    #[test]
    fn octet_stream_with_text_bytes_stays_on_the_text_path() {
        let d = detect(
            Some("application/octet-stream"),
            b"<html><body>hello</body></html>",
        );
        assert!(d.kind.is_text_pipeline(), "{d:?}");
        assert_eq!(d.source, KindSource::Default);
    }

    #[test]
    fn octet_stream_with_binary_bytes_is_binary() {
        let d = detect(
            Some("application/octet-stream"),
            &[0x00, 0x01, 0x02, 0x03, 0xFE],
        );
        assert_eq!(d.kind, ContentKind::Binary);
    }

    #[test]
    fn a_response_with_no_header_and_html_bytes_is_html() {
        let d = detect(None, b"<!doctype html><title>x</title>");
        assert_eq!((d.kind, d.source), (ContentKind::Html, KindSource::Default));
    }

    #[test]
    fn latin1_pages_stay_on_the_text_path() {
        // 0xE5 is `å` in ISO-8859-1 and invalid UTF-8; the charset stage owns it.
        let body = b"<html><body>Norsk side med \xe5 og \xf8</body></html>";
        assert_eq!(kind(Some("text/html"), body), ContentKind::Html);
        assert!(detect(Some("application/octet-stream"), body)
            .kind
            .is_text_pipeline());
    }

    #[test]
    fn labels_are_stable_wire_values() {
        assert_eq!(ContentKind::OleCompound.label(), "ole_compound");
        assert_eq!(ContentKind::Pdf.label(), "pdf");
        assert_eq!(KindSource::Magic.label(), "magic");
        assert!(ContentKind::Pdf.is_parseable_document());
        assert!(ContentKind::Docx.is_parseable_document());
        assert!(!ContentKind::Xlsx.is_parseable_document());
        assert!(!ContentKind::Image.is_text_pipeline());
    }
}
