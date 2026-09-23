//! Readability-first content extraction.
//!
//! Strips boilerplate (nav, footer, sidebars, scripts, images) and collapses
//! duplicated responsive-CSS content before selecting the densest content
//! subtree and handing off to the markdown converter. The algorithm is a
//! small subset of Mozilla Readability:
//!
//!   1. Strip structural noise (`<script>`, `<style>`, `<noscript>`,
//!      `<iframe>`, `<svg>`) and boilerplate — both tag-name-based
//!      (`<nav>`, `<header>`, `<footer>`, `<aside>`) and ARIA-role-based
//!      (`role="navigation"` etc, for component-framework markup with no
//!      matching semantic tag) — plus `<img>`/`<picture>`/`<source>`, whose
//!      raw CDN URLs are noise in a text-retrieval chunk.
//!   2. Collapse sibling elements with identical, non-trivial text — the
//!      CSS-only mobile/desktop dual-render pattern common on Tailwind-style
//!      sites means a static (no-JS) fetch sees both variants at once.
//!   3. Find candidate subtrees rooted at `<article>`, `<main>`, `[role=main]`,
//!      `#content`, `#main`, or any block-level element with paragraph-rich
//!      descendants.
//!   4. Score each candidate by paragraph length, paragraph count, and a
//!      link-density penalty. High link density signals navigation, not body.
//!   5. Render the highest-scoring subtree's (already-cleaned) HTML.
//!
//! Falls back to the document body when no candidate scores positively.

use scraper::{ElementRef, Html, Selector};

use crate::charset::{self, charset_from_content_type, DecodedBody};

/// Result of readability extraction. `content_html` is a sanitized HTML
/// fragment suitable for `html2md::parse_html`.
#[derive(Debug, Clone)]
pub struct Readable {
    pub title: Option<String>,
    pub byline: Option<String>,
    pub content_html: String,
}

/// Elements unconditionally stripped from the document before candidate
/// scoring: tag-name boilerplate (script/style/noscript/iframe/svg/nav/
/// header/footer/aside) plus ARIA landmark roles. The role selectors matter
/// because modern component-framework markup (Next.js/React and similar)
/// very often renders navigation/footer as plain `<div role="navigation">`
/// wrappers with no matching semantic tag at all — the tag-only list misses
/// those entirely, so a real-world app router page falls through to the
/// `body` candidate with its nav (frequently duplicated once for a mobile
/// menu, once for desktop) still inside it. `img`/`picture`/`source` are
/// stripped too: html2md renders every surviving `<img>` as a literal
/// `![](src)`, and CDN-proxied thumbnail URLs (e.g. Next's
/// `/_next/image?url=...`) are pure noise in a chunk that only ever feeds
/// text retrieval, never gets rendered as an image.
const STRIP_SELECTOR: &str = "script, style, noscript, iframe, svg, nav, header, footer, aside, \
     [role=\"navigation\"], [role=\"banner\"], [role=\"contentinfo\"], [role=\"complementary\"], \
     img, picture, source";

/// Detach every element matching [`STRIP_SELECTOR`] from `doc`'s tree.
/// Runs before candidate scoring (not just before final serialization) so
/// the link-density penalty in [`score_element`] no longer counts links
/// that live inside boilerplate a tag-name-only pass would have missed.
fn strip_boilerplate(doc: &mut Html) {
    let Ok(sel) = Selector::parse(STRIP_SELECTOR) else {
        return;
    };
    let ids: std::collections::HashSet<_> = doc.select(&sel).map(|el| el.id()).collect();
    for id in ids {
        if let Some(mut node) = doc.tree.get_mut(id) {
            node.detach();
        }
    }
}

/// Below this length a text match is treated as coincidental (e.g. two
/// unrelated short labels) rather than a duplicated content block, so
/// [`dedupe_responsive_siblings`] leaves it alone.
const MIN_DEDUPE_CHARS: usize = 40;

