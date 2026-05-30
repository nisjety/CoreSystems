//! Minimal metadata extraction (title/lang/content-type hints) and
//! artifact sidecar (`<artifact>.meta.json`) emission.

use std::io;
use std::path::{Path, PathBuf};

use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};

use quarry_core::output::PageMetadata;

pub fn extract(html: &str, content_type: Option<String>) -> PageMetadata {
    let doc = Html::parse_document(html);
    let title = Selector::parse("title")
        .ok()
        .and_then(|s| {
            doc.select(&s)
                .next()
                .map(|n| n.text().collect::<String>().trim().to_string())
        })
        .filter(|t| !t.is_empty());
    let lang = Selector::parse("html[lang]").ok().and_then(|s| {
        doc.select(&s)
            .next()
            .and_then(|n| n.value().attr("lang").map(ToString::to_string))
    });
    PageMetadata {
        title,
        lang,
        content_type,
    }
}

/// Sidecar metadata emitted alongside each transformed artifact.
///
/// Written as `<artifact>.meta.json` next to the artifact file. Captures
/// the deterministic content hash, generation timestamp, the transform
/// chain that produced the artifact, and the originating source URL so
/// downstream consumers can audit provenance without re-running the
/// pipeline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactMeta {
    /// Content hash, e.g. `blake3:<hex>`.
    pub hash: String,
    /// RFC3339 UTC timestamp.
    pub timestamp: String,
    /// Ordered list of transforms applied (e.g. `["html2md", "chunk"]`).
    pub transform_chain: Vec<String>,
    /// Originating source URL.
    pub source_url: String,
}

impl ArtifactMeta {
    /// Build a new `ArtifactMeta` stamped with the current UTC time.
    pub fn new(
        hash: impl Into<String>,
        source_url: impl Into<String>,
        transform_chain: Vec<String>,
    ) -> Self {
        Self {
            hash: hash.into(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            transform_chain,
            source_url: source_url.into(),
        }
    }
}

/// Compute the sidecar path (`<artifact>.meta.json`) for a given artifact path.
pub fn sidecar_path(artifact: impl AsRef<Path>) -> PathBuf {
    let p = artifact.as_ref();
    let mut name = p.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(".meta.json");
    p.with_file_name(name)
}

/// Write `meta` as pretty JSON to `<artifact>.meta.json`.
///
/// Returns the sidecar path on success.
pub fn write_sidecar(artifact: impl AsRef<Path>, meta: &ArtifactMeta) -> io::Result<PathBuf> {
    let path = sidecar_path(&artifact);
    let json = serde_json::to_string_pretty(meta)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    std::fs::write(&path, json)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidecar_path_appends_meta_json() {
        let p = sidecar_path("/tmp/out/page.md");
        assert_eq!(p, PathBuf::from("/tmp/out/page.md.meta.json"));
    }

    #[test]
    fn artifact_meta_serializes_round_trip() {
        let m = ArtifactMeta::new(
            "blake3:deadbeef",
            "https://example.com/a",
            vec!["html2md".into(), "chunk".into()],
        );
        let json = serde_json::to_string(&m).unwrap();
        let back: ArtifactMeta = serde_json::from_str(&json).unwrap();
        assert_eq!(back, m);
        assert!(m.timestamp.contains('T'));
    }

    #[test]
    fn write_sidecar_emits_file() {
        let dir = tempfile::tempdir().unwrap();
        let artifact = dir.path().join("page.md");
        std::fs::write(&artifact, b"# hi").unwrap();
        let meta = ArtifactMeta::new(
            "blake3:abc",
            "https://example.com/p",
            vec!["html2md".into()],
        );
        let sidecar = write_sidecar(&artifact, &meta).unwrap();
        assert!(sidecar.exists());
        let raw = std::fs::read_to_string(&sidecar).unwrap();
        let back: ArtifactMeta = serde_json::from_str(&raw).unwrap();
        assert_eq!(back, meta);
    }
}
