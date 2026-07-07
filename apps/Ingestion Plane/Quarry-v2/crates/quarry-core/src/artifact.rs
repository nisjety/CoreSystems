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