/// Detach the second-and-later occurrence whenever two element children of
/// the SAME parent render identical, non-trivial visible text.
///
/// Real-world component-framework sites very commonly render two full
/// copies of a section — one for a mobile breakpoint, one for desktop —
/// as CSS-only sibling variants (Tailwind's `lg:hidden` next to a `hidden
/// lg:grid` sibling, for example) rather than conditionally rendering just
/// one. A static HTML fetch (no JS, no viewport) sees both, so identical
/// hero/heading/nav content ends up duplicated verbatim in the extracted
/// markdown. Comparing text is enough to catch this regardless of the
/// specific responsive-CSS convention a given site uses — it doesn't
/// depend on recognizing `lg:hidden`-style classes at all.
fn dedupe_responsive_siblings(doc: &mut Html) {
    let Ok(all_sel) = Selector::parse("*") else {
        return;
    };
    let mut to_remove = std::collections::HashSet::new();

    for parent in doc.select(&all_sel) {
        let mut seen = std::collections::HashSet::new();
        for child in parent.children().filter_map(ElementRef::wrap) {
            let text = visible_text(&child);
            if text.chars().count() < MIN_DEDUPE_CHARS {
                continue;
            }
            if !seen.insert(text) {
                to_remove.insert(child.id());
            }
        }
    }

    for id in to_remove {
        if let Some(mut node) = doc.tree.get_mut(id) {
            node.detach();
        }
    }
}

/// Extract the main content from a document. Returns `None` only if the input
/// cannot be parsed as HTML (always returns `Some` for well-formed documents,
/// falling back to body when no candidate scores positively).
pub fn extract(html: &str) -> Option<Readable> {
    let mut doc = Html::parse_document(html);
    let title = extract_title(&doc);
    let byline = extract_byline(&doc);

    strip_boilerplate(&mut doc);
    dedupe_responsive_siblings(&mut doc);

    let candidates = collect_candidates(&doc);
    let best = candidates
        .iter()
        .map(|el| (score_element(*el), *el))
        .filter(|(s, _)| *s > 0.0)
        .max_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(_, el)| el);

    let chosen = best.or_else(|| {
        let sel = Selector::parse("body").ok()?;
        doc.select(&sel).next()
    });

    let content_html = chosen.map(|el| el.html()).unwrap_or_else(|| doc.html());

    Some(Readable {
        title,
        byline,
        content_html,
    })
}

/// How far into the document we look for a `<meta charset>` declaration.
/// The HTML spec only obliges a browser's pre-scan to cover the first 1024
/// bytes, but we decode once, offline, with the whole body already in hand —
/// and older public-sector pages routinely push the meta tag past 1 KB behind
/// IE conditional comments and inline shims, so a 1 KB window would miss
/// exactly the legacy documents that still depend on the declaration.
const META_CHARSET_SCAN_BYTES: usize = 4096;

/// Decode a raw HTTP response body into HTML text, taking the encoding from
/// (in order) the `Content-Type` header's `charset=`, the document's own
/// `<meta charset>` / `<meta http-equiv="Content-Type">` declaration, and
/// finally a `chardetng` byte sniff.
///
/// Exists because fetchers that did `str::from_utf8(&bytes).ok()` dropped
/// every ISO-8859-1 / windows-1252 page on the floor as if it had no content
/// at all — still the common case on Norwegian municipal and older
/// public-sector sites. Decoding here is lossy rather than fallible: a page
/// with a handful of undecodable bytes is worth far more than no page, and
/// callers that care can measure the `U+FFFD` share of the result.
pub fn decode_html_body(body: &[u8], content_type: Option<&str>) -> DecodedBody {
    if let Some(label) = content_type.and_then(charset_from_content_type) {
        // Not blind trust: `charset::decode` sanity-checks the declared
        // label against a prefix decode and sniffs anyway when it is a lie.
        return charset::decode(body, Some(label));
    }
    let window = &body[..body.len().min(META_CHARSET_SCAN_BYTES)];
    if let Some(label) = meta_charset(window) {
        return charset::decode(body, Some(&label));
    }
    charset::decode(body, None)
}

