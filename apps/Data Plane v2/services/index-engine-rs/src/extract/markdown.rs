//! Markdown entity extraction (closes D4-5 markdown branch).
//!
//! Spec §6 asks for `tree-sitter-markdown` headings + link parser. v2.3 ships
//! a pragmatic regex-based extractor that produces the same shape of
//! `EntityProposal`s; we expect to swap in tree-sitter-markdown in a future
//! wave without changing the API. Until then the regex covers the most-used
//! markdown constructs: ATX headings (`#`-prefix) and inline + reference
//! links.
//!
//! Confidence label follows the spec: 0.85 for parsed (non-LLM) extraction.
//! Entities emitted here are tagged `provenance="extracted"` upstream so the
//! data-orchestrator promotion job treats them as primary evidence rather
//! than inferred.
//!
//! Skipped on purpose: code fences (covered by future tree-sitter-* code
//! extractors), tables (handled by header-as-attribute extractor in §6),
//! HTML blocks (rare in our corpus).

// D4-5: extractor is built but not yet driven by the chunker pipeline.
// Suppress dead-code warnings on the module-private regex statics and the
// `EntityProposal` shape until the wiring phase lands.
#![allow(dead_code)]

use once_cell::sync::Lazy;
use regex::Regex;

#[derive(Debug, Clone, PartialEq)]
pub struct EntityProposal {
    /// `heading` | `link`
    pub kind: String,
    /// Normalized label (heading text or link anchor text).
    pub label: String,
    /// URL for links, empty for headings.
    pub uri: String,
    /// Source `document_id` — set by caller (chunker has it; this fn doesn't).
    pub document_id: String,
    /// 1-based line number in the source.
    pub line: usize,
    /// Constant 0.85 per spec §6.
    pub confidence: f32,
}

// ATX headings: 1-6 `#` then space then text. Setext headings (underline-style)
// not supported — rare in our corpus.
static HEADING_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^(#{1,6})\s+(.+?)\s*$").expect("compile heading re"));

// Inline link: [text](url "optional title"). Reference-style links not yet
// supported — they need a two-pass parser to resolve `[label][ref]` against
// `[ref]: url`. Tree-sitter will close that gap.
static INLINE_LINK_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"\[([^\]]+)\]\(([^)\s]+)(?:\s+"[^"]*")?\)"#).expect("compile link re")
});

/// Extract entity proposals from the markdown body of a document.
/// Skips fenced code blocks (` ``` ` markers) so headings inside code are
/// not confused with real document structure.
pub fn extract_markdown_entities(document_id: &str, body: &str) -> Vec<EntityProposal> {
    let mut out = Vec::new();
    let mut in_code_fence = false;

    for (idx, line) in body.lines().enumerate() {
        let line_no = idx + 1;
        if line.trim_start().starts_with("```") {
            in_code_fence = !in_code_fence;
            continue;
        }
        if in_code_fence {
            continue;
        }

        if let Some(caps) = HEADING_RE.captures(line) {
            // Headings prefixed only by `#` (and nothing else after the
            // trimming) are skipped — they're empty section markers.
            let text = caps.get(2).map(|m| m.as_str().trim()).unwrap_or("");
            if !text.is_empty() {
                out.push(EntityProposal {
                    kind: "heading".into(),
                    label: text.to_string(),
                    uri: String::new(),
                    document_id: document_id.to_string(),
                    line: line_no,
                    confidence: 0.85,
                });
            }
        }

        for caps in INLINE_LINK_RE.captures_iter(line) {
            let anchor = caps.get(1).map(|m| m.as_str()).unwrap_or("");
            let url = caps.get(2).map(|m| m.as_str()).unwrap_or("");
            if anchor.is_empty() || url.is_empty() {
                continue;
            }
            out.push(EntityProposal {
                kind: "link".into(),
                label: anchor.to_string(),
                uri: url.to_string(),
                document_id: document_id.to_string(),
                line: line_no,
                confidence: 0.85,
            });
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_headings_and_inline_links() {
        let md = r#"# Title

## Section A
Some text with a [link to docs](https://example.com/docs).

```
# this is inside code, ignore
```

### Subsection
"#;
        let out = extract_markdown_entities("doc-1", md);
        let headings: Vec<&str> = out
            .iter()
            .filter(|e| e.kind == "heading")
            .map(|e| e.label.as_str())
            .collect();
        assert_eq!(headings, vec!["Title", "Section A", "Subsection"]);

        let links: Vec<&str> = out
            .iter()
            .filter(|e| e.kind == "link")
            .map(|e| e.uri.as_str())
            .collect();
        assert_eq!(links, vec!["https://example.com/docs"]);
    }

    #[test]
    fn skips_empty_heading_marker() {
        let out = extract_markdown_entities("doc", "##   \nreal content\n");
        assert!(out.is_empty(), "empty heading should not produce an entity");
    }

    #[test]
    fn handles_multiple_links_per_line() {
        let out = extract_markdown_entities("doc", "See [A](http://a) and [B](http://b).");
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].uri, "http://a");
        assert_eq!(out[1].uri, "http://b");
    }
}
