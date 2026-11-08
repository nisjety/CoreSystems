//! What cleaning a large page costs (WS5.E). A measurement, not a check: it is
//! ignored by default and prints one `MEASURE` line per run and option, with the
//! median per option. It asserts only that each run produced markdown.
//!
//!   cargo test -p quarry-transform --test clean_html_cost -- --ignored --nocapture
//!
//! `MEASURE_RUNS` sets the number of runs per option (default 3). The `profile=`
//! field on the first line says whether the test binary was built with debug
//! assertions (the `dev`/`test` profile) or not (`--release`). A number is only
//! as good as the machine and the build that printed it.

use std::time::Instant;

use quarry_core::scrape::Extractor;
use quarry_transform::clean_html::{to_markdown, CleanOptions};

const TARGET_BYTES: usize = 5 * 1024 * 1024;

/// A news-style page of about `target` bytes: a navigation bar with 200 links, a
/// share bar, an article of paragraphs with links and a related-stories box every
/// 50 paragraphs, an aside, a footer, and an inline script and style.
fn large_page(target: usize) -> String {
    let mut html = String::with_capacity(target + 8192);
    html.push_str(
        "<!doctype html><html><head><title>Measurement page</title>\
         <script>var tracking = 1;</script><style>p { color: red; }</style></head><body><nav>",
    );
    for i in 0..200 {
        html.push_str(&format!("<a href=\"/section/{i}\">Section {i}</a> "));
    }
    html.push_str(
        "</nav><div class=\"share\">Share on Twitter Share on Facebook</div>\
         <article><h1>Measured headline</h1>",
    );
    let mut i = 0usize;
    while html.len() < target {
        html.push_str(&format!(
            "<p class=\"para\">Paragraph {i} of the measured article, with a \
             <a href=\"https://example.test/story/{i}\">link</a> and enough words to read as \
             prose to the main-content heuristic.</p>"
        ));
        if i % 50 == 49 {
            html.push_str(&format!("<div class=\"related\">Related story {i}</div>"));
        }
        i += 1;
    }
    html.push_str(
        "</article><aside class=\"related\">Related box</aside>\
         <footer>Footer small print</footer></body></html>",
    );
    html
}

struct Case {
    label: &'static str,
    options: CleanOptions,
}

fn cases() -> Vec<Case> {
    vec![
        Case {
            label: "default",
            options: CleanOptions::default(),
        },
        Case {
            label: "exclude_tags",
            options: CleanOptions {
                exclude_tags: vec![".related".into(), "aside".into()],
                ..CleanOptions::default()
            },
        },
        Case {
            label: "include_tags",
            options: CleanOptions {
                include_tags: vec!["article".into()],
                ..CleanOptions::default()
            },
        },
        Case {
            label: "only_main_content_false",
            options: CleanOptions {
                only_main_content: false,
                extractor: Extractor::CleanFull,
                ..CleanOptions::default()
            },
        },
    ]
}

#[test]
#[ignore = "measurement: run with --ignored --nocapture; prints MEASURE lines"]
fn cleaning_a_five_megabyte_page_costs_this_much_per_option() {
    let runs: usize = std::env::var("MEASURE_RUNS")
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|runs| *runs > 0)
        .unwrap_or(3);
    let html = large_page(TARGET_BYTES);
    let profile = if cfg!(debug_assertions) { "debug" } else { "release" };
    eprintln!("MEASURE page bytes={} profile={profile} runs={runs}", html.len());

    for case in cases() {
        let mut millis = Vec::with_capacity(runs);
        let mut markdown_bytes = 0;
        for run in 1..=runs {
            let started = Instant::now();
            let out = to_markdown(&html, &case.options);
            let elapsed = started.elapsed().as_secs_f64() * 1000.0;
            assert!(!out.markdown.is_empty(), "{}: nothing was extracted", case.label);
            markdown_bytes = out.markdown.len();
            eprintln!(
                "MEASURE case={} run={run} ms={elapsed:.1} markdown_bytes={markdown_bytes}",
                case.label
            );
            millis.push(elapsed);
        }
        millis.sort_by(|a, b| a.partial_cmp(b).expect("finite timings"));
        eprintln!(
            "MEASURE case={} median_ms={:.1} min_ms={:.1} markdown_bytes={markdown_bytes}",
            case.label,
            millis[millis.len() / 2],
            millis[0]
        );
    }
}
