//! Output-size regression tests for the HTML to markdown step (WS5.E, R7).
//!
//! The converter (html2md 0.2) pads every table row to the widest cell of each
//! column, so one very wide cell made every row as wide: a 1 MB Wikipedia page
//! produced 30.9 MB of markdown with `only_main_content=false`. It also read the
//! rows of a nested table once per nesting level, which is exponential in the
//! depth. The tests below build pathological inputs and assert that the markdown
//! stays within a small multiple of the input (tables, `pre` blocks), and that
//! ordinary tables, lists and `pre` blocks keep their content.
//!
//! Known limit, not fixed: nested lists. html2md indents every line of a nested
//! item by the list depth, so the markdown grows with the SQUARE of the depth (the
//! ratio to the input grows linearly with it). A 1 MB page of 20,000 nested lists
//! would be about a gigabyte; real pages are a few levels deep. The last test pins
//! the quadratic shape so that it cannot get worse unnoticed.
//!
//! Measured before the fix (debug build, html2md 0.2.17 table handler):
//!   wide cell, 200 rows       26,162 B html ->     4,061,420 B markdown (x155)
//!   ten nested tables            434 B html ->   770,781,021 B markdown, 185 s
//!   nested lists, depth 200   10,207 B html ->       127,006 B markdown (x12)
//!   pre block                628,914 B html ->       628,898 B markdown (x1.0)

use std::time::Instant;

use quarry_core::scrape::Extractor;
use quarry_transform::clean_html::{to_markdown, CleanOptions};
use quarry_transform::markdown::html_to_markdown;

/// Bytes of markdown allowed per byte of input in the pathological cases.
const MAX_RATIO: usize = 3;

fn convert(name: &str, html: &str) -> String {
    let started = Instant::now();
    let md = html_to_markdown(html);
    eprintln!(
        "MEASURE {name}: html_bytes={} md_bytes={} ratio={:.2} ms={}",
        html.len(),
        md.len(),
        md.len() as f64 / html.len().max(1) as f64,
        started.elapsed().as_millis()
    );
    md
}

fn assert_bounded(name: &str, html: &str, md: &str) {
    assert!(
        md.len() <= html.len() * MAX_RATIO,
        "{name}: {} bytes of html became {} bytes of markdown (limit x{MAX_RATIO})",
        html.len(),
        md.len()
    );
}

#[test]
fn one_wide_cell_does_not_pad_every_row() {
    let wide = "w".repeat(20_000);
    let mut html = format!("<table><tr><th>k</th><th>v</th></tr><tr><td>{wide}</td><td>x</td></tr>");
    for i in 0..200 {
        html.push_str(&format!("<tr><td>{i}</td><td>y</td></tr>"));
    }
    html.push_str("</table>");
    let md = convert("wide_cell_table", &html);
    assert_bounded("wide cell", &html, &md);
    assert!(md.contains(&wide), "the wide cell is kept whole");
    for i in [0, 99, 199] {
        assert!(md.contains(&format!("| {i} | y |")), "row {i} lost");
    }
    let longest = md.lines().map(str::len).max().unwrap_or(0);
    assert!(longest < 20_100, "no line is padded beyond the wide cell: {longest}");
}

#[test]
fn a_short_row_is_not_filled_up_to_the_widest_row() {
    // 5,000 empty header cells and 5,000 one-cell rows: filling every row up to
    // the widest would write 25 million cells.
    let mut html = String::from("<table><tr>");
    html.push_str(&"<th></th>".repeat(5_000));
    html.push_str("</tr>");
    for i in 0..5_000 {
        html.push_str(&format!("<tr><td>{i}</td></tr>"));
    }
    html.push_str("</table>");
    let md = convert("wide_header_short_rows", &html);
    assert_bounded("wide header", &html, &md);
    assert!(md.contains("| 4999 |"));
}

#[test]
fn ten_nested_tables_stay_small_and_fast() {
    let depth = 10;
    let mut html = String::new();
    for _ in 0..depth {
        html.push_str("<table><tr><td>a</td><td>");
    }
    html.push_str("core");
    for _ in 0..depth {
        html.push_str("</td></tr></table>");
    }
    let started = Instant::now();
    let md = convert("nested_tables", &html);
    assert!(started.elapsed().as_secs() < 5, "nested tables took {:?}", started.elapsed());
    assert_bounded("nested tables", &html, &md);
    assert_eq!(md.matches("core").count(), 1, "the innermost content appears once:\n{md}");
}

