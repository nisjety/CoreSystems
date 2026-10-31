//! Namespace-aware sitemap parsing (roxmltree). No DTD or external entity
//! expansion; malformed or unsupported XML is reported as `valid == false`
//! and an over-long document as `truncated == true`, never as a complete map.
//!
//! Supports both `<urlset>` (regular sitemaps) and `<sitemapindex>` (nested
//! indexes). Returns a flat list of URLs and a list of nested sitemap URLs
//! that the caller should fetch and parse next.
//!
//! For discovery (E3.7.4): [`parse_body`] and [`decode_body`] accept the
//! fetched bytes and inflate a gzip body (`.xml.gz`) under a size cap, and
//! [`sitemap_urls_from_robots`] reads the `Sitemap:` lines of a robots.txt
//! body. See `docs/SITEMAP_PARSING.md`.
use chrono::{DateTime, Utc};
use std::collections::HashSet;
use std::io::Read;

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

/// Largest decoded sitemap accepted. The sitemap protocol caps one sitemap
/// file at 50 MiB uncompressed; a gzip body that inflates past this is refused
/// rather than read.
pub const MAX_DECODED_BYTES: usize = 50 * 1024 * 1024;

/// Why a fetched sitemap body could not be turned into text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SitemapBodyError {
    /// The body starts with the gzip magic bytes but is not a valid gzip stream
    /// (corrupt, or cut short).
    Gzip,
    /// The decoded body is larger than the decode limit.
    TooLarge,
}

/// Parse a fetched sitemap body. A gzip body (detected by its magic bytes, so
/// `.xml.gz` works whatever the URL says) is inflated first, with the decoded
/// size capped at [`MAX_DECODED_BYTES`]. Any other body is read as UTF-8,
/// lossily. The caller bounds the compressed size of the fetch.
pub fn parse_body(body: &[u8]) -> Result<Sitemap, SitemapBodyError> {
    let text = decode_body(body)?;
    Ok(parse(&text))
}

/// Inflate a fetched sitemap body to text, or pass a non-gzip body through.
pub fn decode_body(body: &[u8]) -> Result<String, SitemapBodyError> {
    decode_body_limited(body, MAX_DECODED_BYTES)
}

fn decode_body_limited(body: &[u8], limit: usize) -> Result<String, SitemapBodyError> {
    if !is_gzip(body) {
        return Ok(String::from_utf8_lossy(body).into_owned());
    }
    // Read one byte past the limit so an over-long body is detected, not truncated.
    let cap = (limit as u64).saturating_add(1);
    let mut out = Vec::new();
    flate2::read::MultiGzDecoder::new(body)
        .take(cap)
        .read_to_end(&mut out)
        .map_err(|_| SitemapBodyError::Gzip)?;
    if out.len() > limit {
        return Err(SitemapBodyError::TooLarge);
    }
    Ok(String::from_utf8_lossy(&out).into_owned())
}

fn is_gzip(body: &[u8]) -> bool {
    body.len() >= 2 && body[0] == 0x1f && body[1] == 0x8b
}

