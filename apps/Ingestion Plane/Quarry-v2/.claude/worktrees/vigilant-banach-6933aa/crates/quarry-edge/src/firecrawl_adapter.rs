//! Firecrawl-compatible response adapter.
//!
//! Maps Quarry's `NormalizedOutput` envelope to a shape Firecrawl SDKs and
//! existing Firecrawl-using consumers can deserialize. Closes Phase 9 parity
//! item: "Firecrawl-compatible response adapter and SDK ergonomics."
//!
//! Reference Firecrawl shape (v0/v1, paraphrased):
//! ```text
//! {
//!   "success": true,
//!   "data": {
//!     "markdown": "...",
//!     "html": "...",
//!     "rawHtml": "...",
//!     "links": ["..."],
//!     "metadata": {
//!       "title": "...",
//!       "description": "...",
//!       "language": "en",
//!       "sourceURL": "https://example.com",
//!       "statusCode": 200
//!     }
//!   }
//! }
//! ```

use serde::{Deserialize, Serialize};

use quarry_core::output::NormalizedOutput;

#[derive(Debug, Serialize, Deserialize)]
pub struct FirecrawlResponse {
    pub success: bool,
    pub data: FirecrawlData,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FirecrawlData {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub markdown: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub html: Option<String>,
    #[serde(rename = "rawHtml", skip_serializing_if = "Option::is_none")]
    pub raw_html: Option<String>,
    pub links: Vec<String>,
    pub metadata: FirecrawlMetadata,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FirecrawlMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(rename = "sourceURL")]
    pub source_url: String,
    #[serde(rename = "statusCode")]
    pub status_code: u16,
    /// Quarry-specific extension: blake3 fingerprint. Standard Firecrawl
    /// clients ignore unknown fields, so this is safe to add.
    #[serde(rename = "_quarryFingerprint", skip_serializing_if = "Option::is_none")]
    pub quarry_fingerprint: Option<String>,
}

/// Adapt a Quarry `NormalizedOutput` to a Firecrawl-shaped response envelope.
pub fn adapt(output: &NormalizedOutput) -> FirecrawlResponse {
    FirecrawlResponse {
        success: output.status >= 200 && output.status < 400,
        data: FirecrawlData {
            // FormatRef is an artifact handle — we don't inline contents here;
            // SDK callers can fetch the artifact via `/v1/artifacts/:id`.
            // Where Quarry inlines markdown via the meta sidecar, that path is
            // separate. We expose nothing here unless the artifact is small
            // and present, matching Firecrawl's behavior of returning small
            // payloads inline only for the markdown format.
            markdown: None,
            html: None,
            raw_html: None,
            links: output.formats.links.iter().map(|l| l.href.clone()).collect(),
            metadata: FirecrawlMetadata {
                title: output.metadata.title.clone(),
                description: None,
                language: output.metadata.lang.clone(),
                source_url: output.url.requested.clone(),
                status_code: output.status as u16,
                quarry_fingerprint: Some(output.fingerprint.clone()),
            },
        },
    }
}

/// As [`adapt`] but with caller-supplied inline markdown text. Use this when
/// the caller has already fetched the artifact bytes and wants Firecrawl-style
/// inline content rather than just a link reference.
pub fn adapt_with_inline(
    output: &NormalizedOutput,
    inline_markdown: Option<String>,
    inline_html: Option<String>,
) -> FirecrawlResponse {
    let mut resp = adapt(output);
    resp.data.markdown = inline_markdown;
    resp.data.html = inline_html;
    resp
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarry_core::output::{
        ChangeInfo, ChangeStatus, DriverInfo, FormatRef, Link, NormalizedOutput, OutputFormats,
        PageMetadata, UrlTriple,
    };

    fn make_output(status: u16) -> NormalizedOutput {
        NormalizedOutput {
            run_id: quarry_core::ids::Id::new(),
            url: UrlTriple {
                requested: "https://example.com/page".into(),
                final_url: "https://example.com/page".into(),
                canonical: Some("https://example.com/page".into()),
            },
            status,
            fetched_at: chrono::Utc::now(),
            fingerprint: "blake3:deadbeef".into(),
            formats: OutputFormats {
                html: Some(FormatRef {
                    artifact_id: quarry_core::ids::Id::new(),
                    bytes: 100,
                }),
                raw: None,
                markdown: Some(FormatRef {
                    artifact_id: quarry_core::ids::Id::new(),
                    bytes: 50,
                }),
                links: vec![
                    Link {
                        href: "https://example.com/a".into(),
                        text: Some("a".into()),
                        rel: None,
                    },
                    Link {
                        href: "https://example.com/b".into(),
                        text: None,
                        rel: None,
                    },
                ],
                screenshot: None,
                pdf: None,
                extract: None,
            },
            change: ChangeInfo {
                status: ChangeStatus::New,
                prev_fingerprint: None,
            },
            metadata: PageMetadata {
                title: Some("Example".into()),
                lang: Some("en".into()),
                content_type: Some("text/html".into()),
            },
            driver: DriverInfo {
                kind: quarry_core::output::DriverKind::Static,
                duration_ms: 100,
                profile: None,
                version: None,
                session_id: None,
                live_view_url: None,
                recording_id: None,
            },
            determinism: None,
        }
    }

    #[test]
    fn adapt_marks_success_for_2xx() {
        let resp = adapt(&make_output(200));
        assert!(resp.success);
        assert_eq!(resp.data.metadata.status_code, 200);
    }

    #[test]
    fn adapt_marks_failure_for_5xx() {
        let resp = adapt(&make_output(503));
        assert!(!resp.success);
    }

    #[test]
    fn adapt_carries_links_metadata_and_fingerprint() {
        let resp = adapt(&make_output(200));
        assert_eq!(resp.data.links.len(), 2);
        assert_eq!(resp.data.metadata.title.as_deref(), Some("Example"));
        assert_eq!(resp.data.metadata.language.as_deref(), Some("en"));
        assert_eq!(resp.data.metadata.source_url, "https://example.com/page");
        assert_eq!(
            resp.data.metadata.quarry_fingerprint.as_deref(),
            Some("blake3:deadbeef")
        );
    }

    #[test]
    fn adapt_serializes_with_firecrawl_field_names() {
        let resp = adapt(&make_output(200));
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"sourceURL\""));
        assert!(json.contains("\"statusCode\""));
        assert!(!json.contains("\"source_url\""));
    }

    #[test]
    fn adapt_with_inline_attaches_content() {
        let resp = adapt_with_inline(
            &make_output(200),
            Some("# Hello".into()),
            Some("<h1>Hello</h1>".into()),
        );
        assert_eq!(resp.data.markdown.as_deref(), Some("# Hello"));
        assert_eq!(resp.data.html.as_deref(), Some("<h1>Hello</h1>"));
    }
}
