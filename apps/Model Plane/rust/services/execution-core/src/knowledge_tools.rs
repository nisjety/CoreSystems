//! Real `knowledge_search` tool for the agentic loop — RAG over the org's own
//! ingested knowledge via Data Plane v2 `RetrievalService.Retrieve`.
//!
//! This replaces the deterministic echo fallback for `knowledge_search`. The
//! `org_id` is supplied by the run context (execution-core's verified
//! `ExecuteStepRequest.org_id`), NOT by the model's tool input, so a tool call
//! can't read another tenant's knowledge. Auth mirrors `model-gateway`'s
//! retrieval `authorize()`: a `x-api-key` metadata header from
//! `DATAPLANE_INTERNAL_KEY` when configured.
//!
//! Transport — gRPC against `dataplane.retrieval.v2.RetrievalService`. The
//! generated client + messages live in `mp-contracts`.

// `# Errors` prose for `Result<String, String>` is noise; `doc_markdown`
// over-flags wire tokens. Low-signal pedantic lints — scoped-allowed.
#![allow(clippy::missing_errors_doc, clippy::doc_markdown)]

use mp_contracts::dataplane::retrieval_v2::{
    retrieval_service_client::RetrievalServiceClient, RetrieveRequest, RetrieveResponse,
};
use tonic::metadata::MetadataValue;
use tonic::transport::Channel;

const MAX_TOP_K: i32 = 20;
const MAX_CHUNK_CHARS: usize = 600;

/// gRPC client for Data Plane v2 retrieval. Cheap to clone.
#[derive(Clone)]
pub struct KnowledgeClient {
    client: RetrievalServiceClient<Channel>,
    api_key: Option<String>,
}

impl KnowledgeClient {
    /// Build from the environment. Returns `None` when
    /// `DATAPLANE_RETRIEVAL_URL` / `DATAPLANE_RETRIEVAL_ADDR` is unset or
    /// unparseable, so the caller surfaces a clear "not configured" error.
    #[must_use]
    pub fn from_env() -> Option<Self> {
        let url = std::env::var("DATAPLANE_RETRIEVAL_URL")
            .or_else(|_| std::env::var("DATAPLANE_RETRIEVAL_ADDR"))
            .ok()
            .filter(|s| !s.trim().is_empty())?;
        let channel = Channel::from_shared(url).ok()?.connect_lazy();
        let api_key = std::env::var("DATAPLANE_INTERNAL_KEY")
            .ok()
            .filter(|s| !s.is_empty());
        Some(Self {
            client: RetrievalServiceClient::new(channel),
            api_key,
        })
    }

    /// `knowledge_search` — retrieve the top-k knowledge chunks for `query`
    /// within `org_id` (the run's verified tenant). Returns a ranked,
    /// agent-readable list. An empty result set is a successful, informative
    /// response (not an error).
    pub async fn search(
        &self,
        org_id: &str,
        user_id: &str,
        query: &str,
        top_k: i32,
    ) -> Result<String, String> {
        let top_k = top_k.clamp(1, MAX_TOP_K);
        let mut request = tonic::Request::new(RetrieveRequest {
            org_id: org_id.to_owned(),
            query: query.to_owned(),
            top_k,
            // Per-user ownership: the retrieval post-filter grounds AS this user.
            // Empty → org-scoped fallback (never cross-user).
            user_id: if user_id.is_empty() {
                None
            } else {
                Some(user_id.to_owned())
            },
            ..Default::default()
        });
        if let Some(key) = &self.api_key {
            if let Ok(value) = MetadataValue::try_from(key.as_str()) {
                request.metadata_mut().insert("x-api-key", value);
            }
        }
        let response = self
            .client
            .clone()
            .retrieve(request)
            .await
            .map_err(|e| format!("knowledge retrieval failed: {}", e.message()))?
            .into_inner();
        Ok(format_candidates(query, &response))
    }
}

/// Format a `RetrieveResponse` into agent-readable text (pure; testable without
/// a live Data Plane).
fn format_candidates(query: &str, response: &RetrieveResponse) -> String {
    use std::fmt::Write as _;
    if response.candidates.is_empty() {
        let suffix = if response.low_confidence {
            " (low confidence)"
        } else {
            ""
        };
        return format!("No knowledge found for \"{query}\"{suffix}.");
    }
    let mut out = format!(
        "Knowledge results for \"{query}\" ({} chunk(s)):\n",
        response.candidates.len()
    );
    for (i, c) in response.candidates.iter().enumerate() {
        let text = truncate_chars(c.text.trim(), MAX_CHUNK_CHARS);
        let _ = write!(
            out,
            "{}. [score {:.3}] {text}\n   (document {})\n",
            i + 1,
            c.final_score,
            if c.document_id.is_empty() {
                "?"
            } else {
                &c.document_id
            }
        );
    }
    out
}

/// Char-boundary-safe truncation (never panics on multibyte input).
fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_owned();
    }
    let head: String = s.chars().take(max).collect();
    format!("{head}…")
}

#[cfg(test)]
mod tests {
    use super::*;
    use mp_contracts::dataplane::retrieval_v2::Candidate;

    fn candidate(text: &str, doc: &str, score: f32) -> Candidate {
        Candidate {
            text: text.to_owned(),
            document_id: doc.to_owned(),
            final_score: score,
            ..Default::default()
        }
    }

    #[test]
    fn formats_ranked_candidates_with_scores_and_docs() {
        let resp = RetrieveResponse {
            candidates: vec![
                candidate("Velion onboarding flow.", "doc-1", 0.42),
                candidate("Billing trial is 14 days.", "doc-2", 0.31),
            ],
            ..Default::default()
        };
        let out = format_candidates("onboarding", &resp);
        assert!(out.contains("2 chunk(s)"));
        assert!(out.contains("1. [score 0.420] Velion onboarding flow."));
        assert!(out.contains("(document doc-1)"));
        assert!(out.contains("2. [score 0.310] Billing trial is 14 days."));
    }

    #[test]
    fn empty_results_are_informative_not_error() {
        let out = format_candidates("zzz", &RetrieveResponse::default());
        assert!(out.contains("No knowledge found"));
    }

    #[test]
    fn empty_results_note_low_confidence() {
        let resp = RetrieveResponse {
            low_confidence: true,
            ..Default::default()
        };
        assert!(format_candidates("q", &resp).contains("low confidence"));
    }

    #[test]
    fn missing_document_id_renders_placeholder() {
        let resp = RetrieveResponse {
            candidates: vec![candidate("orphan chunk", "", 0.1)],
            ..Default::default()
        };
        assert!(format_candidates("q", &resp).contains("(document ?)"));
    }

    #[test]
    fn long_chunk_is_truncated_char_safe() {
        let resp = RetrieveResponse {
            candidates: vec![candidate(&"é".repeat(MAX_CHUNK_CHARS + 50), "d", 0.5)],
            ..Default::default()
        };
        let out = format_candidates("q", &resp);
        assert!(out.contains('…'));
    }
}
