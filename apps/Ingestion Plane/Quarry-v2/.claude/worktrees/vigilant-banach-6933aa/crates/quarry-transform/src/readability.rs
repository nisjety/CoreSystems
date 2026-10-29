//! Readability-first content extraction.
//!
//! Strips boilerplate (nav, footer, sidebars, scripts) and selects the densest
//! content subtree before handing off to the markdown converter. The algorithm
//! is a small subset of Mozilla Readability:
//!
//!   1. Find candidate subtrees rooted at `<article>`, `<main>`, `[role=main]`,
//!      `#content`, `#main`, or any block-level element with paragraph-rich
//!      descendants.
//!   2. Score each candidate by paragraph length, paragraph count, and a
//!      link-density penalty. High link density signals navigation, not body.
//!   3. Render the highest-scoring subtree's HTML, with structural noise
//!      (`<script>`, `<style>`, `<noscript>`, `<iframe>`, `<svg>`) and
//!      boilerplate containers (`<nav>`, `<header>`, `<footer>`, `<aside>`)
//!      removed.
//!
//! Falls back to the document body when no candidate scores positively.

use scraper::{ElementRef, Html, Selector};

/// Result of readability extraction. `content_html` is a sanitized HTML
/// fragment suitable for `html2md::parse_html`.
#[derive(Debug, Clone)]
pub struct Readable {
    pub title: Option<String>,
    pub byline: Option<String>,
    pub content_html: String,
}

/// Tag names whose content is unconditionally stripped from the chosen subtree.
const STRIP_TAGS: &[&str] = &[
    "script", "style", "noscript", "iframe", "svg",
    "nav", "header", "footer", "aside",
];

/// Extract the main content from a document. Returns `None` only if the input
/// cannot be parsed as HTML (always returns `Some` for well-formed documents,
/// falling back to body when no candidate scores positively).
pub fn extract(html: &str) -> Option<Readable> {
    let doc = Html::parse_document(html);
    let title = extract_title(&doc);
    let byline = extract_byline(&doc);

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

    let content_html = chosen
        .map(|el| sanitize_html(&el.html()))
        .unwrap_or_else(|| sanitize_html(html));

    Some(Readable {
        title,
        byline,
        content_html,
    })
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
    let is_role_main = el.value().attr("role").map_or(false, |r| r == "main");
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

/// Strip noise tags from an HTML fragment by removing complete tag pairs
/// (`<script>...</script>`, `<nav>...</nav>`, etc.). Self-closing forms
/// and stray opening tags are also handled.
fn sanitize_html(html: &str) -> String {
    let mut current = html.to_string();
    for tag in STRIP_TAGS {
        current = strip_tag(&current, tag);
    }
    current
}

/// Remove all `<tag ...>...</tag>` regions case-insensitively. Also removes
/// self-closing `<tag .../>` forms. Stops at end-of-string when a closing tag
/// is missing rather than panicking.
fn strip_tag(html: &str, tag: &str) -> String {
    let lower = html.to_ascii_lowercase();
    let open_lc = format!("<{tag}");
    let close_lc = format!("</{tag}>");
    let mut out = String::with_capacity(html.len());
    let mut cursor = 0usize;

    while let Some(rel) = lower[cursor..].find(&open_lc) {
        let abs = cursor + rel;
        // Confirm this is a tag boundary, not a longer name (e.g. `<scriptish`).
        let after = lower[abs + open_lc.len()..].chars().next();
        let is_tag =
            matches!(after, Some(c) if c == '>' || c == ' ' || c == '\t' || c == '\n' || c == '/' );
        if !is_tag {
            out.push_str(&html[cursor..=abs]);
            cursor = abs + 1;
            continue;
        }
        // Append everything before the open tag.
        out.push_str(&html[cursor..abs]);

        // Find the end of the open tag '>'.
        let Some(open_end_rel) = html[abs..].find('>') else {
            // Malformed: drop the rest.
            return out;
        };
        let open_end = abs + open_end_rel;

        // Self-closing? <tag/>
        if html[abs..=open_end].trim_end_matches('>').ends_with('/') {
            cursor = open_end + 1;
            continue;
        }

        // Find the matching closing tag.
        match lower[open_end + 1..].find(&close_lc) {
            Some(rel_close) => {
                let close_start = open_end + 1 + rel_close;
                cursor = close_start + close_lc.len();
            }
            None => {
                // Missing close — drop everything from here on.
                return out;
            }
        }
    }
    out.push_str(&html[cursor..]);
    out
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
}
