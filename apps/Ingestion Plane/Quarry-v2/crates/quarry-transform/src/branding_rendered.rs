//! Rendered branding fallback (QRY-11).
//!
//! When static HTML extraction misses key brand signals (single-page apps that
//! don't ship `<meta>` until after JS hydrates, or template-based sites that
//! render colors via CSS variables computed at runtime), the rendered branding
//! pipeline captures additional signal from the rendered DOM:
//!
//! - Inline `<style>` content (theme palette, primary colors)
//! - Body/html computed background colors (best-effort regex)
//! - Largest `<img>` element by attribute size as a logo candidate
//! - Detected font family from CSS
//!
//! This is intentionally a heuristic — the visual interpretation step (running
//! a vision model on a screenshot) belongs in Model Plane. Quarry just
//! captures the evidence.

use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use url::Url;

use crate::branding::Branding;

// CSS selectors compiled once at first use. The previous version called
// `Selector::parse(...)` on every invocation of `collect_palette`,
// `detect_font_family`, and `detect_logo` — three regex compiles per
// branding extraction, multiplied by every crawled page.
fn style_block_selector() -> &'static Selector {
    static SEL: OnceLock<Selector> = OnceLock::new();
    SEL.get_or_init(|| Selector::parse("style").expect("static selector"))
}
fn inline_style_selector() -> &'static Selector {
    static SEL: OnceLock<Selector> = OnceLock::new();
    SEL.get_or_init(|| Selector::parse("[style]").expect("static selector"))
}
fn img_selector() -> &'static Selector {
    static SEL: OnceLock<Selector> = OnceLock::new();
    SEL.get_or_init(|| Selector::parse("img").expect("static selector"))
}
fn body_selector() -> &'static Selector {
    static SEL: OnceLock<Selector> = OnceLock::new();
    SEL.get_or_init(|| Selector::parse("body").expect("static selector"))
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RenderedBranding {
    /// Static branding signals (favicon, og:image, etc.) layered first.
    pub static_signals: Branding,
    /// Hex color codes detected in inline CSS (primary palette candidates).
    pub palette: Vec<String>,
    /// Detected primary font family.
    pub font_family: Option<String>,
    /// Largest `<img>` URL detected — likely a logo/hero image.
    pub logo_candidate: Option<String>,
    /// Body background color if detected from inline style.
    pub body_background: Option<String>,
}

/// Run the full pipeline: static branding + rendered enhancement.
pub fn extract(html: &str, base_url: &Url) -> RenderedBranding {
    let doc = Html::parse_document(html);
    let static_signals = crate::branding::extract(html, base_url);

    RenderedBranding {
        palette: collect_palette(&doc),
        font_family: detect_font_family(&doc),
        logo_candidate: detect_logo(&doc, base_url),
        body_background: detect_body_background(&doc),
        static_signals,
    }
}

fn collect_palette(doc: &Html) -> Vec<String> {
    let mut palette = Vec::new();
    for el in doc.select(style_block_selector()) {
        let css = el.text().collect::<String>();
        extract_hex_colors(&css, &mut palette);
    }
    for el in doc.select(inline_style_selector()) {
        if let Some(s) = el.value().attr("style") {
            extract_hex_colors(s, &mut palette);
        }
    }
    palette.sort();
    palette.dedup();
    palette.into_iter().take(16).collect()
}

fn extract_hex_colors(s: &str, out: &mut Vec<String>) {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'#' {
            // Try the longest match first (8 → 6 → 3) and fall through
            // explicitly. The previous version conflated the 6-digit
            // and 3-digit branches and could emit a 3-char "color"
            // built from garbage bytes when the input had exactly 6
            // hex chars at end-of-string. Now each length is
            // independently bounds-checked.
            //
            // We require a non-hex byte immediately after the run so
            // `#ff00ff` is parsed as 6-digit (not 6 + then-fail-to-3).
            let n = pick_hex_len(bytes, i);
            if n > 0 {
                let hex = &s[i..i + n + 1];
                out.push(hex.to_lowercase());
                i += n + 1;
                continue;
            }
        }
        i += 1;
    }
}

/// Returns the length of the hex digits run starting at `bytes[i+1]`,
/// snapped to one of {8,6,3}. Returns 0 when no recognised CSS-color
/// length fits. The `#` itself is not counted.
///
/// Each candidate length passes only when:
/// 1. The `want` hex bytes exist at positions `i+1..=i+want`, and
/// 2. The boundary byte at `i+want+1` is either past end-of-string
///    or not a hex digit — otherwise we'd truncate a longer color
///    (`#ffaabbcc` parsed as 6-digit instead of 8-digit).
fn pick_hex_len(bytes: &[u8], i: usize) -> usize {
    // Longest first so `#ffaabbcc` is 8-digit, not 6-then-suffix.
    for &want in &[8usize, 6, 3] {
        // Need positions i+1..=i+want to all exist (highest index is
        // i+want, so the slice must have len >= i+want+1).
        if i + want >= bytes.len() {
            continue;
        }
        if !(1..=want).all(|n| is_hex(bytes[i + n])) {
            continue;
        }
        // Boundary check: i+want+1 may legitimately be out of bounds
        // (we matched up to EOF) — that counts as a valid terminator.
        let boundary_ok = i + want + 1 >= bytes.len() || !is_hex(bytes[i + want + 1]);
        if boundary_ok {
            return want;
        }
    }
    0
}

