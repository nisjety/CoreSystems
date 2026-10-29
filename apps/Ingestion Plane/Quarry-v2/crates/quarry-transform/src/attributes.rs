//! HTML attribute & structural transforms.
//!
//! Donor parity:
//! - `internal/transform/script_stripper.go`
//! - `internal/transform/canonical_url_normalizer.go`
//! - `internal/transform/dublin_core_extractor.go`
//! - `internal/transform/table_normalizer.go`
//! - `internal/transform/link_resolver.go` (anchor href paths)

use lol_html::html_content::ContentType;
use lol_html::{element, rewrite_str, RewriteStrSettings};
use regex::Regex;
use std::collections::HashMap;
use std::sync::OnceLock;
use url::Url;

// ---------------------------------------------------------------------------
// ScriptStripper
// ---------------------------------------------------------------------------

/// Remove `<script>`, `<noscript>`, tracking iframes, and 1x1 / hidden pixel
/// images. Mirrors donor `script_stripper.go`.
pub fn strip_scripts(html: &str) -> String {
    let element_content_handlers = vec![
        element!("script", |el| {
            el.remove();
            Ok(())
        }),
        element!("noscript", |el| {
            el.remove();
            Ok(())
        }),
        element!("iframe", |el| {
            if let Some(src) = el.get_attribute("src") {
                let s = src.to_ascii_lowercase();
                if s.contains("doubleclick")
                    || s.contains("googletagmanager")
                    || s.contains("google-analytics")
                    || s.contains("facebook.com/tr")
                    || s.contains("hotjar")
                {
                    el.remove();
                }
            }
            Ok(())
        }),
        element!("img", |el| {
            let w = el.get_attribute("width").unwrap_or_default();
            let h = el.get_attribute("height").unwrap_or_default();
            if w == "1" && h == "1" {
                el.remove();
                return Ok(());
            }
            if let Some(style) = el.get_attribute("style") {
                let s = style.to_ascii_lowercase();
                if s.contains("display:none") || s.contains("display: none") {
                    el.remove();
                }
            }
            Ok(())
        }),
    ];

    let settings = element_content_handlers.into_iter().fold(
        RewriteStrSettings::new(),
        RewriteStrSettings::append_element_content_handler,
    );
    rewrite_str(html, settings).unwrap_or_else(|_| html.to_string())
}

// ---------------------------------------------------------------------------
// CanonicalURLNormalizer
// ---------------------------------------------------------------------------

/// Extract `<link rel="canonical">` href, resolving against `base` when relative.
/// Returns `None` when no canonical link is present or resolution fails.
pub fn normalize_canonical_url(html: &str, base: &str) -> Option<String> {
    let link_re = link_tag_regex();
    for cap in link_re.captures_iter(html) {
        let attrs_str = cap.get(1)?.as_str();
        let attrs = parse_tag_attrs(attrs_str);
        let rel = attrs
            .get("rel")
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_default();
        if rel != "canonical" {
            continue;
        }
        let href = attrs.get("href")?.trim();
        if href.is_empty() {
            return None;
        }
        if let Ok(abs) = Url::parse(href) {
            return Some(abs.to_string());
        }
        let base_url = Url::parse(base).ok()?;
        return base_url.join(href).ok().map(|u| u.to_string());
    }
    None
}

fn link_tag_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?is)<link\b([^>]*)/?>").expect("link regex"))
}

/// One language variant of the same logical page, as advertised by
/// `<link rel="alternate" hreflang="...">`. The crawler uses the
/// variant list to *dedupe* — a single canonical page with 30
/// hreflang siblings shouldn't be fetched 31 times for the same
/// content. See `extract_hreflang_variants` and the crawl frontier's
/// canonical-aware dedup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HreflangVariant {
    /// Language tag from the `hreflang` attribute, e.g. `en-US`,
    /// `nb-NO`, `x-default`. Preserved verbatim (lower-cased) so
    /// downstream consumers can compare or sort.
    pub lang: String,
    /// Resolved absolute URL for that variant.
    pub url: String,
}

/// Extract every `<link rel="alternate" hreflang="...">` from the
/// document, resolving relative hrefs against `base`. Returns an empty
/// vec when the page has no hreflang alternates (the common case).
///
/// Used by the crawl frontier to recognise that "the German, French,
/// Italian, etc. variants of /products/widget all redirect to the same
/// canonical content" and skip the duplicate fetches.
pub fn extract_hreflang_variants(html: &str, base: &str) -> Vec<HreflangVariant> {
    let mut out = Vec::new();
    let base_url = match Url::parse(base) {
        Ok(u) => u,
        Err(_) => return out,
    };
    for cap in link_tag_regex().captures_iter(html) {
        let Some(attrs_str) = cap.get(1).map(|m| m.as_str()) else {
            continue;
        };
        let attrs = parse_tag_attrs(attrs_str);
        let rel = attrs
            .get("rel")
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_default();
        if rel != "alternate" {
            continue;
        }
        let Some(href) = attrs.get("href").map(|s| s.trim()) else {
            continue;
        };
        if href.is_empty() {
            continue;
        }
        let Some(lang) = attrs.get("hreflang").map(|s| s.trim().to_ascii_lowercase()) else {
            continue;
        };
        if lang.is_empty() {
            continue;
        }
        let url = if let Ok(abs) = Url::parse(href) {
            abs.to_string()
        } else if let Ok(joined) = base_url.join(href) {
            joined.to_string()
        } else {
            continue;
        };
        out.push(HreflangVariant { lang, url });
    }
    out
}