/// Sitemap URLs named by `Sitemap:` lines in a robots.txt body, in file order
/// and without duplicates.
///
/// RFC 9309 section 2.4: the line is not scoped to a User-agent group, so every
/// line counts. The field name is matched case-insensitively, a `#` starts a
/// comment, and a BOM at the start is ignored. Only absolute `http` and `https`
/// URLs are kept, returned in normalised form. Relative, empty, unparseable and
/// other-scheme values are dropped.
pub fn sitemap_urls_from_robots(robots: &str) -> Vec<String> {
    let body = robots.strip_prefix('\u{feff}').unwrap_or(robots);
    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<String> = Vec::new();
    for raw in body.lines() {
        let line = match raw.find('#') {
            Some(i) => &raw[..i],
            None => raw,
        };
        let Some((field, value)) = line.split_once(':') else {
            continue;
        };
        if !field.trim().eq_ignore_ascii_case("sitemap") {
            continue;
        }
        let Ok(url) = url::Url::parse(value.trim()) else {
            continue;
        };
        if !matches!(url.scheme(), "http" | "https") {
            continue;
        }
        let normalised = url.to_string();
        if seen.insert(normalised.clone()) {
            out.push(normalised);
        }
    }
    out
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

    const URLSET: &str = "<urlset><url><loc>https://example.com/a</loc></url><url><loc>https://example.com/b</loc></url></urlset>";

    fn gzip(bytes: &[u8]) -> Vec<u8> {
        use std::io::Write;
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(bytes).unwrap();
        enc.finish().unwrap()
    }

    #[test]
    fn gzip_urlset_parses_like_plain() {
        let sm = parse_body(&gzip(URLSET.as_bytes())).expect("valid gzip");
        assert_eq!(sm.urls.len(), 2);
        assert_eq!(sm.urls[0].loc, "https://example.com/a");
        assert_eq!(sm.urls[1].loc, "https://example.com/b");
    }

    #[test]
    fn plain_body_parses_like_parse() {
        let sm = parse_body(URLSET.as_bytes()).expect("plain body");
        assert_eq!(sm, parse(URLSET));
    }

    #[test]
    fn gzip_sitemap_index_yields_nested() {
        let idx = "<sitemapindex><sitemap><loc>https://example.com/s1.xml.gz</loc></sitemap></sitemapindex>";
        let sm = parse_body(&gzip(idx.as_bytes())).expect("valid gzip");
        assert!(sm.urls.is_empty());
        assert_eq!(sm.nested, vec!["https://example.com/s1.xml.gz".to_string()]);
    }

    #[test]
    fn concatenated_gzip_members_are_all_read() {
        // Members concatenate into one stream: one document split across two.
        let mut body = gzip(b"<urlset><url><loc>https://example.com/a</loc></url>");
        body.extend(gzip(b"<url><loc>https://example.com/b</loc></url></urlset>"));
        let sm = parse_body(&body).expect("two valid members");
        assert_eq!(sm.urls.len(), 2);
    }

    #[test]
    fn truncated_gzip_is_refused() {
        let full = gzip(URLSET.as_bytes());
        let cut = &full[..full.len() / 2];
        assert_eq!(parse_body(cut).unwrap_err(), SitemapBodyError::Gzip);
    }

    #[test]
    fn gzip_magic_with_garbage_is_refused() {
        let body = [0x1f, 0x8b, 0x00, 0x01, 0x02, 0x03];
        assert_eq!(decode_body(&body), Err(SitemapBodyError::Gzip));
    }

    #[test]
    fn decoded_size_over_limit_is_refused() {
        let body = gzip(&vec![b'a'; 4096]);
        assert_eq!(
            decode_body_limited(&body, 4096)
                .expect("at the limit")
                .len(),
            4096
        );
        assert_eq!(
            decode_body_limited(&body, 4095),
            Err(SitemapBodyError::TooLarge)
        );
    }

    #[test]
    fn robots_sitemap_lines_in_order_and_deduped() {
        let robots = "User-agent: *\nDisallow: /x\nSitemap: https://example.com/a.xml\nsitemap:https://example.com/b.xml\r\nSITEMAP:   https://example.com/a.xml\n";
        assert_eq!(
            sitemap_urls_from_robots(robots),
            vec!["https://example.com/a.xml", "https://example.com/b.xml"]
        );
    }

    #[test]
    fn robots_comments_and_bad_values_are_dropped() {
        let robots = "# Sitemap: https://example.com/commented.xml\nSitemap: /relative.xml\nSitemap: ftp://example.com/ftp.xml\nSitemap:\nSitemap https://example.com/nocolon.xml\nSitemap: https://example.com/ok.xml # trailing note\n";
        assert_eq!(
            sitemap_urls_from_robots(robots),
            vec!["https://example.com/ok.xml"]
        );
    }

    #[test]
    fn robots_bom_and_no_trailing_newline() {
        let robots = "\u{feff}Sitemap: https://example.com/bom.xml";
        assert_eq!(
            sitemap_urls_from_robots(robots),
            vec!["https://example.com/bom.xml"]
        );
    }

    #[test]
    fn robots_without_sitemap_lines_is_empty() {
        assert!(sitemap_urls_from_robots("User-agent: *\nDisallow:\n").is_empty());
        assert!(sitemap_urls_from_robots("").is_empty());
    }
}
