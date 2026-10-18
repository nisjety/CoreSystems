//! Selector semantics of `include_tags` and `exclude_tags` (WS5.E), pinned on two
//! fixture pages in `tests/fixtures/clean_html/`.
//!
//! What is pinned is the behaviour of `scraper` 0.25 (over the `selectors` crate)
//! as `clean_html::to_markdown` uses it, plus the rules `clean_html` adds on top:
//!
//! * A selector is a CSS selector: descendant (`a b`), child (`a > b`), adjacent
//!   (`a + b`), general sibling (`a ~ b`), attribute (`[data-x]`, `[data-x="2"]`),
//!   `:not()`, `:nth-child()`, `:is()`, `:has()`, and comma groups. These are the
//!   CSS meanings; the tests pin them on real markup, not on a restatement of them.
//! * scraper defines no non-tree-structural pseudo-classes and no pseudo-elements
//!   (`scraper` 0.25, `src/selector.rs`: the `NonTSPseudoClass` and `PseudoElement`
//!   enums are empty; its parser overrides only `parse_is_and_where` and `parse_has`,
//!   both true, and keeps the `selectors` defaults for the rest). So `:hover`,
//!   `:focus` and `::before` are parse errors here. A browser accepts them; this is
//!   a difference from CSS, not a "matches nothing".
//! * An unclosed block at the end of the input is accepted: `li:nth-child(2` is read
//!   as `li:nth-child(2)`, because CSS closes a block at the end of the input (a parse
//!   error that the rules recover from). The test of that name pins it.
//! * Exclusion runs before inclusion. Structural selectors in `include_tags` see the
//!   page as it is after the exclusion (siblings removed by `exclude_tags` no longer
//!   count for `:nth-child`). `scraper`'s `Html::select` walks the whole node arena,
//!   detached nodes included, but the inclusion pass only emits nodes still attached
//!   under the root, so an excluded node is never emitted.
//! * Inclusion keeps the matched elements in document order, whatever order the
//!   selectors were listed in, each once. An include list that matches nothing is
//!   empty markdown with `include_tags_matched_nothing`, never the whole page.
//! * `to_markdown` itself skips an entry that does not parse (`parse_all`). The
//!   handler rejects such an entry with a 400 before `to_markdown` runs, so the
//!   skip is only the library's fallback.
//!
//! Not compared here: Firecrawl's `includeTags` and `excludeTags`. Nothing in this
//! file says what Firecrawl does. The open questions to settle against its
//! documentation or a live run are: whether its exclusion runs before or after its
//! inclusion, what an inclusion that matches nothing returns, and whether its
//! structural selectors see the page before or after exclusion.

use quarry_core::scrape::Extractor;
use quarry_transform::clean_html::{to_markdown, validate_selectors, CleanOptions, CleanOutput};

const CARDS: &str = include_str!("fixtures/clean_html/cards.html");
const NESTED: &str = include_str!("fixtures/clean_html/nested.html");

/// Inclusion only. Inclusion never runs the main-content heuristic, so the
/// options other than `include_tags` stay at their defaults.
fn include(selectors: &[&str]) -> CleanOptions {
    CleanOptions {
        include_tags: selectors.iter().map(|s| (*s).to_owned()).collect(),
        ..CleanOptions::default()
    }
}

/// Exclusion with the whole page kept otherwise (`clean_full`), so the
/// main-content heuristic cannot hide the text the test is about.
fn exclude(selectors: &[&str]) -> CleanOptions {
    CleanOptions {
        exclude_tags: selectors.iter().map(|s| (*s).to_owned()).collect(),
        only_main_content: false,
        extractor: Extractor::CleanFull,
        ..CleanOptions::default()
    }
}

fn run(html: &str, options: CleanOptions) -> CleanOutput {
    to_markdown(html, &options)
}

/// `markdown` holds every `keep` string and none of the `drop` strings.
fn check(label: &str, markdown: &str, keep: &[&str], drop: &[&str]) {
    for needle in keep {
        assert!(markdown.contains(needle), "{label}: missing {needle:?} in:\n{markdown}");
    }
    for needle in drop {
        assert!(!markdown.contains(needle), "{label}: must not contain {needle:?} in:\n{markdown}");
    }
}

fn position(markdown: &str, needle: &str) -> usize {
    markdown
        .find(needle)
        .unwrap_or_else(|| panic!("{needle:?} not in:\n{markdown}"))
}

#[test]
fn descendant_selector_matches_at_any_depth() {
    let out = run(NESTED, include(&["article h2"]));
    check("descendant", &out.markdown, &["Direct heading", "Nested heading"], &["Side heading"]);
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
}

