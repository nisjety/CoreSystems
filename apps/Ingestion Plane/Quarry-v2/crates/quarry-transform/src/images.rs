//! Image transforms: lazy-load resolution, base64 stripping, srcset resolution.
//!
//! Donor parity:
//! - `internal/transform/lazy_image_resolver.go`
//! - `internal/transform/base64_image_stripper.go`
//! - `internal/transform/link_resolver.go` (image-specific paths)

use lol_html::html_content::ContentType;
use lol_html::{element, rewrite_str, RewriteStrSettings};
use regex::Regex;
use std::sync::OnceLock;
use url::Url;

/// Lazy-load attributes inspected, in donor priority order.
pub const LAZY_ATTRS: &[&str] = &["data-src", "data-lazy-src", "data-original"];

/// Match donor `isPlaceholder` regex — case-insensitive markers for tracking pixels & blanks.
fn placeholder_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)(data:image|placeholder|grey\.gif|blank\.gif|spacer\.gif|pixel\.gif|1x1)")
            .expect("placeholder regex must compile")
    })
}

/// Returns true when `src` looks like a placeholder/tracking pixel.
pub fn is_placeholder(src: &str) -> bool {
    src.is_empty() || placeholder_re().is_match(src)
}

/// Resolve a comma-separated `srcset` against `base`, preserving descriptors.
pub fn resolve_srcset(srcset: &str, base: &Url) -> String {
    srcset
        .split(',')
        .map(|part| {
            let trimmed = part.trim();
            if trimmed.is_empty() {
                return String::new();
            }
            let mut iter = trimmed.split_whitespace();
            let url_part = iter.next().unwrap_or("");
            let descriptor: Vec<&str> = iter.collect();
            let resolved = base
                .join(url_part)
                .map(|u| u.to_string())
                .unwrap_or_else(|_| url_part.to_string());
            if descriptor.is_empty() {
                resolved
            } else {
                format!("{} {}", resolved, descriptor.join(" "))
            }
        })
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Promote lazy-load attributes onto `src`/`srcset` when the visible value is missing or a placeholder.
pub fn resolve_lazy_images(html: &str) -> String {
    rewrite_str(
        html,
        RewriteStrSettings::new().append_element_content_handler(element!("img", |el| {
            let current = el.get_attribute("src").unwrap_or_default();
            if is_placeholder(&current) {
                for attr in LAZY_ATTRS {
                    if let Some(val) = el.get_attribute(attr) {
                        if !val.trim().is_empty() {
                            let _ = el.set_attribute("src", &val);
                            break;
                        }
                    }
                }
            }
            let current_set = el.get_attribute("srcset").unwrap_or_default();
            if current_set.trim().is_empty() {
                if let Some(val) = el.get_attribute("data-srcset") {
                    if !val.trim().is_empty() {
                        let _ = el.set_attribute("srcset", &val);
                    }
                }
            }
            Ok(())
        })),
    )
    .unwrap_or_else(|_| html.to_string())
}

/// Resolve relative `src` and `srcset` URLs on `<img>` and `<source>` against `base`.
pub fn resolve_image_links(html: &str, base: &Url) -> String {
    let resolve_one = |val: &str| -> String {
        base.join(val)
            .map(|u| u.to_string())
            .unwrap_or_else(|_| val.to_string())
    };
    let base_for_src = base.clone();
    let base_for_set = base.clone();
    rewrite_str(
        html,
        RewriteStrSettings::new().append_element_content_handler(element!(
            "img, source",
            move |el| {
                if let Some(src) = el.get_attribute("src") {
                    if !src.is_empty() {
                        let _ = el.set_attribute("src", &resolve_one(&src));
                    }
                }
                if let Some(set) = el.get_attribute("srcset") {
                    if !set.is_empty() {
                        let _ = el.set_attribute("srcset", &resolve_srcset(&set, &base_for_set));
                    }
                }
                let _ = &base_for_src;
                Ok(())
            }
        )),
    )
    .unwrap_or_else(|_| html.to_string())
}

/// Remove `<img>` / `<source>` whose `src`, `srcset`, or `data-src` is a base64 data URI.
pub fn strip_base64_images(html: &str) -> String {
    let is_b64 = |v: &str| v.trim_start().to_ascii_lowercase().starts_with("data:");
    rewrite_str(
        html,
        RewriteStrSettings::new().append_element_content_handler(element!(
            "img, source",
            move |el| {
                let src = el.get_attribute("src").unwrap_or_default();
                let set = el.get_attribute("srcset").unwrap_or_default();
                let data_src = el.get_attribute("data-src").unwrap_or_default();
                if is_b64(&src) || is_b64(&set) || is_b64(&data_src) {
                    el.replace("", ContentType::Html);
                }
                Ok(())
            }
        )),
    )
    .unwrap_or_else(|_| html.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Url {
        Url::parse("https://example.com/page/").unwrap()
    }

    #[test]
    fn placeholder_detects_known_markers() {
        assert!(is_placeholder(""));
        assert!(is_placeholder("data:image/png;base64,abc"));
        assert!(is_placeholder("https://cdn.example.com/spacer.gif"));
        assert!(is_placeholder("/assets/1x1.png"));
        assert!(!is_placeholder("https://cdn.example.com/photo.jpg"));
    }

    #[test]
    fn resolve_srcset_preserves_descriptors() {
        let out = resolve_srcset("a.jpg 1x, /b.jpg 2x", &base());
        assert_eq!(
            out,
            "https://example.com/page/a.jpg 1x, https://example.com/b.jpg 2x"
        );
    }

    #[test]
    fn lazy_image_promotes_data_src_when_placeholder() {
        let html = r#"<img src="spacer.gif" data-src="/real.jpg">"#;
        let out = resolve_lazy_images(html);
        assert!(out.contains(r#"src="/real.jpg""#), "got: {}", out);
    }

    #[test]
    fn lazy_image_keeps_real_src() {
        let html = r#"<img src="/real.jpg" data-src="/other.jpg">"#;
        let out = resolve_lazy_images(html);
        assert!(out.contains(r#"src="/real.jpg""#));
    }

    #[test]
    fn strip_base64_removes_data_uri_imgs() {
        let html = r#"<img src="data:image/png;base64,xxx"><img src="/keep.jpg">"#;
        let out = strip_base64_images(html);
        assert!(!out.contains("data:image"));
        assert!(out.contains("/keep.jpg"));
    }

    #[test]
    fn resolve_image_links_makes_absolute() {
        let html = r#"<img src="img.jpg"><source srcset="a.jpg 1x">"#;
        let out = resolve_image_links(html, &base());
        assert!(out.contains("https://example.com/page/img.jpg"));
        assert!(out.contains("https://example.com/page/a.jpg 1x"));
    }
}
