//! Assembles a Firecrawl-compatible document from already-processed HTML and markdown.
//!
//! `to_firecrawl` is the only public entry point. It:
//! 1. Resolves lazy-loaded images in the raw HTML.
//! 2. Extracts Dublin Core / Open Graph metadata via `attributes::extract_dublin_core`.
//! 3. Extracts all hyperlinks via `links::extract`.
//! 4. Resolves `<img>` `src` attributes into absolute URLs.
//! 5. Reads the `<html lang>` attribute.

use std::collections::HashMap;

use scraper::{Html, Selector};
use url::Url;

use quarry_core::output::Link;

use crate::{attributes, images, links};

// ── Public types ────────────────────────────────────────────────────────────

/// A Firecrawl-compatible document produced from a single scraped page.
#[derive(Debug, Clone)]
pub struct FirecrawlDocument {
    /// Markdown rendition of the page body.
    pub markdown: String,
    /// Cleaned HTML (if caller supplies it; `None` when only raw is available).
    pub html: Option<String>,
    /// Raw HTML from the scraper, before any cleaning.
    pub raw_html: Option<String>,
    /// All hyperlinks found on the page.
    pub links: Vec<Link>,
    /// Absolute URLs of `<img>` elements.
    pub images: Vec<String>,
    /// Per-page metadata.
    pub metadata: FirecrawlMetadata,
}

/// Structured page-level metadata mirroring the Firecrawl API surface.
#[derive(Debug, Clone, Default)]
pub struct FirecrawlMetadata {
    pub title: Option<String>,
    pub description: Option<String>,
    /// Value of `<html lang="…">`.
    pub language: Option<String>,
    pub og_title: Option<String>,
    pub og_description: Option<String>,
    pub og_image: Option<String>,
    pub og_url: Option<String>,
    pub keywords: Option<String>,
    pub author: Option<String>,
    /// Canonical source URL (stringified).
    pub source_url: String,
    /// HTTP status code, if known at call time.
    pub status_code: Option<u16>,
}

// ── Public API ───────────────────────────────────────────────────────────────

/// Converts raw HTML + pre-rendered `markdown` into a [`FirecrawlDocument`].
///
/// * `html`        – raw HTML as received from the scraper.
/// * `base`        – canonical page URL used to resolve relative references.
/// * `markdown`    – already-rendered markdown (caller's responsibility).
/// * `status_code` – HTTP status received when fetching the page.
pub fn to_firecrawl(
    html: &str,
    base: &Url,
    markdown: String,
    status_code: Option<u16>,
) -> FirecrawlDocument {
    // Resolve lazy-loaded images first so every downstream step sees real src values.
    let resolved_html = images::resolve_lazy_images(html);

    let meta_map: HashMap<String, String> = attributes::extract_dublin_core(&resolved_html);
    let page_links = links::extract(&resolved_html, base);
    let lang = extract_lang(&resolved_html);
    let document_title = extract_title(&resolved_html);
    let imgs = extract_images(&resolved_html, base);

    FirecrawlDocument {
        markdown,
        html: None,
        raw_html: Some(html.to_owned()),
        links: page_links,
        images: imgs,
        metadata: FirecrawlMetadata {
            title: meta_map.get("title").cloned().or(document_title),
            description: meta_map.get("description").cloned(),
            language: lang,
            og_title: meta_map.get("og:title").cloned(),
            og_description: meta_map.get("og:description").cloned(),
            og_image: meta_map.get("og:image").cloned(),
            og_url: meta_map.get("og:url").cloned(),
            keywords: meta_map.get("keywords").cloned(),
            author: meta_map.get("author").cloned(),
            source_url: base.to_string(),
            status_code,
        },
    }
}

// ── Private helpers ──────────────────────────────────────────────────────────

/// Returns the value of `<html lang="…">`, if present and non-empty.
fn extract_lang(html: &str) -> Option<String> {
    // Safety: this selector is a compile-time constant; unwrap is intentional.
    let sel = Selector::parse("html[lang]").expect("valid selector");
    let doc = Html::parse_document(html);
    doc.select(&sel)
        .next()
        .and_then(|el| el.value().attr("lang").map(ToOwned::to_owned))
        .filter(|s| !s.is_empty())
}