fn is_hex(b: u8) -> bool {
    b.is_ascii_hexdigit()
}

fn detect_font_family(doc: &Html) -> Option<String> {
    for el in doc.select(style_block_selector()) {
        let css = el.text().collect::<String>();
        if let Some(family) = extract_font_family(&css) {
            return Some(family);
        }
    }
    None
}

fn extract_font_family(css: &str) -> Option<String> {
    // Search for `font-family: ...` (case-insensitive) and return the first
    // identifier in the value.
    let lower = css.to_lowercase();
    let key = "font-family:";
    let pos = lower.find(key)?;
    let rest = &css[pos + key.len()..];
    let end = rest.find([';', '}', '\n']).unwrap_or(rest.len());
    let value = rest[..end].trim();
    let family = value
        .split(',')
        .next()?
        .trim()
        .trim_matches('"')
        .trim_matches('\'');
    if family.is_empty() {
        None
    } else {
        Some(family.to_string())
    }
}

fn detect_logo(doc: &Html, base_url: &Url) -> Option<String> {
    let mut best: Option<(u32, String)> = None;
    for el in doc.select(img_selector()) {
        let src = match el.value().attr("src") {
            Some(s) if !s.is_empty() => s,
            _ => continue,
        };
        let w = el
            .value()
            .attr("width")
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(0);
        let h = el
            .value()
            .attr("height")
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(0);
        let area = w.saturating_mul(h);
        // Prefer images named "logo" even if dimensions are missing.
        let alt = el.value().attr("alt").unwrap_or("").to_lowercase();
        let cls = el.value().attr("class").unwrap_or("").to_lowercase();
        let logoish =
            alt.contains("logo") || cls.contains("logo") || src.to_lowercase().contains("logo");
        // Logo signals dominate — naming a child "logo" is a strong hint that
        // beats area heuristics for any reasonably-sized image.
        let score = area.saturating_add(if logoish { 10_000_000 } else { 0 });
        if score > best.as_ref().map(|(s, _)| *s).unwrap_or(0) {
            best = Some((score, src.to_string()));
        }
    }
    best.map(|(_, src)| resolve(base_url, &src))
}

fn detect_body_background(doc: &Html) -> Option<String> {
    let body = doc.select(body_selector()).next()?;
    if let Some(style) = body.value().attr("style") {
        if let Some(bg) = extract_property(style, "background-color") {
            return Some(bg);
        }
        if let Some(bg) = extract_property(style, "background") {
            return Some(bg);
        }
    }
    None
}

fn extract_property(style: &str, prop: &str) -> Option<String> {
    let lower = style.to_lowercase();
    let key = format!("{prop}:");
    let pos = lower.find(&key)?;
    let rest = &style[pos + key.len()..];
    let end = rest.find(';').unwrap_or(rest.len());
    let value = rest[..end].trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn resolve(base: &Url, href: &str) -> String {
    if href.starts_with("http://") || href.starts_with("https://") || href.starts_with("data:") {
        return href.to_string();
    }
    base.join(href)
        .map(|u| u.to_string())
        .unwrap_or_else(|_| href.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url() -> Url {
        Url::parse("https://example.com/").unwrap()
    }

    #[test]
    fn collects_palette_from_inline_style_and_style_block() {
        let html = r#"
            <html><head>
                <style>:root { --primary: #ff5500; --secondary: #00aaff; }</style>
            </head>
            <body style="background-color: #ffffff;">
                <p style="color:#333">x</p>
            </body></html>
        "#;
        let r = extract(html, &url());
        assert!(r.palette.contains(&"#ff5500".to_string()));
        assert!(r.palette.contains(&"#00aaff".to_string()));
        assert!(r.palette.contains(&"#ffffff".to_string()));
    }

    #[test]
    fn detects_font_family_from_style_block() {
        let html = r#"<style>body { font-family: "Inter", sans-serif; }</style>"#;
        let r = extract(html, &url());
        assert_eq!(r.font_family.as_deref(), Some("Inter"));
    }

    #[test]
    fn detects_logo_with_logo_class_or_alt() {
        let html = r#"
            <img src="/banner.jpg" width="800" height="200">
            <img src="/logo.svg" alt="Acme Logo" width="100" height="40">
        "#;
        let r = extract(html, &url());
        // Despite smaller area, logo.svg wins via class/alt boost.
        assert!(r.logo_candidate.as_deref().unwrap().ends_with("/logo.svg"));
    }

    #[test]
    fn detects_body_background_from_inline_style() {
        let html = r#"<body style="background-color: #112233;">x</body>"#;
        let r = extract(html, &url());
        assert_eq!(r.body_background.as_deref(), Some("#112233"));
    }

    #[test]
    fn returns_layered_static_signals() {
        let html = r#"
            <head>
                <meta property="og:site_name" content="Acme">
                <link rel="icon" href="/favicon.ico">
            </head>
            <body></body>
        "#;
        let r = extract(html, &url());
        assert_eq!(r.static_signals.site_name.as_deref(), Some("Acme"));
        assert!(r.static_signals.favicon.is_some());
    }

    #[test]
    fn empty_html_returns_empty_branding() {
        let r = extract("<html></html>", &url());
        assert!(r.palette.is_empty());
        assert!(r.logo_candidate.is_none());
        assert!(r.font_family.is_none());
    }
}