/// Find the encoding label declared by a `<meta>` tag inside `prefix`.
///
/// One pass handles both spellings — `<meta charset="...">` and the legacy
/// `<meta http-equiv="Content-Type" content="text/html; charset=...">` —
/// because both put the literal `charset=` inside the tag and differ only in
/// the surrounding quoting.
fn meta_charset(prefix: &[u8]) -> Option<String> {
    let lower = prefix.to_ascii_lowercase();
    let mut from = 0usize;
    while let Some(rel) = find(&lower[from..], b"<meta") {
        let tag_start = from + rel;
        let tag_end = find(&lower[tag_start..], b">")
            .map(|n| tag_start + n)
            .unwrap_or(lower.len());
        if let Some(label) = charset_attr_value(&lower[tag_start..tag_end]) {
            return Some(label);
        }
        from = tag_end.max(tag_start + 1);
    }
    None
}

/// Read the `charset=` value out of one already-lowercased `<meta …>` tag.
fn charset_attr_value(tag: &[u8]) -> Option<String> {
    const KEY: &[u8] = b"charset=";
    let mut from = 0usize;
    while let Some(rel) = find(&tag[from..], KEY) {
        let at = from + rel;
        // A real declaration is always preceded by a delimiter; requiring
        // that keeps framework attributes like `data-charset=` from being
        // mistaken for the document's own encoding.
        let inside_longer_attr_name = at
            .checked_sub(1)
            .is_some_and(|i| tag[i].is_ascii_alphanumeric() || tag[i] == b'-' || tag[i] == b'_');
        if !inside_longer_attr_name {
            let value: Vec<u8> = tag[at + KEY.len()..]
                .iter()
                .copied()
                .skip_while(|b| matches!(b, b'"' | b'\'' | b' '))
                .take_while(|b| !matches!(b, b'"' | b'\'' | b' ' | b';' | b'/' | b'>'))
                .collect();
            if !value.is_empty() {
                return String::from_utf8(value).ok();
            }
        }
        from = at + KEY.len();
    }
    None
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Convenience: extract and stringify directly to markdown via `html2md`.
pub fn html_to_readable_markdown(html: &str) -> String {
    match extract(html) {
        Some(r) => crate::markdown::html_to_markdown(&r.content_html),
        None => crate::markdown::html_to_markdown(html),
    }
}

fn extract_title(doc: &Html) -> Option<String> {
    if let Ok(sel) = Selector::parse("meta[property='og:title']") {
        if let Some(el) = doc.select(&sel).next() {
            if let Some(c) = el.value().attr("content") {
                let t = c.trim();
                if !t.is_empty() {
                    return Some(t.to_string());
                }
            }
        }
    }
    if let Ok(sel) = Selector::parse("title") {
        if let Some(el) = doc.select(&sel).next() {
            let t: String = el.text().collect::<String>().trim().to_string();
            if !t.is_empty() {
                return Some(t);
            }
        }
    }
    if let Ok(sel) = Selector::parse("h1") {
        if let Some(el) = doc.select(&sel).next() {
            let t: String = el.text().collect::<String>().trim().to_string();
            if !t.is_empty() {
                return Some(t);
            }
        }
    }
    None
}

fn extract_byline(doc: &Html) -> Option<String> {
    let candidates = [
        "meta[name='author']",
        "meta[property='article:author']",
        "[rel='author']",
        ".byline",
        ".author",
    ];
    for q in candidates {
        let Ok(sel) = Selector::parse(q) else {
            continue;
        };
        if let Some(el) = doc.select(&sel).next() {
            if let Some(c) = el.value().attr("content") {
                let t = c.trim();
                if !t.is_empty() {
                    return Some(t.to_string());
                }
            }
            let t: String = el.text().collect::<String>().trim().to_string();
            if !t.is_empty() {
                return Some(t);
            }
        }
    }
    None
}

fn collect_candidates<'a>(doc: &'a Html) -> Vec<ElementRef<'a>> {
    let queries = [
        "article",
        "main",
        "[role=main]",
        "#content",
        "#main",
        "#article",
        ".article",
        ".post",
        ".entry-content",
    ];
    let mut out = Vec::new();
    for q in queries {
        let Ok(sel) = Selector::parse(q) else {
            continue;
        };
        for el in doc.select(&sel) {
            out.push(el);
        }
    }
    // Always include body as a fallback candidate.
    if let Ok(sel) = Selector::parse("body") {
        if let Some(body) = doc.select(&sel).next() {
            out.push(body);
        }
    }
    out
}

