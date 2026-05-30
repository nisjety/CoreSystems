//! Branding extraction — favicon, theme color, OG image, site name.

use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Branding {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub site_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub favicon: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub og_image: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub apple_touch_icon: Option<String>,
}

pub fn extract(html: &str, base_url: &Url) -> Branding {
    let doc = Html::parse_document(html);
    Branding {
        site_name: extract_meta_content(&doc, "og:site_name")
            .or_else(|| extract_meta_content(&doc, "application-name")),
        favicon: extract_favicon(&doc, base_url),
        theme_color: extract_meta_content(&doc, "theme-color"),
        og_image: extract_meta_content(&doc, "og:image")
            .map(|href| resolve(base_url, &href)),
        apple_touch_icon: extract_apple_touch_icon(&doc, base_url),
    }
}

fn extract_meta_content(doc: &Html, name: &str) -> Option<String> {
    let selectors = [
        format!(r#"meta[property="{name}"]"#),
        format!(r#"meta[name="{name}"]"#),
    ];
    for sel_str in &selectors {
        if let Ok(sel) = Selector::parse(sel_str) {
            if let Some(el) = doc.select(&sel).next() {
                if let Some(content) = el.value().attr("content") {
                    let trimmed = content.trim();
                    if !trimmed.is_empty() {
                        return Some(trimmed.to_string());
                    }
                }
            }
        }
    }
    None
}

fn extract_favicon(doc: &Html, base_url: &Url) -> Option<String> {
    let icon_selectors = [
        r#"link[rel="icon"]"#,
        r#"link[rel="shortcut icon"]"#,
    ];
    for sel_str in &icon_selectors {
        if let Ok(sel) = Selector::parse(sel_str) {
            if let Some(el) = doc.select(&sel).next() {
                if let Some(href) = el.value().attr("href") {
                    let trimmed = href.trim();
                    if !trimmed.is_empty() {
                        return Some(resolve(base_url, trimmed));
                    }
                }
            }
        }
    }
    Some(resolve(base_url, "/favicon.ico"))
}

fn extract_apple_touch_icon(doc: &Html, base_url: &Url) -> Option<String> {
    let sel = Selector::parse(r#"link[rel="apple-touch-icon"]"#).ok()?;
    let el = doc.select(&sel).next()?;
    let href = el.value().attr("href")?.trim();
    if href.is_empty() {
        return None;
    }
    Some(resolve(base_url, href))
}

fn resolve(base: &Url, href: &str) -> String {
    base.join(href).map(|u| u.to_string()).unwrap_or_else(|_| href.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Url {
        "https://example.com/page".parse().unwrap()
    }

    #[test]
    fn extracts_full_branding() {
        let html = r##"
        <html>
        <head>
            <meta property="og:site_name" content="Example Corp" />
            <meta name="theme-color" content="#ff6600" />
            <meta property="og:image" content="/images/og.png" />
            <link rel="icon" href="/favicon.svg" />
            <link rel="apple-touch-icon" href="/apple-icon-180.png" />
        </head>
        <body></body>
        </html>"##;
        let b = extract(html, &base());
        assert_eq!(b.site_name.as_deref(), Some("Example Corp"));
        assert_eq!(b.theme_color.as_deref(), Some("#ff6600"));
        assert_eq!(b.og_image.as_deref(), Some("https://example.com/images/og.png"));
        assert_eq!(b.favicon.as_deref(), Some("https://example.com/favicon.svg"));
        assert_eq!(b.apple_touch_icon.as_deref(), Some("https://example.com/apple-icon-180.png"));
    }

    #[test]
    fn defaults_favicon_to_ico() {
        let html = "<html><head></head><body></body></html>";
        let b = extract(html, &base());
        assert_eq!(b.favicon.as_deref(), Some("https://example.com/favicon.ico"));
        assert!(b.site_name.is_none());
        assert!(b.og_image.is_none());
    }

    #[test]
    fn application_name_fallback() {
        let html = r#"<html><head><meta name="application-name" content="MyApp" /></head><body></body></html>"#;
        let b = extract(html, &base());
        assert_eq!(b.site_name.as_deref(), Some("MyApp"));
    }

    #[test]
    fn empty_content_ignored() {
        let html = r#"<html><head><meta property="og:site_name" content="  " /></head><body></body></html>"#;
        let b = extract(html, &base());
        assert!(b.site_name.is_none());
    }
}