// ---------------------------------------------------------------------------
// DublinCoreExtractor
// ---------------------------------------------------------------------------

/// Extract Dublin Core, OpenGraph, article:* and standard meta keywords/description
/// into a single map. Mirrors donor `dublin_core_extractor.go`.
pub fn extract_dublin_core(html: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let meta_re = meta_tag_regex();

    for cap in meta_re.captures_iter(html) {
        let attrs_str = match cap.get(1) {
            Some(m) => m.as_str(),
            None => continue,
        };
        let attrs = parse_tag_attrs(attrs_str);
        let content = attrs.get("content").map(|s| s.trim()).unwrap_or("");
        if content.is_empty() {
            continue;
        }

        if let Some(name) = attrs.get("name") {
            let name = name.trim();
            let lower = name.to_ascii_lowercase();
            if lower.starts_with("dc.")
                || lower.starts_with("dcterms.")
                || lower.starts_with("article:")
                || lower == "keywords"
                || lower == "description"
                || lower == "author"
            {
                out.entry(name.to_string())
                    .or_insert_with(|| content.to_string());
            }
        }

        if let Some(prop) = attrs.get("property") {
            let prop = prop.trim();
            let lower = prop.to_ascii_lowercase();
            if lower.starts_with("og:") || lower.starts_with("article:") {
                out.entry(prop.to_string())
                    .or_insert_with(|| content.to_string());
            }
        }
    }
    out
}

fn meta_tag_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?is)<meta\b([^>]*)/?>").expect("meta regex"))
}

/// Parse HTML tag attributes from the inside of an opening tag (the part
/// between the tag name and `>`). Returns lowercased attribute names mapped
/// to their unescaped values. Supports double-quoted, single-quoted, and
/// unquoted values.
fn parse_tag_attrs(s: &str) -> HashMap<String, String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(
            r#"(?is)([a-zA-Z_:][-a-zA-Z0-9_:.]*)\s*(?:=\s*(?:"([^"]*)"|'([^']*)'|([^\s"'>`]+)))?"#,
        )
        .expect("attr regex")
    });
    let mut out = HashMap::new();
    for cap in re.captures_iter(s) {
        let key = cap.get(1).expect("attr name").as_str().to_ascii_lowercase();
        let val = cap
            .get(2)
            .or_else(|| cap.get(3))
            .or_else(|| cap.get(4))
            .map(|m| m.as_str().to_string())
            .unwrap_or_default();
        out.entry(key).or_insert(val);
    }
    out
}

// ---------------------------------------------------------------------------
// TableNormalizer
// ---------------------------------------------------------------------------

/// If a `<table>` has no `<thead>` but its first `<tr>` is composed entirely of
/// `<th>` cells, wrap that row in a `<thead>`. Mirrors donor `table_normalizer.go`.
///
/// Uses regex on the raw input rather than scraper, because `Html::parse_document`
/// injects an implicit `<tbody>` around `<tr>` children of a `<table>` per the
/// HTML5 spec, which makes round-tripping through `table.html()` mismatch the
/// raw input string.
pub fn normalize_tables(html: &str) -> String {
    let table_re = table_regex();
    let tr_re = tr_regex();
    let cell_re = cell_regex();

    let mut out = String::with_capacity(html.len());
    let mut last = 0usize;
    for m in table_re.captures_iter(html) {
        let whole = m.get(0).expect("whole match");
        let inner = m.get(1).expect("inner capture").as_str();
        out.push_str(&html[last..whole.start()]);
        last = whole.end();

        // Skip if the table already has a <thead>.
        if inner.to_ascii_lowercase().contains("<thead") {
            out.push_str(whole.as_str());
            continue;
        }
        // Locate the first <tr>...</tr>.
        let Some(tr_m) = tr_re.captures(inner) else {
            out.push_str(whole.as_str());
            continue;
        };
        let tr_full = tr_m.get(0).expect("tr full match").as_str();
        let tr_inner = tr_m.get(1).expect("tr inner").as_str();
        // All cell tags in the first row must be <th>.
        let cells: Vec<&str> = cell_re
            .captures_iter(tr_inner)
            .map(|c| c.get(1).expect("cell tag").as_str())
            .collect();
        if cells.is_empty() || !cells.iter().all(|c| c.eq_ignore_ascii_case("th")) {
            out.push_str(whole.as_str());
            continue;
        }
        // Wrap the first <tr> in a <thead>.
        let table_open_end = whole.as_str().find('>').expect("table open") + 1;
        let table_open = &whole.as_str()[..table_open_end];
        let new_inner = inner.replacen(tr_full, &format!("<thead>{}</thead>", tr_full), 1);
        out.push_str(table_open);
        out.push_str(&new_inner);
        out.push_str("</table>");
    }
    out.push_str(&html[last..]);
    out
}

