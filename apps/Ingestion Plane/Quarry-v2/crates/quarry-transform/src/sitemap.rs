//! Minimal sitemap.xml parser.
//!
//! Supports both `<urlset>` (regular sitemaps) and `<sitemapindex>` (nested
//! indexes). Returns a flat list of URLs and a list of nested sitemap URLs
//! that the caller should fetch and parse next.

use chrono::{DateTime, Utc};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Sitemap {
    pub urls: Vec<SitemapEntry>,
    pub nested: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SitemapEntry {
    pub loc: String,
    pub lastmod: Option<DateTime<Utc>>,
    pub changefreq: Option<String>,
    pub priority: Option<String>,
}

/// Parse a sitemap XML document. Tolerant of malformed input — returns an
/// empty `Sitemap` rather than failing, since sitemaps are advisory.
pub fn parse(xml: &str) -> Sitemap {
    let lower = xml.to_lowercase();
    let is_index = lower.contains("<sitemapindex");
    if is_index {
        parse_index(xml)
    } else {
        parse_urlset(xml)
    }
}

fn parse_urlset(xml: &str) -> Sitemap {
    let mut sm = Sitemap::default();
    for url_block in extract_blocks(xml, "<url>", "</url>") {
        let loc = match extract_inner(url_block, "<loc>", "</loc>") {
            Some(s) => s.trim().to_string(),
            None => continue,
        };
        let lastmod = extract_inner(url_block, "<lastmod>", "</lastmod>")
            .and_then(|s| DateTime::parse_from_rfc3339(s.trim()).ok())
            .map(|dt| dt.with_timezone(&Utc));
        let changefreq =
            extract_inner(url_block, "<changefreq>", "</changefreq>").map(|s| s.trim().to_string());
        let priority =
            extract_inner(url_block, "<priority>", "</priority>").map(|s| s.trim().to_string());
        sm.urls.push(SitemapEntry {
            loc,
            lastmod,
            changefreq,
            priority,
        });
    }
    sm
}

fn parse_index(xml: &str) -> Sitemap {
    let mut sm = Sitemap::default();
    for sitemap_block in extract_blocks(xml, "<sitemap>", "</sitemap>") {
        if let Some(loc) = extract_inner(sitemap_block, "<loc>", "</loc>") {
            sm.nested.push(loc.trim().to_string());
        }
    }
    sm
}

fn extract_blocks<'a>(xml: &'a str, open: &str, close: &str) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut cursor = 0;
    while cursor < xml.len() {
        let Some(start) = xml[cursor..].find(open) else {
            break;
        };
        let abs_start = cursor + start + open.len();
        let Some(end_rel) = xml[abs_start..].find(close) else {
            break;
        };
        let abs_end = abs_start + end_rel;
        out.push(&xml[abs_start..abs_end]);
        cursor = abs_end + close.len();
    }
    out
}

fn extract_inner<'a>(xml: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let s = xml.find(open)? + open.len();
    let e_rel = xml[s..].find(close)?;
    Some(&xml[s..s + e_rel])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_urlset() {
        let xml = r#"<?xml version="1.0"?><urlset>
            <url>
                <loc>https://example.com/a</loc>
                <lastmod>2026-05-01T00:00:00Z</lastmod>
                <changefreq>weekly</changefreq>
                <priority>0.8</priority>
            </url>
            <url>
                <loc>https://example.com/b</loc>
            </url>
        </urlset>"#;
        let sm = parse(xml);
        assert_eq!(sm.urls.len(), 2);
        assert_eq!(sm.urls[0].loc, "https://example.com/a");
        assert!(sm.urls[0].lastmod.is_some());
        assert_eq!(sm.urls[0].changefreq.as_deref(), Some("weekly"));
        assert_eq!(sm.urls[0].priority.as_deref(), Some("0.8"));
        assert!(sm.urls[1].lastmod.is_none());
        assert!(sm.nested.is_empty());
    }

    #[test]
    fn parses_sitemap_index() {
        let xml = r#"<?xml version="1.0"?><sitemapindex>
            <sitemap><loc>https://example.com/sm1.xml</loc></sitemap>
            <sitemap><loc>https://example.com/sm2.xml</loc></sitemap>
        </sitemapindex>"#;
        let sm = parse(xml);
        assert!(sm.urls.is_empty());
        assert_eq!(sm.nested.len(), 2);
        assert_eq!(sm.nested[0], "https://example.com/sm1.xml");
    }

    #[test]
    fn empty_xml_returns_empty_sitemap() {
        let sm = parse("");
        assert!(sm.urls.is_empty());
        assert!(sm.nested.is_empty());
    }

    #[test]
    fn malformed_xml_does_not_panic() {
        let sm = parse("<urlset><url><loc>https://example.com/a</loc>");
        // Block extractor requires both tags; missing </url> means no entries
        assert!(sm.urls.is_empty());
    }

    #[test]
    fn skips_url_block_without_loc() {
        let xml = "<urlset><url><lastmod>2026-01-01T00:00:00Z</lastmod></url></urlset>";
        let sm = parse(xml);
        assert!(sm.urls.is_empty());
    }
}