#[test]
fn an_ordinary_table_renders_as_a_pipe_table() {
    let html = "<table><thead><tr><th>Region</th><th align=\"right\">Revenue</th></tr></thead>\
                <tbody><tr><td>North</td><td>1000</td></tr><tr><td>South</td><td>2000</td></tr></tbody></table>";
    let md = convert("ordinary_table", html);
    let lines: Vec<&str> = md.lines().collect();
    assert_eq!(
        lines,
        vec![
            "| Region | Revenue |",
            "| --- | ---: |",
            "| North | 1000 |",
            "| South | 2000 |",
        ],
        "{md}"
    );
}

#[test]
fn table_cells_keep_inline_markup_and_line_breaks() {
    let html = "<p>before</p><table><tr><th>Name</th><th>Link</th></tr>\
                <tr><td><strong>bold</strong> text</td><td><a href=\"https://example.test/x\">go</a><br>second</td></tr></table><p>after</p>";
    let md = convert("cell_markup", html);
    assert!(md.contains("before"), "{md}");
    assert!(md.contains("**bold** text"), "{md}");
    // html2md writes a hard break (two spaces) before the line break of a cell.
    assert!(md.contains("[go](https://example.test/x)"), "{md}");
    assert!(md.contains("<br/>second"), "{md}");
    assert_eq!(md.lines().filter(|line| line.starts_with('|')).count(), 3, "header, delimiter and one row, each on one line: {md}");
    assert!(md.contains("after"), "{md}");
    assert!(md.contains("| --- | --- |"), "{md}");
}

#[test]
fn a_table_inside_a_table_cell_is_kept_once() {
    let html = "<table><tr><th>outer</th></tr><tr><td><table><tr><th>inner</th></tr><tr><td>cell</td></tr></table></td></tr></table>";
    let md = convert("table_in_cell", html);
    assert_eq!(md.matches("inner").count(), 1, "{md}");
    assert_eq!(md.matches("cell").count(), 1, "{md}");
    assert!(md.contains("outer"), "{md}");
}

#[test]
fn an_empty_table_writes_nothing() {
    let md = convert("empty_table", "<p>x</p><table></table><table><tr></tr></table><p>y</p>");
    assert!(!md.contains('|'), "{md}");
    assert!(md.contains('x') && md.contains('y'));
}

#[test]
fn a_wide_table_page_stays_small_through_clean_html() {
    // The WS5.E path of the evidence run: only_main_content=false, clean_full.
    let wide = "w".repeat(50_000);
    let mut html = format!(
        "<html><body><h1>Page</h1><table><tr><th>k</th><th>v</th></tr><tr><td>{wide}</td><td>x</td></tr>"
    );
    for i in 0..500 {
        html.push_str(&format!("<tr><td>{i}</td><td>y</td></tr>"));
    }
    html.push_str("</table></body></html>");
    let options = CleanOptions {
        only_main_content: false,
        extractor: Extractor::CleanFull,
        ..CleanOptions::default()
    };
    let out = to_markdown(&html, &options);
    assert_bounded("clean_full", &html, &out.markdown);
    assert!(out.markdown.contains("Page"));
    assert!(out.markdown.contains("| 499 | y |"));
}

#[test]
fn a_pre_block_is_copied_without_growth() {
    let mut html = String::from("<pre><code>");
    for i in 0..20_000 {
        html.push_str(&format!("line {i} of preformatted text\n"));
    }
    html.push_str("</code></pre>");
    let md = convert("pre_block", &html);
    assert!(md.len() <= html.len(), "pre content grew");
    assert!(md.contains("line 19999 of preformatted text"));
}

#[test]
fn nested_lists_grow_quadratically_with_depth_and_no_faster() {
    // Measured: depth 100 -> 33,506 B, 200 -> 127,006 B, 400 -> 494,006 B (three lines
    // per item), about 3.1 x depth^2. The bound allows 4 x depth^2 plus the item text.
    for depth in [100usize, 200, 400] {
        let mut html = String::new();
        for _ in 0..depth {
            html.push_str("<ul><li>item<br>second line<br>third line");
        }
        html.push_str("deepest");
        for _ in 0..depth {
            html.push_str("</li></ul>");
        }
        let md = convert("nested_lists", &html);
        assert!(md.len() <= 4 * depth * depth + 100 * depth, "depth {depth}: {} -> {}", html.len(), md.len());
        assert!(md.contains("deepest"));
    }
}
