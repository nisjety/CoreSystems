//! Soft-404 detection.
//!
//! A real-world annoyance: many sites return HTTP 200 with a body
//! whose visible content is "Page not found" or equivalent — usually
//! because the upstream router rewrote a missing path to a CMS error
//! template instead of issuing a proper 404 status. Downstream
//! consumers (indexers, RAG ingestion, the velion onboarding wizard)
//! that trust `status == 200` end up storing "Not Found" as the
//! authoritative snippet for what was supposed to be a product or
//! blog page.
//!
//! Heuristic — we only flag a page as a soft-404 when ALL of the
//! following hold:
//!
//! 1. HTTP status is in the 2xx range (we don't try to "rescue" a real
//!    404 here, only to detect a lying 200).
//! 2. The HTML body is small — under `MAX_SOFT_404_BYTES`. Real
//!    content pages are almost always larger; tiny 200s are usually
//!    error templates or empty placeholders.
//! 3. The visible text contains at least one phrase from a small
//!    multilingual keyword set ("not found", "siden finnes ikke",
//!    "página no encontrada", etc.), AND no strong "real content"
//!    signal like a populated `<article>` / `<main>` / multiple
//!    paragraphs.
//!
//! Returns `Some(reason)` when flagged, `None` when the page passes.
//! Conservative on purpose — false negatives are fine (caller still
//! gets the body), false positives are bad (caller throws away real
//! content that happened to mention "not found" somewhere).

use scraper::{Html, Selector};
use std::sync::OnceLock;

/// Max body size we consider for soft-404 classification. Real pages
/// almost always exceed this; tiny 200s are very likely error
/// templates. Tunable — start at 8 KB which covers most JSON error
/// payloads, plain-text 404s, and minimal HTML templates.
pub const MAX_SOFT_404_BYTES: usize = 8 * 1024;

/// Multilingual "page not found" phrases. Lower-case; we lowercase
/// the visible text before matching. Ordered by frequency — English
/// first since it dominates the open web, Nordic + major EU + Asian
/// scripts after for our target market.
const NOT_FOUND_PHRASES: &[&str] = &[
    // English
    "page not found",
    "404 not found",
    "404 error",
    "page does not exist",
    "page you requested could not be found",
    "page you are looking for",
    "we couldn't find that page",
    "we can't find the page",
    // Norwegian (bokmål + nynorsk)
    "siden finnes ikke",
    "fant ikke siden",
    "kunne ikke finne siden",
    "siden ble ikke funnet",
    // Swedish
    "sidan finns inte",
    "sidan kunde inte hittas",
    // Danish
    "siden findes ikke",
    "siden blev ikke fundet",
    // German
    "seite nicht gefunden",
    "seite existiert nicht",
    // French
    "page introuvable",
    "page non trouvée",
    "n'existe pas",
    // Spanish
    "página no encontrada",
    "página no existe",
    // Italian
    "pagina non trovata",
    // Dutch
    "pagina niet gevonden",
];

fn article_selector() -> &'static Selector {
    static SEL: OnceLock<Selector> = OnceLock::new();
    SEL.get_or_init(|| Selector::parse("article, main, [role=main]").expect("static selector"))
}

fn paragraph_selector() -> &'static Selector {
    static SEL: OnceLock<Selector> = OnceLock::new();
    SEL.get_or_init(|| Selector::parse("p").expect("static selector"))
}

/// Inspect HTTP status + decoded HTML body. Returns the soft-404
/// reason when the page is flagged, `None` otherwise.
///
/// `status` is the HTTP status code. `html` is the (decoded) HTML
/// body. `body_bytes` is the original byte length so we don't
/// re-encode for the size check.
pub fn detect(status: u16, body_bytes: usize, html: &str) -> Option<&'static str> {
    if !(200..300).contains(&status) {
        return None;
    }
    if body_bytes > MAX_SOFT_404_BYTES {
        return None;
    }
    if html.is_empty() {
        return Some("empty 200 body");
    }

    // Quick check: do we see ANY of the known not-found phrases?
    let lower = html.to_ascii_lowercase();
    let matched_phrase = NOT_FOUND_PHRASES.iter().find(|p| lower.contains(*p))?;

    // Counter-check: a long article/main with multiple <p> elements
    // suggests real content that just mentions "not found" in its
    // text. Bail on the soft-404 verdict in that case.
    let doc = Html::parse_document(html);
    let has_strong_main = doc.select(article_selector()).any(|el| {
        let text_len: usize = el.text().map(str::len).sum();
        text_len > 512
    });
    let paragraph_count = doc.select(paragraph_selector()).count();
    if has_strong_main || paragraph_count >= 4 {
        return None;
    }

    Some(matched_phrase)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_small_english_404() {
        let html = r#"<html><body><h1>Page not found</h1><p>Sorry.</p></body></html>"#;
        assert!(detect(200, html.len(), html).is_some());
    }

    #[test]
    fn flags_norwegian_404() {
        let html = r#"<html><body><h1>Siden finnes ikke</h1></body></html>"#;
        assert!(detect(200, html.len(), html).is_some());
    }

    #[test]
    fn ignores_non_2xx_even_with_phrase() {
        let html = r#"<html><body>Page not found</body></html>"#;
        assert!(detect(404, html.len(), html).is_none());
        assert!(detect(500, html.len(), html).is_none());
    }

    #[test]
    fn ignores_large_pages_even_with_phrase() {
        let mut html = String::from(r#"<html><body><h1>Page not found</h1>"#);
        // Pad to past the size threshold.
        while html.len() <= MAX_SOFT_404_BYTES {
            html.push_str("<p>real content paragraph</p>");
        }
        html.push_str("</body></html>");
        assert!(detect(200, html.len(), &html).is_none());
    }

    #[test]
    fn ignores_pages_with_real_article_content() {
        // Small body with the trigger phrase mentioned but a strong
        // article element with real content.
        let mut article_text = String::from("real content. ");
        // Pad article to past the strong-main threshold (512 chars).
        while article_text.len() < 1200 {
            article_text.push_str("more real content. ");
        }
        let html = format!(
            r#"<html><body>
                <p>Did you encounter a "page not found" recently?</p>
                <article>{article_text}</article>
            </body></html>"#
        );
        assert!(detect(200, html.len(), &html).is_none());
    }

    #[test]
    fn empty_body_is_flagged() {
        assert!(detect(200, 0, "").is_some());
    }
}
