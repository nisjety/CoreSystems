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
use url::Url;

use crate::branding::Branding;

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
    if let Ok(sel) = Selector::parse("style") {
        for el in doc.select(&sel) {
            let css = el.text().collect::<String>();
            extract_hex_colors(&css, &mut palette);
        }
    }
    if let Ok(sel) = Selector::parse("[style]") {
        for el in doc.select(&sel) {
            if let Some(s) = el.value().attr("style") {
                extract_hex_colors(s, &mut palette);
            }
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
            // attempt 6-digit hex
            if i + 6 < bytes.len() && (1..=6).all(|n| is_hex(bytes[i + n])) {
                let n = if i + 7 < bytes.len() && (1..=8).all(|n| is_hex(bytes[i + n])) {
                    8
                } else if i + 6 < bytes.len() && (1..=6).all(|n| is_hex(bytes[i + n])) {
                    6
                } else {
                    3
                };
                let hex = &s[i..i + n + 1];
                out.push(hex.to_lowercase());
                i += n + 1;
                continue;
            }
            if i + 3 < bytes.len() && (1..=3).all(|n| is_hex(bytes[i + n])) {
                let hex = &s[i..i + 4];
                out.push(hex.to_lowercase());
                i += 4;
                continue;
            }
        }
        i += 1;
    }
}

fn is_hex(b: u8) -> bool {
    b.is_ascii_hexdigit()
}

fn detect_font_family(doc: &Html) -> Option<String> {
    if let Ok(sel) = Selector::parse("style") {
        for el in doc.select(&sel) {
            let css = el.text().collect::<String>();
            if let Some(family) = extract_font_family(&css) {
                return Some(family);
            }
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
    let sel = Selector::parse("img").ok()?;
    let mut best: Option<(u32, String)> = None;
    for el in doc.select(&sel) {
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
        let logoish = alt.contains("logo") || cls.contains("logo") || src.to_lowercase().contains("logo");
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
    let sel = Selector::parse("body").ok()?;
    let body = doc.select(&sel).next()?;
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
