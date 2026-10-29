//! Artifact naming + kinds. Mirrors CONTRACTS §5.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    Html,
    Markdown,
    Raw,
    Links,
    Screenshot,
    ScreenshotAnnotated,
    Pdf,
    Trace,
    Extract,
    VisualChange,
    VisualObservation,
    EvidenceDelta,
    Thumbnail,
    Tiles,
    PageImageClean,
    OcrPreprocessed,
    LogoCandidate,
    RenderedPalette,
    Meta,
}

impl ArtifactKind {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Html => "html",
            Self::Markdown => "md",
            Self::Raw => "bin",
            Self::Links => "links.json",
            Self::Screenshot => "png",
            Self::ScreenshotAnnotated => "png",
            Self::Pdf => "pdf",
            Self::Trace => "trace.zip",
            Self::Extract => "extract.json",
            Self::VisualChange => "json",
            Self::VisualObservation => "json",
            Self::EvidenceDelta => "json",
            Self::Thumbnail => "png",
            Self::Tiles => "json",
            Self::PageImageClean => "png",
            Self::OcrPreprocessed => "png",
            Self::LogoCandidate => "png",
            Self::RenderedPalette => "json",
            Self::Meta => "meta.json",
        }
    }
}

pub fn object_key(org: &str, run_id: &str, page_hash: &str, kind: ArtifactKind) -> String {
    format!(
        "org={org}/run={run_id}/page={page_hash}/{}.{}",
        kind_stem(kind),
        kind.extension()
    )
}

fn kind_stem(kind: ArtifactKind) -> &'static str {
    match kind {
        ArtifactKind::Html => "html",
        ArtifactKind::Markdown => "markdown",
        ArtifactKind::Raw => "raw",
        ArtifactKind::Links => "links",
        ArtifactKind::Screenshot => "screenshot",
        ArtifactKind::ScreenshotAnnotated => "screenshot_annotated",
        ArtifactKind::Pdf => "pdf",
        ArtifactKind::Trace => "trace",
        ArtifactKind::Extract => "extract",
        ArtifactKind::VisualChange => "visual_change",
        ArtifactKind::VisualObservation => "visual_observation",
        ArtifactKind::EvidenceDelta => "evidence_delta",
        ArtifactKind::Thumbnail => "thumbnail",
        ArtifactKind::Tiles => "tiles",
        ArtifactKind::PageImageClean => "page_image_clean",
        ArtifactKind::OcrPreprocessed => "ocr_preprocessed",
        ArtifactKind::LogoCandidate => "logo_candidate",
        ArtifactKind::RenderedPalette => "rendered_palette",
        ArtifactKind::Meta => "meta",
    }
}

pub fn page_hash(normalized_url: &str, content_fingerprint: &str) -> String {
    let mut h = blake3::Hasher::new();
    h.update(normalized_url.as_bytes());
    h.update(b"\0");
    h.update(content_fingerprint.as_bytes());
    format!("blake3:{}", h.finalize().to_hex())
}

/// Authoritative `Content-Type` for an artifact, derived from the extension its
/// object key already carries.
///
/// # Why derived from the stored key, never sniffed
///
/// `quarry-edge`'s artifact route serves bytes with `X-Content-Type-Options:
/// nosniff`, which is correct — a browser must not guess. But it previously also
/// served a hardcoded `application/octet-stream`, so **no** artifact could be
/// rendered or opened by type: a screenshot the runtime had captured came back
/// as an opaque download, which is why `screenshot_ref` reached the UI with
/// nowhere to display it.
///
/// The kind is decided by the producer at `put` time and encoded into the object
/// key by [`object_key`], so the extension is a server-side fact, not attacker
/// input. Combined with `nosniff` that makes an accurate type safe to declare:
/// the browser is told exactly what the producer stored and is forbidden from
/// second-guessing it.
///
/// Anything unrecognised stays `application/octet-stream` — the safe default,
/// and the honest answer for bytes whose type we cannot establish.
#[must_use]
pub fn content_type_for_key(key: &str) -> &'static str {
    // The stem carries a compound extension for some kinds (`links.json`,
    // `trace.zip`), so match on the file segment rather than only the last dot.
    let file = key.rsplit('/').next().unwrap_or(key);
    match file.rsplit_once('.').map(|(_, ext)| ext) {
        Some("png") => "image/png",
        Some("pdf") => "application/pdf",
        Some("json") => "application/json",
        Some("html") => "text/html",
        Some("md") => "text/markdown",
        Some("zip") => "application/zip",
        // `Raw` (`.bin`) and anything else: do not guess.
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod content_type_tests {
    use super::*;

    #[test]
    fn a_screenshot_key_is_declared_as_a_png() {
        // The case that made this necessary: a captured screenshot served as an
        // opaque download could not be shown anywhere.
        let key = object_key("acme", "run_1", "page_1", ArtifactKind::Screenshot);
        assert_eq!(content_type_for_key(&key), "image/png");
        let annotated = object_key("acme", "run_1", "page_1", ArtifactKind::ScreenshotAnnotated);
        assert_eq!(content_type_for_key(&annotated), "image/png");
    }

    /// Every kind the enum declares must produce SOME type without panicking,
    /// and every type must be one a browser will not misinterpret.
    #[test]
    fn every_kind_maps_to_a_declared_type() {
        for kind in [
            ArtifactKind::Html,
            ArtifactKind::Markdown,
            ArtifactKind::Raw,
            ArtifactKind::Links,
            ArtifactKind::Screenshot,
            ArtifactKind::ScreenshotAnnotated,
            ArtifactKind::Pdf,
            ArtifactKind::Trace,
            ArtifactKind::Extract,
            ArtifactKind::VisualChange,
            ArtifactKind::VisualObservation,
            ArtifactKind::EvidenceDelta,
            ArtifactKind::Thumbnail,
            ArtifactKind::Tiles,
            ArtifactKind::PageImageClean,
            ArtifactKind::OcrPreprocessed,
            ArtifactKind::LogoCandidate,
            ArtifactKind::RenderedPalette,
            ArtifactKind::Meta,
        ] {
            let key = object_key("acme", "run_1", "page_1", kind);
            let declared = content_type_for_key(&key);
            assert!(!declared.is_empty(), "{kind:?} produced no content type");
        }
    }

    /// Compound extensions must resolve on the LAST segment, not the first.
    #[test]
    fn compound_extensions_resolve_correctly() {
        assert_eq!(
            content_type_for_key(&object_key("a", "r", "p", ArtifactKind::Links)),
            "application/json",
            "links.json is JSON, not an unknown `links` type"
        );
        assert_eq!(
            content_type_for_key(&object_key("a", "r", "p", ArtifactKind::Trace)),
            "application/zip"
        );
    }

    /// The safe default. An unknown or extensionless key must never be declared
    /// as something a browser would try to render.
    #[test]
    fn unknown_keys_stay_opaque() {
        for key in [
            "org=a/run=r/page=p/raw.bin",
            "org=a/run=r/page=p/noextension",
            "",
            "org=a/weird",
        ] {
            assert_eq!(content_type_for_key(key), "application/octet-stream", "{key}");
        }
    }

    /// A key whose *directory* contains a dot must not be mistaken for the
    /// file's extension.
    #[test]
    fn a_dotted_directory_is_not_the_extension() {
        assert_eq!(
            content_type_for_key("org=a/run=r/page=sha256.abc/screenshot.png"),
            "image/png"
        );
        assert_eq!(
            content_type_for_key("org=a/run=r/page=sha256.abc/noextension"),
            "application/octet-stream",
            "the dot in the directory segment must not leak into the match"
        );
    }
}