/// Semantic content element names that get a scoring bonus. These are
/// explicit authorial signals that the subtree contains the main content.
const SEMANTIC_CONTENT_TAGS: &[&str] = &["article", "main"];

/// Score = sum(paragraph text length) * (1 - link_density), with a small
/// bonus per paragraph, a semantic element bonus, and a penalty when the
/// subtree is dominated by links.
fn score_element(el: ElementRef<'_>) -> f64 {
    let text_len = visible_text(&el).len() as f64;
    if text_len < 50.0 {
        return 0.0;
    }

    let para_sel = Selector::parse("p").unwrap();
    let paragraphs: Vec<ElementRef<'_>> = el.select(&para_sel).collect();
    let para_chars: usize = paragraphs
        .iter()
        .map(|p| p.text().collect::<String>().trim().chars().count())
        .filter(|n| *n >= 25) // ignore micro paragraphs
        .sum();

    let link_chars = link_text_length(&el);
    let density = if text_len > 0.0 {
        (link_chars as f64 / text_len).min(1.0)
    } else {
        0.0
    };

    let tag = el.value().name().to_ascii_lowercase();
    let is_role_main = el.value().attr("role") == Some("main");
    let semantic_bonus = if SEMANTIC_CONTENT_TAGS.contains(&tag.as_str()) || is_role_main {
        200.0
    } else {
        0.0
    };

    let base = para_chars as f64;
    let para_bonus = (paragraphs.len() as f64).min(20.0) * 8.0;
    (base + para_bonus + semantic_bonus) * (1.0 - density)
}

