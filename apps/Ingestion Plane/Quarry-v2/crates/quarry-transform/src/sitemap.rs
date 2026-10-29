//! Namespace-aware sitemap parsing. No DTD or external entity expansion.
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Sitemap { pub urls: Vec<SitemapEntry>, pub nested: Vec<String>, pub valid: bool, pub truncated: bool }
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SitemapEntry { pub loc: String, pub lastmod: Option<DateTime<Utc>>, pub changefreq: Option<String>, pub priority: Option<String> }

/// Malformed/unsupported XML is reported as invalid, never as a complete map.
pub fn parse(xml: &str) -> Sitemap {
    let options = roxmltree::ParsingOptions { allow_dtd: false, nodes_limit: 100_000 };
    let Ok(doc) = roxmltree::Document::parse_with_options(xml, options) else { return Sitemap::default(); };
    let root = doc.root_element();
    let index = root.tag_name().name() == "sitemapindex";
    if !index && root.tag_name().name() != "urlset" { return Sitemap::default(); }
    let mut sm = Sitemap { valid: true, ..Default::default() };
    for node in root.children().filter(|n| n.is_element() && n.tag_name().name() == if index { "sitemap" } else { "url" }) {
        if sm.urls.len() + sm.nested.len() >= 2000 { sm.truncated = true; break; }
        let field = |name: &str| node.children().find(|n| n.is_element() && n.tag_name().name() == name).and_then(|n| n.text()).map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned);
        let Some(loc) = field("loc").filter(|v| v.len() <= 4096) else { sm.truncated = true; continue; };
        if index { sm.nested.push(loc); } else {
            sm.urls.push(SitemapEntry { loc, lastmod: field("lastmod").and_then(|s| DateTime::parse_from_rfc3339(&s).ok()).map(|d| d.with_timezone(&Utc)), changefreq: field("changefreq"), priority: field("priority") });
        }
    }
    sm
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namespaces_entities_and_dtd_are_handled_without_network_expansion() {
        let value=parse(r#"<s:urlset xmlns:s="urn:sitemap"><s:url><s:loc>https://example.com/a?x=1&amp;y=2</s:loc></s:url></s:urlset>"#);
        assert!(value.valid);assert_eq!(value.urls[0].loc,"https://example.com/a?x=1&y=2");
        assert!(!parse(r#"<!DOCTYPE urlset [<!ENTITY ext SYSTEM "file:///etc/passwd">]><urlset><url><loc>&ext;</loc></url></urlset>"#).valid);
    }
    #[test]
    fn entries_are_bounded_and_truncation_is_explicit() {
        let xml=format!("<urlset>{}</urlset>","<url><loc>https://example.com/a</loc></url>".repeat(2001));let sm=parse(&xml);assert!(sm.valid&&sm.truncated);assert_eq!(sm.urls.len(),2000);
    }
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