#[test]
fn child_selector_matches_direct_children_only() {
    let out = run(NESTED, include(&["article > h2"]));
    check(
        "child",
        &out.markdown,
        &["Direct heading"],
        &["Nested heading", "Nested paragraph", "Trailing paragraph"],
    );
    // The page has no `#root > h2`: a child selector that matches nothing is the
    // empty-match case, not the whole page.
    let none = run(CARDS, include(&["#root > h2"]));
    assert_eq!(none.markdown, "");
    assert_eq!(none.warnings[0].code, "include_tags_matched_nothing");
}

#[test]
fn adjacent_selector_takes_only_the_next_element_sibling() {
    // The first paragraph after each heading. The whitespace text between the
    // elements does not stop `+` (CSS ignores text nodes).
    let out = run(CARDS, include(&["h2 + p"]));
    check("adjacent", &out.markdown, &["Lead one", "Body two", "Body three"], &["Body one", "Lead two"]);
    assert!(position(&out.markdown, "Lead one") < position(&out.markdown, "Body two"));
    assert!(position(&out.markdown, "Body two") < position(&out.markdown, "Body three"));
}

#[test]
fn general_sibling_selector_takes_every_later_sibling() {
    let out = run(CARDS, include(&["h2 ~ p"]));
    check(
        "general sibling",
        &out.markdown,
        &["Lead one", "Body one", "Body two", "Lead two", "Body three"],
        &["Card one", "Footer text", "first item"],
    );
}

#[test]
fn attribute_selectors_match_on_presence_and_on_value() {
    let present = run(CARDS, include(&["[data-x]"]));
    check(
        "attribute presence",
        &present.markdown,
        &["Card one", "Lead one", "Card two", "Lead two"],
        &["Card three", "Body three"],
    );
    let value = run(CARDS, include(&["section[data-x=\"2\"]"]));
    check("attribute value", &value.markdown, &["Card two", "Body two"], &["Card one", "Card three"]);
}

#[test]
fn not_selector_narrows_the_match_both_ways() {
    let included = run(CARDS, include(&["p:not(.lead)"]));
    check("not include", &included.markdown, &["Body one", "Body two", "Body three"], &["Lead one", "Lead two"]);
    let excluded = run(CARDS, exclude(&["section:not([data-x])"]));
    check("not exclude", &excluded.markdown, &["Card one", "Card two"], &["Card three", "Body three"]);
}

#[test]
fn nth_child_counts_element_siblings_and_not_text_nodes() {
    let second = run(CARDS, include(&["li:nth-child(2)"]));
    check("nth-child 2", &second.markdown, &["second item"], &["first item", "third item"]);

    let odd = run(CARDS, include(&["li:nth-child(odd)", "section:nth-child(3)"]));
    check(
        "nth-child odd and section 3",
        &odd.markdown,
        &["first item", "third item", "Card three", "Body three"],
        &["second item", "Card one", "Card two"],
    );
    // Document order: the section comes before the list in the page.
    assert!(position(&odd.markdown, "Card three") < position(&odd.markdown, "first item"));
}

#[test]
fn comma_groups_and_separate_entries_select_the_same_elements_in_document_order() {
    let grouped = run(CARDS, include(&["h2, li"]));
    let separate = run(CARDS, include(&["li", "h2"]));
    assert_eq!(
        grouped.markdown, separate.markdown,
        "a comma group and two entries select the same elements"
    );
    check(
        "comma",
        &grouped.markdown,
        &["Card one", "Card two", "Card three", "first item", "third item"],
        &["Lead one", "Body one"],
    );
    // The list entries say `li` second, but the headings come first in the page.
    assert!(position(&grouped.markdown, "Card three") < position(&grouped.markdown, "first item"));
}

#[test]
fn is_and_has_are_supported() {
    let is = run(CARDS, include(&[":is(h2, li)"]));
    check("is", &is.markdown, &["Card one", "first item"], &["Lead one"]);
    let has = run(CARDS, include(&["section:has(p.lead)"]));
    check("has", &has.markdown, &["Card one", "Lead one", "Card two", "Lead two"], &["Card three"]);
}

#[test]
fn exclude_removes_the_matched_subtree_and_child_and_descendant_differ() {
    let child = run(NESTED, exclude(&["article > h2"]));
    check(
        "exclude child",
        &child.markdown,
        &["Nested heading", "Nested paragraph", "Trailing paragraph"],
        &["Direct heading"],
    );
    let descendant = run(NESTED, exclude(&["article h2"]));
    check(
        "exclude descendant",
        &descendant.markdown,
        &["Nested paragraph", "Trailing paragraph"],
        &["Direct heading", "Nested heading"],
    );
    let subtree = run(NESTED, exclude(&[".inner"]));
    check(
        "exclude subtree",
        &subtree.markdown,
        &["Direct heading", "Trailing paragraph"],
        &["Nested heading", "Nested paragraph"],
    );
}

