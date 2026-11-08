//! Goldens for `clean_html::to_markdown`: what each cleaning option keeps and
//! drops, as `must_keep` and `must_drop` lists on small realistic pages, and the
//! guarantee that default options are byte-identical to the standard extraction.

use quarry_core::scrape::Extractor;
use quarry_transform::clean_html::{to_markdown, validate_selectors, CleanOptions};
use quarry_transform::readability::html_to_readable_markdown;

fn opts() -> CleanOptions {
    CleanOptions::default()
}

fn assert_golden(label: &str, markdown: &str, must_keep: &[&str], must_drop: &[&str]) {
    for needle in must_keep {
        assert!(markdown.contains(needle), "{label}: missing {needle:?} in:\n{markdown}");
    }
    for needle in must_drop {
        assert!(!markdown.contains(needle), "{label}: should not contain {needle:?} in:\n{markdown}");
    }
}

const ARTICLE: &str = r#"<html><head><title>Story</title><script>var tracking = 1;</script></head><body>
<nav><a href="/">Home</a> <a href="/about">About us</a></nav>
<div class="share">Share on Twitter Share on Facebook</div>
<article>
  <h1>The Big Headline</h1>
  <p>First paragraph of the story, with enough words in it to look like real prose to a reader.</p>
  <p>Second paragraph continues the story and adds a few more sentences about the topic at hand.</p>
  <aside class="related">Related stories: ten things you missed</aside>
</article>
<footer>Copyright small print</footer></body></html>"#;

#[test]
fn exclude_tags_remove_the_subtree_and_keep_the_rest() {
    let out = to_markdown(
        ARTICLE,
        &CleanOptions {
            exclude_tags: vec!["aside".into(), ".share".into()],
            ..opts()
        },
    );
    assert_golden(
        "article exclude",
        &out.markdown,
        &["The Big Headline", "First paragraph", "Second paragraph"],
        &["Related stories", "Share on Twitter"],
    );
    assert!(out.warnings.is_empty());
}

#[test]
fn include_tags_keep_only_the_selection_and_skip_the_heuristic() {
    let out = to_markdown(
        ARTICLE,
        &CleanOptions {
            include_tags: vec!["article".into()],
            exclude_tags: vec!["aside".into()],
            ..opts()
        },
    );
    assert_golden(
        "article include",
        &out.markdown,
        &["The Big Headline", "First paragraph"],
        &["Home", "About us", "Related stories", "Copyright", "Share on"],
    );
    assert!(!out.html.contains("<script"), "scripts never survive the cleaning");
}

#[test]
fn only_main_content_false_keeps_navigation_and_footer_but_not_scripts() {
    let out = to_markdown(
        ARTICLE,
        &CleanOptions {
            only_main_content: false,
            extractor: Extractor::CleanFull,
            ..opts()
        },
    );
    assert_golden(
        "clean_full",
        &out.markdown,
        &["The Big Headline", "About us", "Related stories", "Copyright small print"],
        &["var tracking"],
    );
}

#[test]
fn a_selector_that_matches_nothing_is_empty_with_a_warning_never_the_whole_page() {
    let out = to_markdown(
        ARTICLE,
        &CleanOptions {
            include_tags: vec!["#nope".into()],
            ..opts()
        },
    );
    assert_eq!(out.markdown, "");
    assert_eq!(out.html, "");
    assert_eq!(out.warnings[0].code, "include_tags_matched_nothing");
}

const DOCS: &str = r#"<html><body>
<header><a href="/">Product docs</a><nav>Getting started | API | Pricing</nav></header>
<div class="sidebar">Sidebar index of every page</div>
<main><h1>Install</h1><p>Run the installer and follow the prompts to finish setting everything up on your machine.</p><pre><code>cargo install tool</code></pre></main>
</body></html>"#;

#[test]
fn a_docs_page_with_include_main_keeps_the_content_and_drops_the_chrome() {
    let out = to_markdown(DOCS, &CleanOptions { include_tags: vec!["main".into()], ..opts() });
    assert_golden(
        "docs",
        &out.markdown,
        &["Install", "Run the installer", "cargo install tool"],
        &["Sidebar index", "Pricing", "Product docs"],
    );
}

const PRODUCTS: &str = r#"<html><body><nav>Shop menu</nav><section>
<div class="product-card"><h2>Red kettle</h2><span class="badge">SALE</span><p>1 litre, steel</p></div>
<div class="product-card"><h2>Blue teapot</h2><span class="badge">NEW</span><p>0.8 litre, ceramic</p></div>
<div class="product-card"><h2>Green mug</h2><p>300 ml, glass</p></div></section></body></html>"#;

#[test]
fn product_cards_are_kept_each_without_its_badge() {
    let out = to_markdown(
        PRODUCTS,
        &CleanOptions {
            include_tags: vec![".product-card".into()],
            exclude_tags: vec![".badge".into()],
            ..opts()
        },
    );
    assert_golden(
        "products",
        &out.markdown,
        &["Red kettle", "1 litre, steel", "Blue teapot", "Green mug", "300 ml, glass"],
        &["SALE", "NEW", "Shop menu"],
    );
}

const COOKIE: &str = r#"<html><body><div id="cookie-banner">We use cookies. Accept all cookies?</div>
<article><h1>Real content</h1><p>This paragraph is the actual content a reader came for, and it is long enough.</p></article></body></html>"#;

#[test]
fn an_overlay_is_removed_by_selector() {
    let out = to_markdown(COOKIE, &CleanOptions { exclude_tags: vec!["#cookie-banner".into()], ..opts() });
    assert_golden("cookie", &out.markdown, &["Real content", "actual content"], &["cookies"]);
}

