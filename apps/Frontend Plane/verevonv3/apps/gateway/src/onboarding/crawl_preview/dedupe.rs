use std::collections::HashMap;

use serde_json::Value;

/// Per-stream ledger that decides whether a normalized snippet is worth
/// forwarding to the wizard.
///
/// A crawl preview sees the same page more than once: the seed scrape emits
/// `page_fetched` (no text) and then `page_extracted` (title + excerpt) for
/// the homepage, and the live crawl job emits both again for that same URL
/// plus every discovered page. Forwarding all of them produced duplicate,
/// text-less cards. The ledger keys snippets by URL, scores how much a
/// snippet actually shows (excerpt present, specific title), and admits a
/// snippet only when it is the first for its URL or strictly richer than
/// what was already sent — so a later richer snippet UPDATES the card (the
/// frontend replaces by id) instead of duplicating it, and a poorer
/// duplicate is dropped.
#[derive(Debug, Default)]
pub(super) struct SnippetLedger {
    seen: HashMap<String, u8>,
}

/// Outcome of [`SnippetLedger::admit`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Admission {
    /// First snippet for this URL: forward it and count the page.
    First,
    /// Richer than the snippet already forwarded for this URL: forward it,
    /// but do not count the page again.
    Richer,
    /// Same or poorer than what was already forwarded: drop it.
    Skip,
}

impl SnippetLedger {
    pub(super) fn admit(&mut self, snippet: &Value) -> Admission {
        let key = snippet_key(snippet);
        let quality = snippet_quality(snippet);
        match self.seen.get(&key).copied() {
            None => {
                self.seen.insert(key, quality);
                Admission::First
            }
            Some(previous) if quality > previous => {
                self.seen.insert(key, quality);
                Admission::Richer
            }
            Some(_) => Admission::Skip,
        }
    }
}

/// URL (trailing slash and fragment ignored) — falls back to the snippet id.
fn snippet_key(snippet: &Value) -> String {
    let raw = snippet
        .get("url")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .or_else(|| snippet.get("id").and_then(Value::as_str))
        .unwrap_or_default();
    let no_fragment = raw.split('#').next().unwrap_or(raw);
    no_fragment.trim().trim_end_matches('/').to_lowercase()
}

/// 0 = bare fetch (host title, no text) … 3 = specific title + excerpt.
fn snippet_quality(snippet: &Value) -> u8 {
    let has_excerpt = snippet
        .get("excerpt")
        .and_then(Value::as_str)
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);
    let has_specific_title = matches!(
        snippet.get("titleSource").and_then(Value::as_str),
        Some("html") | Some("model")
    );
    (has_excerpt as u8) * 2 + has_specific_title as u8
}

#[cfg(test)]
mod tests {
    use super::{Admission, SnippetLedger};
    use serde_json::json;

    #[test]
    fn first_snippet_for_a_url_is_admitted_and_counted() {
        let mut ledger = SnippetLedger::default();
        let bare =
            json!({ "url": "https://aquatiq.com/", "title": "aquatiq.com", "excerpt": null });
        assert_eq!(ledger.admit(&bare), Admission::First);
    }

    #[test]
    fn richer_snippet_for_same_url_updates_without_recounting() {
        let mut ledger = SnippetLedger::default();
        let bare =
            json!({ "url": "https://aquatiq.com/", "title": "aquatiq.com", "excerpt": null });
        let rich = json!({
            "url": "https://aquatiq.com",
            "title": "Aquatiq – hygiene for matindustrien",
            "titleSource": "model",
            "excerpt": "Vi leverer hygieneløsninger…",
        });
        assert_eq!(ledger.admit(&bare), Admission::First);
        assert_eq!(ledger.admit(&rich), Admission::Richer);
        // The live crawl re-sending the same rich page is a pure duplicate.
        assert_eq!(ledger.admit(&rich), Admission::Skip);
    }

    #[test]
    fn poorer_duplicate_after_a_rich_snippet_is_dropped() {
        let mut ledger = SnippetLedger::default();
        let rich = json!({
            "url": "https://aquatiq.com/om-oss",
            "title": "Om oss",
            "titleSource": "html",
            "excerpt": "Aquatiq ble etablert…",
        });
        let bare = json!({ "url": "https://aquatiq.com/om-oss/", "title": "aquatiq.com" });
        assert_eq!(ledger.admit(&rich), Admission::First);
        assert_eq!(ledger.admit(&bare), Admission::Skip);
    }

    #[test]
    fn different_urls_are_independent() {
        let mut ledger = SnippetLedger::default();
        let a = json!({ "url": "https://aquatiq.com/a", "title": "A", "titleSource": "html", "excerpt": "x" });
        let b = json!({ "url": "https://aquatiq.com/b", "title": "B", "titleSource": "html", "excerpt": "y" });
        assert_eq!(ledger.admit(&a), Admission::First);
        assert_eq!(ledger.admit(&b), Admission::First);
    }

    #[test]
    fn excerpt_outranks_a_specific_title_alone() {
        let mut ledger = SnippetLedger::default();
        let titled = json!({ "url": "https://x.y/", "title": "Real title", "titleSource": "html" });
        let texted = json!({ "url": "https://x.y/", "title": "x.y", "titleSource": "host", "excerpt": "some text" });
        assert_eq!(ledger.admit(&titled), Admission::First);
        assert_eq!(ledger.admit(&texted), Admission::Richer);
    }
}