fn table_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?is)<table\b[^>]*>(.*?)</table>").expect("table regex"))
}

fn tr_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?is)<tr\b[^>]*>(.*?)</tr>").expect("tr regex"))
}

fn cell_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)<(t[hd])\b").expect("cell regex"))
}

// ---------------------------------------------------------------------------
// LinkResolver (anchor href absolutization)
// ---------------------------------------------------------------------------

/// Absolutize all `<a href>` against `base`. Skips javascript:/mailto:/tel:/#.
/// Mirrors donor `link_resolver.go` for anchors only; image paths live in `images.rs`.
pub fn resolve_anchor_hrefs(html: &str, base: &str) -> String {
    let Ok(base_url) = Url::parse(base) else {
        return html.to_string();
    };

    let element_content_handlers = vec![element!("a[href]", move |el| {
        if let Some(href) = el.get_attribute("href") {
            let trimmed = href.trim();
            if trimmed.is_empty() {
                return Ok(());
            }
            let lower = trimmed.to_ascii_lowercase();
            if lower.starts_with("javascript:")
                || lower.starts_with("mailto:")
                || lower.starts_with("tel:")
                || lower.starts_with('#')
            {
                return Ok(());
            }
            if Url::parse(trimmed).is_ok() {
                return Ok(());
            }
            if let Ok(abs) = base_url.join(trimmed) {
                let _ = el.set_attribute("href", abs.as_str());
            }
        }
        Ok(())
    })];

    let settings = element_content_handlers.into_iter().fold(
        RewriteStrSettings::new(),
        RewriteStrSettings::append_element_content_handler,
    );
    rewrite_str(html, settings).unwrap_or_else(|_| html.to_string())
}

// Suppress unused-import lint if a future refactor drops one of the usages.
#[allow(dead_code)]
fn _ct_marker() -> ContentType {
    ContentType::Html
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_scripts_and_noscript() {
        let html =
            "<html><body><p>ok</p><script>alert(1)</script><noscript>nope</noscript></body></html>";
        let out = strip_scripts(html);
        assert!(!out.contains("<script"));
        assert!(!out.contains("<noscript"));
        assert!(out.contains("<p>ok</p>"));
    }

    #[test]
    fn strips_tracking_pixels() {
        let html =
            r#"<html><body><img src="a.gif" width="1" height="1"><img src="b.gif"></body></html>"#;
        let out = strip_scripts(html);
        assert!(!out.contains("a.gif"));
        assert!(out.contains("b.gif"));
    }

    #[test]
    fn extracts_canonical_relative() {
        let html = r#"<html><head><link rel="canonical" href="/x"></head></html>"#;
        let canon = normalize_canonical_url(html, "https://example.com/page").unwrap();
        assert_eq!(canon, "https://example.com/x");
    }

    #[test]
    fn extracts_canonical_absolute() {
        let html = r#"<link rel="canonical" href="https://other.test/a">"#;
        let canon = normalize_canonical_url(html, "https://example.com").unwrap();
        assert_eq!(canon, "https://other.test/a");
    }

    #[test]
    fn extracts_dublin_core_and_og() {
        let html = r#"<html><head>
            <meta name="DC.title" content="T">
            <meta name="keywords" content="a,b">
            <meta property="og:title" content="OG">
            <meta name="ignored" content="x">
        </head></html>"#;
        let map = extract_dublin_core(html);
        assert_eq!(map.get("DC.title").map(String::as_str), Some("T"));
        assert_eq!(map.get("keywords").map(String::as_str), Some("a,b"));
        assert_eq!(map.get("og:title").map(String::as_str), Some("OG"));
        assert!(!map.contains_key("ignored"));
    }

    #[test]
    fn wraps_first_tr_of_th_in_thead() {
        let html = "<table><tr><th>A</th><th>B</th></tr><tr><td>1</td><td>2</td></tr></table>";
        let out = normalize_tables(html);
        assert!(out.contains("<thead>"), "missing <thead>: {}", out);
    }

    #[test]
    fn skips_table_with_existing_thead() {
        let html =
            "<table><thead><tr><th>X</th></tr></thead><tbody><tr><td>v</td></tr></tbody></table>";
        let out = normalize_tables(html);
        assert_eq!(out.matches("<thead>").count(), 1);
    }

    #[test]
    fn resolves_relative_anchor_hrefs() {
        let html =
            r##"<a href="/path">x</a><a href="https://x.test/abs">y</a><a href="#frag">z</a>"##;
        let out = resolve_anchor_hrefs(html, "https://example.com/dir/");
        assert!(out.contains("https://example.com/path"));
        assert!(out.contains("https://x.test/abs"));
        assert!(out.contains("href=\"#frag\""));
    }

    #[test]
    fn skips_javascript_and_mailto() {
        let html = r#"<a href="javascript:foo()">a</a><a href="mailto:x@y">b</a>"#;
        let out = resolve_anchor_hrefs(html, "https://example.com");
        assert!(out.contains("javascript:foo()"));
        assert!(out.contains("mailto:x@y"));
    }
}
