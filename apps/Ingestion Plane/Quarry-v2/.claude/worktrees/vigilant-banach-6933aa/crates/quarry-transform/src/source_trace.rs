//! Field-level SourceTrace builder.
//!
//! Given the raw HTML and the canonical source URL, construct a `SourceTrace`
//! with per-field selectors so downstream consumers (Data Plane, audit logs)
//! can prove WHERE each field came from. Closes the field-level half of QRY-08.
//!
//! Fields populated when present in the HTML:
//! - `title` ← `<title>`
//! - `description` ← `<meta name="description">`
//! - `lang` ← `<html lang="...">`
//! - `canonical_url` ← `<link rel="canonical">`
//! - `og_title` ← `<meta property="og:title">`
//! - `og_description` ← `<meta property="og:description">`
//! - `og_image` ← `<meta property="og:image">`
//! - `author` ← `<meta name="author">`
//! - `published_time` ← `<meta property="article:published_time">`
//! - `body` ← chosen content root (`<article>`, `<main>`, `[role=main]`)

use chrono::{DateTime, Utc};
use scraper::{Html, Selector};

use quarry_core::contracts::{FieldTrace, SourceTrace};

pub struct SourceTraceBuilder<'a> {
    html: &'a str,
    source_url: &'a str,
    fingerprint: &'a str,
    fetched_at: DateTime<Utc>,
}

impl<'a> SourceTraceBuilder<'a> {
    pub fn new(
        html: &'a str,
        source_url: &'a str,
        fingerprint: &'a str,
        fetched_at: DateTime<Utc>,
    ) -> Self {
        Self {
            html,
            source_url,
            fingerprint,
            fetched_at,
        }
    }

    pub fn build(&self) -> SourceTrace {
        SourceTrace {
            source_url: self.source_url.to_string(),
            fetched_at: self.fetched_at,
            fingerprint: self.fingerprint.to_string(),
            field_traces: self.field_traces(),
        }
    }

    fn field_traces(&self) -> Vec<FieldTrace> {
        let doc = Html::parse_document(self.html);
        let mut out = Vec::new();

        let probe = |sel: &str, field: &str, present: bool| -> Option<FieldTrace> {
            present.then(|| FieldTrace {
                field: field.to_string(),
                source_url: self.source_url.to_string(),
                selector: Some(sel.to_string()),
            })
        };

        // (selector, field name)
        let cases: &[(&str, &str)] = &[
            ("title", "title"),
            ("meta[name=description]", "description"),
            ("html[lang]", "lang"),
            ("link[rel=canonical]", "canonical_url"),
            ("meta[property=\"og:title\"]", "og_title"),
            ("meta[property=\"og:description\"]", "og_description"),
            ("meta[property=\"og:image\"]", "og_image"),
            ("meta[name=author]", "author"),
            (
                "meta[property=\"article:published_time\"]",
                "published_time",
            ),
        ];
        for (sel_str, field) in cases {
            if let Ok(sel) = Selector::parse(sel_str) {
                if let Some(trace) = probe(sel_str, field, doc.select(&sel).next().is_some()) {
                    out.push(trace);
                }
            }
        }

        // Body: pick the first content root that exists.
        let body_candidates: &[&str] = &["article", "main", "[role=main]"];
        for sel_str in body_candidates {
            if let Ok(sel) = Selector::parse(sel_str) {
                if doc.select(&sel).next().is_some() {
                    out.push(FieldTrace {
                        field: "body".into(),
                        source_url: self.source_url.to_string(),
                        selector: Some((*sel_str).to_string()),
                    });
                    break;
                }
            }
        }

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-05-08T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn builder_emits_title_and_lang_for_basic_html() {
        let html = r#"<!DOCTYPE html><html lang="en"><head><title>T</title></head><body><article><p>x</p></article></body></html>"#;
        let trace =
            SourceTraceBuilder::new(html, "https://example.com/p", "fp", now()).build();
        assert_eq!(trace.source_url, "https://example.com/p");
        assert_eq!(trace.fingerprint, "fp");

        let fields: Vec<&str> = trace.field_traces.iter().map(|t| t.field.as_str()).collect();
        assert!(fields.contains(&"title"));
        assert!(fields.contains(&"lang"));
        assert!(fields.contains(&"body"));
    }

    #[test]
    fn builder_emits_og_metadata_when_present() {
        let html = r#"<html><head>
            <meta property="og:title" content="OG Title">
            <meta property="og:description" content="OG desc">
            <meta property="og:image" content="https://example.com/img.png">
            <meta name="description" content="d">
            <meta name="author" content="Alice">
            <meta property="article:published_time" content="2026-05-08T00:00:00Z">
            <link rel="canonical" href="https://canonical.example/">
        </head><body></body></html>"#;
        let trace = SourceTraceBuilder::new(html, "https://x.com", "fp", now()).build();
        let fields: Vec<&str> = trace.field_traces.iter().map(|t| t.field.as_str()).collect();
        for expected in [
            "og_title",
            "og_description",
            "og_image",
            "description",
            "author",
            "published_time",
            "canonical_url",
        ] {
            assert!(fields.contains(&expected), "missing {expected}");
        }
    }

    #[test]
    fn builder_picks_main_when_no_article() {
        let html = r#"<html><head><title>t</title></head><body><main><p>x</p></main></body></html>"#;
        let trace = SourceTraceBuilder::new(html, "u", "fp", now()).build();
        let body = trace
            .field_traces
            .iter()
            .find(|t| t.field == "body")
            .unwrap();
        assert_eq!(body.selector.as_deref(), Some("main"));
    }

    #[test]
    fn builder_omits_body_when_no_known_root_exists() {
        let html = r#"<html><head><title>t</title></head><body><div>raw</div></body></html>"#;
        let trace = SourceTraceBuilder::new(html, "u", "fp", now()).build();
        assert!(!trace.field_traces.iter().any(|t| t.field == "body"));
    }

    #[test]
    fn builder_emits_no_traces_for_empty_html() {
        let trace = SourceTraceBuilder::new("", "u", "fp", now()).build();
        assert!(trace.field_traces.is_empty());
    }
}