fn report_page() -> String {
    let rows: String = (1..=12)
        .map(|i| format!("<tr><td>Region {i}</td><td>{}</td></tr>", i * 1000))
        .collect();
    format!(
        "<html><body><nav>Report menu</nav><h1>Quarterly report</h1><table><tr><th>Region</th><th>Revenue</th></tr>{rows}</table></body></html>"
    )
}

#[test]
fn a_table_heavy_report_keeps_every_row_without_the_heuristic() {
    let page = report_page();
    let out = to_markdown(&page, &CleanOptions { only_main_content: false, extractor: Extractor::CleanFull, ..opts() });
    for i in 1..=12 {
        assert!(out.markdown.contains(&format!("Region {i}")), "row {i} lost:\n{}", out.markdown);
        assert!(out.markdown.contains(&format!("{}", i * 1000)));
    }
    assert!(out.markdown.contains("Quarterly report"));
}

const NESTED: &str = r#"<html><body><article><h1>Nested</h1><div class="body"><p>Body text appears exactly once in the result.</p></div></article></body></html>"#;

#[test]
fn overlapping_includes_do_not_duplicate_content() {
    let out = to_markdown(NESTED, &CleanOptions { include_tags: vec!["article".into(), ".body".into()], ..opts() });
    assert_eq!(out.markdown.matches("Body text appears exactly once").count(), 1, "{}", out.markdown);
    // The same selector listed twice does not repeat either.
    let out = to_markdown(NESTED, &CleanOptions { include_tags: vec![".body".into(), ".body".into()], ..opts() });
    assert_eq!(out.markdown.matches("Body text appears exactly once").count(), 1);
}

const IMAGES: &str = r#"<html><body><article><h1>Pictures</h1><p>Some text before the images so the page has real content in it.</p>
<img src="data:image/gif;base64,R0lGODlhAQABAAAAACw=" data-src="https://cdn.example/lazy.jpg" alt="lazy one">
<img src="data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==" alt="inline one">
<img src="https://cdn.example/plain.jpg" alt="plain one"></article></body></html>"#;

#[test]
fn lazy_images_are_resolved_and_base64_images_stripped_by_clean_full() {
    let out = to_markdown(IMAGES, &CleanOptions { only_main_content: false, extractor: Extractor::CleanFull, ..opts() });
    assert_golden(
        "images clean_full",
        &out.markdown,
        &["https://cdn.example/lazy.jpg", "https://cdn.example/plain.jpg"],
        &["base64", "inline one"],
    );
}

#[test]
fn remove_base64_images_applies_to_the_readability_path_too() {
    let kept = to_markdown(IMAGES, &CleanOptions { exclude_tags: vec![".nothing".into()], ..opts() });
    let removed = to_markdown(
        IMAGES,
        &CleanOptions { exclude_tags: vec![".nothing".into()], remove_base64_images: true, ..opts() },
    );
    assert!(!removed.markdown.contains("base64"), "{}", removed.markdown);
    assert!(kept.markdown.len() >= removed.markdown.len());
}

#[test]
fn raw_converts_the_document_as_it_is() {
    let out = to_markdown(ARTICLE, &CleanOptions { only_main_content: false, extractor: Extractor::Raw, ..opts() });
    assert_golden("raw", &out.markdown, &["About us", "Copyright small print", "The Big Headline"], &[]);
}

#[test]
fn selectors_that_do_not_parse_are_reported_with_their_index() {
    let bad = validate_selectors(&["article".into(), "div[".into(), ".ok".into(), "<<<".into()]);
    assert_eq!(bad.iter().map(|(i, _)| *i).collect::<Vec<_>>(), vec![1, 3]);
    assert!(validate_selectors(&["a".into(), "#id.class > p:nth-child(2)".into()]).is_empty());
}

#[test]
fn default_options_are_byte_identical_to_the_standard_extraction() {
    assert!(opts().is_default());
    for page in [ARTICLE, DOCS, PRODUCTS, COOKIE, NESTED, IMAGES] {
        assert_eq!(to_markdown(page, &opts()).markdown, html_to_readable_markdown(page));
    }
    // And a non-default option that changes nothing it matches is not the same call:
    assert!(!CleanOptions { remove_base64_images: true, ..opts() }.is_default());
}

#[test]
fn every_lab_fixture_is_byte_identical_under_default_options() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../lab/evals/fixtures");
    let mut checked = 0;
    for entry in std::fs::read_dir(&dir).expect("lab fixtures directory") {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|ext| ext != "html") {
            continue;
        }
        let html = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            to_markdown(&html, &opts()).markdown,
            html_to_readable_markdown(&html),
            "{} changed under default options",
            path.display()
        );
        checked += 1;
    }
    assert!(checked >= 10, "expected the ten lab fixtures, found {checked}");
}

#[test]
fn a_large_page_is_parsed_once_for_the_cleaning_and_once_by_the_heuristic() {
    // Not a timing assertion: it pins that a big non-default request completes and
    // keeps its content, so a pathological second parse would show up as a hang.
    let filler: String = (0..20_000)
        .map(|i| format!("<p>Paragraph number {i} of a very long page used to check the cost.</p>"))
        .collect();
    let page = format!("<html><body><nav>menu</nav><article><h1>Long</h1>{filler}</article><aside>side</aside></body></html>");
    let out = to_markdown(&page, &CleanOptions { exclude_tags: vec!["aside".into()], ..opts() });
    assert!(out.markdown.contains("Paragraph number 19999"));
    assert!(!out.markdown.contains("side"));
}
