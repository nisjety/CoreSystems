//! Markdown from HTML under the caller's cleaning options (`/v1/scrape`
//! `include_tags`, `exclude_tags`, `only_main_content`, `extractor`,
//! `remove_base64_images`).
//!
//! With default options this is exactly [`crate::readability::html_to_readable_markdown`]:
//! the markdown is byte-identical to what a scrape returned before the options
//! existed (a test runs the lab fixtures through both). Anything else takes the
//! path below, which parses the document once with `scraper`.
//!
//! The rules, in order:
//!
//! 1. **Exclusions** (`exclude_tags`, CSS selectors) detach every matching
//!    element and its subtree.
//! 2. **Inclusions** (`include_tags`): only the matching subtrees are kept, in
//!    document order, each once (an element inside another match is not repeated).
//!    An explicit selection beats the main-content heuristic, so the heuristic
//!    does not run on it; the kept HTML is cleaned like `clean_full`. A list
//!    that matches nothing gives empty markdown and the warning
//!    `include_tags_matched_nothing`: never a silent fall back to the whole page.
//! 3. **Extractor**, when nothing was included:
//!    - `readability` (`only_main_content`, the default): the main-content
//!      heuristic, then html2md;
//!    - `clean_full` (`only_main_content: false`): the whole body with `head`,
//!      `script`, `style`, `noscript`, `template` and `iframe` removed, lazy
//!      images resolved and base64 images stripped;
//!    - `raw`: html2md of the document as it is, with no cleaning.
//!
//! `remove_base64_images` strips inline `data:` images on the other paths too.

use std::collections::HashSet;

use quarry_core::scrape::Extractor;
use scraper::{ElementRef, Html, Selector};

use crate::images::{resolve_lazy_images, strip_base64_images};
use crate::markdown::html_to_markdown;
use crate::readability;

/// What to do to a page before it becomes markdown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanOptions {
    pub only_main_content: bool,
    pub include_tags: Vec<String>,
    pub exclude_tags: Vec<String>,
    pub remove_base64_images: bool,
    pub extractor: Extractor,
}

impl Default for CleanOptions {
    fn default() -> Self {
        Self {
            only_main_content: true,
            include_tags: Vec::new(),
            exclude_tags: Vec::new(),
            remove_base64_images: false,
            extractor: Extractor::Readability,
        }
    }
}

impl CleanOptions {
    /// Nothing asked for beyond the standard main-content extraction.
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// A note about the result, with a stable code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanWarning {
    pub code: &'static str,
    pub message: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanOutput {
    pub markdown: String,
    /// The HTML the markdown was made from (after exclusion, inclusion and
    /// cleaning).
    pub html: String,
    pub warnings: Vec<CleanWarning>,
}

/// The selectors that do not parse, as `(index, reason)`.
pub fn validate_selectors(selectors: &[String]) -> Vec<(usize, String)> {
    selectors
        .iter()
        .enumerate()
        .filter_map(|(index, selector)| {
            Selector::parse(selector)
                .err()
                .map(|error| (index, error.to_string()))
        })
        .collect()
}

fn parse_all(selectors: &[String]) -> Vec<Selector> {
    selectors
        .iter()
        .filter_map(|selector| Selector::parse(selector).ok())
        .collect()
}

/// Elements that never carry reading content.
const NON_CONTENT: &str = "head, script, style, noscript, template, iframe";

fn detach_all(doc: &mut Html, selector: &Selector) {
    let ids: Vec<_> = doc.select(selector).map(|element| element.id()).collect();
    for id in ids {
        if let Some(mut node) = doc.tree.get_mut(id) {
            node.detach();
        }
    }
}

/// Markdown and HTML for `html` under `options`.
pub fn to_markdown(html: &str, options: &CleanOptions) -> CleanOutput {
    if options.is_default() {
        let (content_html, markdown) = match readability::extract(html) {
            Some(readable) => {
                let markdown = html_to_markdown(&readable.content_html);
                (readable.content_html, markdown)
            }
            None => (html.to_owned(), html_to_markdown(html)),
        };
        return CleanOutput {
            markdown,
            html: content_html,
            warnings: Vec::new(),
        };
    }

    let mut warnings = Vec::new();
    let mut doc = Html::parse_document(html);
    for selector in parse_all(&options.exclude_tags) {
        detach_all(&mut doc, &selector);
    }

    let selected = if options.include_tags.is_empty() {
        None
    } else {
        let selectors = parse_all(&options.include_tags);
        let matched: HashSet<_> = selectors
            .iter()
            .flat_map(|selector| doc.select(selector).map(|element| element.id()))
            .collect();
        let mut kept = Vec::new();
        for node in doc.root_element().descendants() {
            if !matched.contains(&node.id()) {
                continue;
            }
            // Skip an element whose ancestor is also a match: it is already inside.
            if node
                .ancestors()
                .any(|ancestor| matched.contains(&ancestor.id()))
            {
                continue;
            }
            if let Some(element) = ElementRef::wrap(node) {
                kept.push(element.html());
            }
        }
        if kept.is_empty() {
            warnings.push(CleanWarning {
                code: "include_tags_matched_nothing",
                message: "none of the include_tags selectors matched anything on the page",
            });
        }
        Some(kept.join("\n"))
    };

    let (mut content_html, full_cleaning) = match selected {
        Some(fragment) => (fragment, true),
        None => match options.extractor {
            Extractor::Readability => {
                let serialized = doc.html();
                let content = readability::extract(&serialized)
                    .map(|readable| readable.content_html)
                    .unwrap_or(serialized);
                (content, false)
            }
            Extractor::CleanFull => (doc.html(), true),
            Extractor::Raw => (doc.html(), false),
        },
    };

    if full_cleaning && !content_html.trim().is_empty() {
        content_html = clean_full(&content_html);
    }
    if options.remove_base64_images && !full_cleaning {
        content_html = strip_base64_images(&content_html);
    }
    let markdown = if content_html.trim().is_empty() {
        String::new()
    } else {
        html_to_markdown(&content_html)
    };
    CleanOutput {
        markdown,
        html: content_html,
        warnings,
    }
}

/// `clean_full`: drop the non-content elements, resolve lazy images and strip
/// inline images.
fn clean_full(html: &str) -> String {
    let mut doc = Html::parse_document(html);
    if let Ok(selector) = Selector::parse(NON_CONTENT) {
        detach_all(&mut doc, &selector);
    }
    strip_base64_images(&resolve_lazy_images(&doc.html()))
}
