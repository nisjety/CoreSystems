//! Minimal metadata extraction (title/lang/content-type hints) and
//! artifact sidecar (`<artifact>.meta.json`) emission.

use std::io;
use std::path::{Path, PathBuf};

use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};

use quarry_core::output::PageMetadata;

pub fn extract(html: &str, content_type: Option<String>) -> PageMetadata {
    let doc = Html::parse_document(html);

    // Lift every <meta> tag once. Previous version re-parsed a fresh
    // selector for every field; this is one pass over the head.
    let meta_props = lift_meta_tags(&doc);

    // Pull schema.org JSON-LD when present. It's usually the most
    // accurate source for headline, author, dates — the publisher
    // emitted it specifically for machine consumption.
    let json_ld = crate::json_ld::extract(html);

    // Title: JSON-LD headline → og:title → twitter:title → <title>.
    let title = json_ld
        .as_ref()
        .and_then(|s| s.headline.clone())
        .or_else(|| meta_prop(&meta_props, "og:title"))
        .or_else(|| meta_name(&meta_props, "twitter:title"))
        .or_else(|| {
            Selector::parse("title").ok().and_then(|s| {
                doc.select(&s)
                    .next()
                    .map(|n| n.text().collect::<String>().trim().to_string())
                    .filter(|t| !t.is_empty())
            })
        });

    // Description: og:description → meta[name=description] →
    // twitter:description → JSON-LD.
    let description = meta_prop(&meta_props, "og:description")
        .or_else(|| meta_name(&meta_props, "description"))
        .or_else(|| meta_name(&meta_props, "twitter:description"))
        .or_else(|| json_ld.as_ref().and_then(|s| s.description.clone()));

    // Author: meta[name=author] → article:author → JSON-LD.
    let author = meta_name(&meta_props, "author")
        .or_else(|| meta_prop(&meta_props, "article:author"))
        .or_else(|| json_ld.as_ref().and_then(|s| s.author.clone()));

    // Publish + modified timestamps.
    let published_at = meta_prop(&meta_props, "article:published_time")
        .or_else(|| json_ld.as_ref().and_then(|s| s.date_published.clone()))
        .or_else(|| pubdate_from_time_tag(&doc));
    let modified_at = meta_prop(&meta_props, "article:modified_time")
        .or_else(|| meta_prop(&meta_props, "og:updated_time"))
        .or_else(|| json_ld.as_ref().and_then(|s| s.date_modified.clone()));

    // Language: html[lang] → og:locale. Body-content detection lives
    // in `lang.rs` because it needs cleaned visible text, not raw
    // HTML (which would include script/style content).
    let lang = Selector::parse("html[lang]")
        .ok()
        .and_then(|s| {
            doc.select(&s)
                .next()
                .and_then(|n| n.value().attr("lang").map(ToString::to_string))
        })
        .or_else(|| meta_prop(&meta_props, "og:locale"))
        .filter(|s| !s.trim().is_empty());

    PageMetadata {
        title,
        lang,
        content_type,
        description,
        author,
        published_at,
        modified_at,
    }
}

/// Lift every `<meta>` tag into per-attribute dicts. One DOM walk
/// instead of re-parsing a selector per field.
fn lift_meta_tags(doc: &Html) -> MetaProps {
    let mut props = MetaProps::default();
    if let Ok(sel) = Selector::parse("meta") {
        for el in doc.select(&sel) {
            let v = el.value();
            let content = match v.attr("content") {
                Some(c) if !c.trim().is_empty() => c.trim().to_string(),
                _ => continue,
            };
            if let Some(p) = v.attr("property") {
                props
                    .by_property
                    .insert(p.to_ascii_lowercase(), content.clone());
            }
            if let Some(n) = v.attr("name") {
                props.by_name.insert(n.to_ascii_lowercase(), content);
            }
        }
    }
    props
}

#[derive(Default)]
struct MetaProps {
    by_property: std::collections::HashMap<String, String>,
    by_name: std::collections::HashMap<String, String>,
}

fn meta_prop(p: &MetaProps, key: &str) -> Option<String> {
    p.by_property.get(&key.to_ascii_lowercase()).cloned()
}
fn meta_name(p: &MetaProps, key: &str) -> Option<String> {
    p.by_name.get(&key.to_ascii_lowercase()).cloned()
}

fn pubdate_from_time_tag(doc: &Html) -> Option<String> {
    // `<time pubdate datetime="2026-05-01">…</time>` — common on
    // older blog templates / WordPress themes.
    let sel = Selector::parse("time[pubdate][datetime], time[datetime]").ok()?;
    for el in doc.select(&sel) {
        if let Some(dt) = el.value().attr("datetime") {
            let t = dt.trim();
            if !t.is_empty() {
                return Some(t.to_string());
            }
        }
    }
    None
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