fn visible_text(el: &ElementRef<'_>) -> String {
    el.text()
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn link_text_length(el: &ElementRef<'_>) -> usize {
    let sel = Selector::parse("a").unwrap();
    el.select(&sel)
        .map(|a| a.text().collect::<String>().chars().count())
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = r#"
<!DOCTYPE html>
<html>
<head>
  <title>Sample Article</title>
  <meta name="author" content="Jane Roe">
</head>
<body>
  <header>
    <nav><a href="/a">A</a> <a href="/b">B</a> <a href="/c">C</a></nav>
  </header>
  <main>
    <article>
      <h1>Sample Article</h1>
      <p>This is a substantive paragraph with enough body text to register against the
         readability scoring threshold and survive the minimum length check used by the
         extractor when ranking candidate subtrees against each other.</p>
      <p>A second paragraph keeps the score above the body fallback so the article subtree
         wins out over the unstructured navigation links nearby.</p>
    </article>
  </main>
  <aside>related links: <a href="/x">x</a> <a href="/y">y</a></aside>
  <footer>copyright</footer>
</body>
</html>"#;

    #[test]
    fn extracts_title_and_author() {
        let r = extract(PAGE).expect("readable");
        assert_eq!(r.title.as_deref(), Some("Sample Article"));
        assert_eq!(r.byline.as_deref(), Some("Jane Roe"));
    }

    #[test]
    fn drops_nav_and_footer() {
        let r = extract(PAGE).expect("readable");
        assert!(!r.content_html.contains("copyright"));
        assert!(!r.content_html.contains("related links"));
        assert!(r.content_html.contains("substantive paragraph"));
    }

    #[test]
    fn fallback_to_body_when_no_main_subtree() {
        let html = "<html><body><p>Only one short paragraph.</p></body></html>";
        let r = extract(html).expect("readable");
        assert!(r.content_html.contains("Only one short paragraph"));
    }

    #[test]
    fn readable_markdown_emits_paragraphs() {
        let md = html_to_readable_markdown(PAGE);
        assert!(md.contains("substantive paragraph"));
        // Nav links should be gone after readability filtering.
        assert!(!md.contains("copyright"));
    }

    // Modern component-framework sites (Next.js/React and similar) very
    // often render navigation/footer as plain `<div role="...">` wrappers
    // with no matching semantic tag — a real-world regression this session
    // traced to raw duplicated nav menus surviving into crawled markdown.
    // Mirrors PAGE but with div+role markup and a duplicated nav (mobile +
    // desktop toggle, the actual shape seen on the real site).
    const FRAMEWORK_PAGE: &str = r#"
<!DOCTYPE html>
<html>
<head><title>Framework Page</title></head>
<body>
  <div id="__next">
    <div role="banner">
      <div role="navigation" class="mobile-nav">
        <a href="/a">Chemistry</a> <a href="/b">Training</a>
      </div>
      <div role="navigation" class="desktop-nav">
        <a href="/a">Chemistry</a> <a href="/b">Training</a>
      </div>
    </div>
    <div>
      <h1>Cleaning Systems Service</h1>
      <p>This is a substantive paragraph with enough body text to register against the
         readability scoring threshold and survive the minimum length check used by the
         extractor when ranking candidate subtrees against each other.</p>
      <p>A second paragraph keeps the score above the boilerplate nav links nearby so the
         real content wins out even without a semantic main/article wrapper.</p>
      <img src="/_next/image?url=https%3A%2F%2Fcdn.sanity.io%2Fimages%2Fabc%2Fphoto.jpg">
    </div>
    <div role="contentinfo">copyright 2026</div>
  </div>
</body>
</html>"#;

    #[test]
    fn drops_role_based_nav_with_no_semantic_tag() {
        let r = extract(FRAMEWORK_PAGE).expect("readable");
        assert!(!r.content_html.contains("Chemistry"));
        assert!(!r.content_html.contains("Training"));
        assert!(!r.content_html.contains("copyright"));
        assert!(r.content_html.contains("substantive paragraph"));
    }

    #[test]
    fn strips_img_tags_to_avoid_cdn_url_clutter() {
        let r = extract(FRAMEWORK_PAGE).expect("readable");
        assert!(!r.content_html.contains("<img"));
        assert!(!r.content_html.contains("cdn.sanity.io"));
    }

    #[test]
    fn framework_markdown_has_no_nav_or_image_noise() {
        let md = html_to_readable_markdown(FRAMEWORK_PAGE);
        assert!(md.contains("substantive paragraph"));
        assert!(!md.contains("Chemistry"));
        assert!(!md.contains("cdn.sanity.io"));
    }

    // Mirrors the real-world regression this session traced on a live
    // Tailwind marketing site: a hero section rendered TWICE as CSS-only
    // responsive siblings (`lg:hidden` next to `hidden lg:grid`), both
    // present in the static (no-JS) HTML a crawler actually fetches.
    const DUAL_RESPONSIVE_HERO_PAGE: &str = r#"
<!DOCTYPE html>
<html>
<head><title>Cleaning Systems</title></head>
<body>
  <main>
    <div aria-labelledby="Hero">
      <div class="mx-auto lg:hidden">
        <h1>Cleaning Systems</h1>
        <p>Profesjonelle rengjøringssystemer for matindustrien og hele verdikjeden vår.</p>
      </div>
      <div class="mx-auto hidden lg:grid">
        <h1>Cleaning Systems</h1>
        <p>Profesjonelle rengjøringssystemer for matindustrien og hele verdikjeden vår.</p>
      </div>
    </div>
    <p>This is the one genuinely unique paragraph on the page, long enough to score.</p>
  </main>
</body>
</html>"#;

    #[test]
    fn drops_duplicate_responsive_sibling_block() {
        let md = html_to_readable_markdown(DUAL_RESPONSIVE_HERO_PAGE);
        let occurrences = md.matches("Profesjonelle rengjøringssystemer").count();
        assert_eq!(
            occurrences, 1,
            "the mobile/desktop hero variant should collapse to one copy, got markdown: {md}"
        );
    }

    /// `æ ø å` and `é` as ISO-8859-1 / windows-1252 single bytes. These are
    /// not valid UTF-8, so a `str::from_utf8` fetcher saw the whole page as
    /// unreadable and reported "no content".
    const LATIN1_BODY: &[u8] =
        b"<html><head><title>Kommune</title></head><body><main><p>Kafeen \xe5pner \
          klokken ti. V\xe6rvarselet for \xf8ya er klart, og caf\xe9en holder \xe5pent \
          hele helgen.</p></main></body></html>";

    #[test]
    fn decodes_latin1_body_declared_in_content_type() {
        let out = decode_html_body(LATIN1_BODY, Some("text/html; charset=iso-8859-1"));
        assert!(out.from_header, "header label should be honoured");
        assert!(out.text.contains("åpner"));
        assert!(out.text.contains("Værvarselet"));
        assert!(out.text.contains("øya"));
        assert!(out.text.contains("café"));
    }

    #[test]
    fn decodes_latin1_body_declared_only_in_meta_tag() {
        let body = b"<html><head><meta charset=\"ISO-8859-1\"><title>K</title></head>\
             <body><p>Kafeen \xe5pner, og v\xe6rvarselet for \xf8ya er klart.</p></body></html>";
        // No Content-Type at all — the document's own declaration is the
        // only signal, which is the shape a bare file/proxy fetch has.
        let out = decode_html_body(body, None);
        assert!(out.text.contains("åpner"));
        assert!(out.text.contains("værvarselet"));
        assert!(out.text.contains("øya"));
    }

    #[test]
    fn decodes_via_legacy_http_equiv_meta() {
        let label = meta_charset(
            b"<html><head><meta http-equiv=\"Content-Type\" \
              content=\"text/html; charset=windows-1252\"></head>",
        );
        assert_eq!(label.as_deref(), Some("windows-1252"));
    }

    #[test]
    fn meta_charset_ignores_lookalike_attribute_names() {
        // `data-charset` is a framework attribute, not a declaration; picking
        // it up would decode the page with whatever the app happened to store.
        let label = meta_charset(b"<meta name=\"x\" data-charset=\"gbk\">");
        assert_eq!(label, None);
        let label = meta_charset(b"<meta data-charset=\"gbk\" charset=\"utf-8\">");
        assert_eq!(label.as_deref(), Some("utf-8"));
    }

    #[test]
    fn decode_falls_back_to_sniff_with_no_header_and_no_meta() {
        let out = decode_html_body(LATIN1_BODY, None);
        assert!(!out.from_header);
        assert!(out.text.contains("åpner"), "sniff should recover latin-1");
    }

    #[test]
    fn utf8_body_round_trips_unchanged() {
        let body = "<html><body><p>Profesjonelle løsninger for næringen.</p></body></html>";
        let out = decode_html_body(body.as_bytes(), Some("text/html; charset=utf-8"));
        assert_eq!(out.text, body);
        assert_eq!(out.encoding, "UTF-8");
    }

    #[test]
    fn dedupe_leaves_short_coincidental_matches_alone() {
        // Two unrelated short labels ("Kontakt") under the same parent must
        // NOT be collapsed — only non-trivial (>= MIN_DEDUPE_CHARS) matches
        // count as a duplicated content block.
        let html = r#"<html><body><main><div><span>Kontakt</span><span>Kontakt</span>
            <p>A perfectly normal paragraph long enough to score on its own merits here.</p>
            </div></main></body></html>"#;
        let r = extract(html).expect("readable");
        assert_eq!(r.content_html.matches("Kontakt").count(), 2);
    }
}