#[test]
fn exclude_runs_before_include() {
    // The lead paragraphs sit in sections that are excluded first, so they are gone
    // before the inclusion looks for them.
    let gone = run(
        CARDS,
        CleanOptions {
            include_tags: vec![".lead".into()],
            exclude_tags: vec!["section".into()],
            ..CleanOptions::default()
        },
    );
    assert_eq!(gone.markdown, "");
    assert_eq!(gone.warnings[0].code, "include_tags_matched_nothing");

    // The other way round: include the sections, then take their leads out.
    let kept = run(
        CARDS,
        CleanOptions {
            include_tags: vec!["section".into()],
            exclude_tags: vec![".lead".into()],
            ..CleanOptions::default()
        },
    );
    check("include then exclude", &kept.markdown, &["Card one", "Body one", "Card three"], &["Lead one", "Lead two"]);
}

#[test]
fn structural_include_selectors_see_the_page_after_exclusion() {
    // Excluding the first item makes the second item the first element child, so
    // `li:nth-child(1)` now selects it.
    let out = run(
        CARDS,
        CleanOptions {
            include_tags: vec!["li:nth-child(1)".into()],
            exclude_tags: vec!["li:nth-child(1)".into()],
            ..CleanOptions::default()
        },
    );
    check("after exclusion", &out.markdown, &["second item"], &["first item", "third item"]);
}

#[test]
fn invalid_selectors_are_reported_by_index_and_pseudo_classes_scraper_does_not_model_are_invalid() {
    let entries = [
        "section > h2",
        "div[",
        "<<<",
        "a:hover",
        "p::before",
        "li:nth-child(two)",
        "",
        "[data-x]",
    ];
    let owned: Vec<String> = entries.iter().map(|entry| (*entry).to_owned()).collect();
    let bad: Vec<usize> = validate_selectors(&owned)
        .into_iter()
        .map(|(index, _)| index)
        .collect();
    assert_eq!(bad, vec![1, 2, 3, 4, 5, 6]);
    assert!(validate_selectors(&[
        "a".into(),
        "#id.class > p:nth-child(2)".into(),
        ":is(h2, li)".into(),
        "section:has(p.lead)".into(),
    ])
    .is_empty());
}

#[test]
fn an_unclosed_block_at_the_end_of_the_input_is_closed_and_accepted() {
    // CSS Syntax Level 3 closes a block that is still open at the end of the input,
    // as a parse error the rules recover from. The selectors crate keeps the block,
    // so `li:nth-child(2` is read as `li:nth-child(2)`. Pinned so the difference
    // from a strict reading of the same text stays visible.
    assert!(validate_selectors(&["li:nth-child(2".into()]).is_empty());
    let out = run(CARDS, include(&["li:nth-child(2"]));
    check("unclosed block", &out.markdown, &["second item"], &["first item", "third item"]);
}

#[test]
fn excluding_the_html_element_leaves_nothing_to_include_and_does_not_panic() {
    // The exclusion detaches the html element, so the document has no element child.
    // The include pass must answer "matched nothing" (empty markdown and the warning),
    // not panic in scraper's root_element().
    let out = run(
        CARDS,
        CleanOptions {
            include_tags: vec!["h2".into()],
            exclude_tags: vec!["html".into()],
            ..CleanOptions::default()
        },
    );
    assert_eq!(out.markdown, "");
    assert_eq!(out.warnings[0].code, "include_tags_matched_nothing");

    // Without an include list the same exclusion leaves no content.
    let whole = run(CARDS, exclude(&["html"]));
    assert!(whole.markdown.trim().is_empty(), "{:?}", whole.markdown);
}

#[test]
fn an_entry_that_does_not_parse_is_skipped_by_the_library_fallback() {
    // The handler answers 400 before this; the library itself just ignores the
    // broken entry, so the result equals the result without it.
    let with_broken = run(CARDS, include(&["div[", "h2"]));
    let plain = run(CARDS, include(&["h2"]));
    assert_eq!(with_broken.markdown, plain.markdown);
    let with_broken = run(CARDS, exclude(&["div[", ".lead"]));
    let plain = run(CARDS, exclude(&[".lead"]));
    assert_eq!(with_broken.markdown, plain.markdown);
}