fn extract_title(html: &str) -> Option<String> {
    let sel = Selector::parse("title").expect("valid selector");
    let doc = Html::parse_document(html);
    doc.select(&sel)
        .next()
        .map(|el| el.text().collect::<String>().trim().to_string())
        .filter(|title| !title.is_empty())
}

/// Collects absolute URLs for every `<img src="…">` found in `html`.
///
/// Relative URLs are resolved against `base`; images that cannot be parsed
/// are silently skipped.
fn extract_images(html: &str, base: &Url) -> Vec<String> {
    let sel = Selector::parse("img[src]").expect("valid selector");
    let doc = Html::parse_document(html);
    doc.select(&sel)
        .filter_map(|el| {
            let src = el.value().attr("src")?;
            base.join(src).ok().map(|u| u.to_string())
        })
        .collect()
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "https://example.com/page";

    fn base_url() -> Url {
        Url::parse(BASE).unwrap()
    }

    // ── extract_lang ─────────────────────────────────────────────────────────

    #[test]
    fn lang_present() {
        let html = r#"<!DOCTYPE html><html lang="en-US"><head></head><body></body></html>"#;
        assert_eq!(extract_lang(html), Some("en-US".into()));
    }

    #[test]
    fn lang_missing() {
        let html = r#"<!DOCTYPE html><html><head></head><body></body></html>"#;
        assert_eq!(extract_lang(html), None);
    }

    #[test]
    fn lang_empty_string_treated_as_absent() {
        let html = r#"<!DOCTYPE html><html lang=""><head></head><body></body></html>"#;
        assert_eq!(extract_lang(html), None);
    }

    // ── extract_images ───────────────────────────────────────────────────────

    #[test]
    fn absolute_image_unchanged() {
        let html = r#"<html><body><img src="https://cdn.example.com/pic.jpg"></body></html>"#;
        let imgs = extract_images(html, &base_url());
        assert_eq!(imgs, vec!["https://cdn.example.com/pic.jpg"]);
    }

    #[test]
    fn relative_image_resolved() {
        let html = r#"<html><body><img src="/images/logo.png"></body></html>"#;
        let imgs = extract_images(html, &base_url());
        assert_eq!(imgs, vec!["https://example.com/images/logo.png"]);
    }

    #[test]
    fn no_images_returns_empty_vec() {
        let html = r#"<html><body><p>no images here</p></body></html>"#;
        let imgs = extract_images(html, &base_url());
        assert!(imgs.is_empty());
    }

    #[test]
    fn data_uri_skipped_when_base_join_fails() {
        // `data:` URIs are valid URLs so `base.join` succeeds and they are kept.
        // This test documents the current behaviour.
        let html = r#"<html><body><img src="data:image/png;base64,abc"></body></html>"#;
        let imgs = extract_images(html, &base_url());
        // data: URI parses fine; we keep it.
        assert_eq!(imgs.len(), 1);
        assert!(imgs[0].starts_with("data:"));
    }

    // ── to_firecrawl ─────────────────────────────────────────────────────────

    #[test]
    fn to_firecrawl_basic() {
        let html = r#"<!DOCTYPE html>
<html lang="fr">
<head>
  <title>Bonjour</title>
  <meta name="description" content="Une page de test">
  <meta property="og:title" content="OG Bonjour">
</head>
<body>
  <a href="/about">About</a>
  <img src="/img/hero.jpg">
</body>
</html>"#;

        let base = base_url();
        let doc = to_firecrawl(html, &base, "# Bonjour".into(), Some(200));

        assert_eq!(doc.markdown, "# Bonjour");
        assert!(doc.raw_html.is_some());
        assert_eq!(doc.metadata.language.as_deref(), Some("fr"));
        assert_eq!(doc.metadata.title.as_deref(), Some("Bonjour"));
        assert_eq!(doc.metadata.og_title.as_deref(), Some("OG Bonjour"));
        assert_eq!(doc.metadata.status_code, Some(200));
        assert!(!doc.links.is_empty());
        assert!(!doc.images.is_empty());
        assert_eq!(doc.images[0], "https://example.com/img/hero.jpg");
    }

    #[test]
    fn to_firecrawl_unknown_status() {
        let base = base_url();
        let doc = to_firecrawl("<html><body></body></html>", &base, String::new(), None);
        assert!(doc.metadata.status_code.is_none());
    }
}
