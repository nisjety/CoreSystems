//! Real `knowledge_search` tool for the agentic loop — RAG over the org's own
//! ingested knowledge via Data Plane v2 `RetrievalService.Retrieve`.
//!
//! This replaces the deterministic echo fallback for `knowledge_search`. The
//! `org_id` is supplied by the run context (execution-core's verified
//! `ExecuteStepRequest.org_id`), NOT by the model's tool input, so a tool call
//! can't read another tenant's knowledge. Auth mirrors `model-gateway`'s
//! retrieval authorization: the originating user bearer is forwarded after
//! model-gateway has verified it, and Data Plane verifies it again. Caller-
//! supplied identity headers are never used.
//!
//! Transport — gRPC against `dataplane.retrieval.v2.RetrievalService`. The
//! generated client + messages live in `mp-contracts`.

// `# Errors` prose for `Result<String, String>` is noise; `doc_markdown`
// over-flags wire tokens. Low-signal pedantic lints — scoped-allowed.
#![allow(clippy::missing_errors_doc, clippy::doc_markdown)]

use mp_contracts::dataplane::retrieval_v2::{
    retrieval_service_client::RetrievalServiceClient, RetrieveRequest, RetrieveResponse,
};
use serde::Serialize;
use tonic::metadata::MetadataValue;
use tonic::transport::Channel;

const MAX_TOP_K: i32 = 20;
const MAX_CHUNK_CHARS: usize = 600;

/// gRPC client for Data Plane v2 retrieval. Cheap to clone.
#[derive(Clone)]
pub struct KnowledgeClient {
    client: RetrievalServiceClient<Channel>,
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
        Some(Self {
            client: RetrievalServiceClient::new(channel),
        })
    }

    /// `knowledge_search` — retrieve the top-k knowledge chunks for `query`
    /// within `org_id` (the run's verified tenant). Returns a ranked,
    /// machine-readable result envelope. An empty result set is a successful,
    /// explicit `no_results` response (not a silent success or an error).
    pub async fn search(
        &self,
        org_id: &str,
        user_id: &str,
        query: &str,
        top_k: i32,
        zdr: bool,
        bearer: &str,
    ) -> Result<String, String> {
        let top_k = top_k.clamp(1, MAX_TOP_K);
        let request = build_request(org_id, user_id, query, top_k, zdr, bearer)?;
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

fn build_request(
    org_id: &str,
    user_id: &str,
    query: &str,
    top_k: i32,
    zdr: bool,
    bearer: &str,
) -> Result<tonic::Request<RetrieveRequest>, String> {
    if bearer.trim().is_empty() || bearer != bearer.trim() {
        return Err("knowledge retrieval requires a verified user credential".to_owned());
    }
    let authorization = MetadataValue::try_from(format!("Bearer {bearer}"))
        .map_err(|_| "knowledge retrieval credential is malformed".to_owned())?;
    let mut request = tonic::Request::new(RetrieveRequest {
        org_id: org_id.to_owned(),
        query: query.to_owned(),
        top_k,
        user_id: if user_id.is_empty() {
            None
        } else {
            Some(user_id.to_owned())
        },
        zdr_mode: zdr.then(|| "ephemeral".to_owned()),
        // Jurisdiction axis, and it must be populated: Data Plane v2 reads an
        // absent `sovereign_required` as `true` (fail-closed, since absence of
        // proof is not proof that egress is permitted), which Azure-hosted
        // Cohere Embed v4 can never satisfy — so `..Default::default()`'s `None`
        // made every governed-loop `knowledge_search` fail before retrieval ran.
        //
        // The value is the declared no-signal default, and this crate has no
        // signal to do better with TODAY: `ExecuteStepRequest` carries `zdr`
        // (field 9) but no privacy floor, so the run's `min_privacy_tier` —
        // which `RunAgentRequest` does carry, and which agent.rs already threads
        // onto every `InferRequest` — never reaches this executor. Closing that
        // means a `min_privacy_tier` field on `ExecuteStepRequest` plus
        // threading it through `runtime_loop::execute_step*`; until then a
        // sovereign-pinned agent run gets sovereign MODEL serving and
        // non-sovereign retrieval EMBEDDING, and this comment is the only place
        // that says so.
        //
        // A signed `sovereign = true` on the forwarded user bearer still wins:
        // retrieval-engine-rs re-applies its own floor on receipt, so this can
        // fail to raise the posture but never relax one.
        sovereign_required: Some(
            mp_contracts::dataplane_posture::SOVEREIGN_REQUIRED_WITHOUT_SIGNAL,
        ),
        ..Default::default()
    });
    request
        .metadata_mut()
        .insert("authorization", authorization);
    Ok(request)
}

#[derive(Serialize)]
struct KnowledgeSearchEnvelope {
    kind: &'static str,
    selected_route: &'static str,
    route_control: &'static str,
    status: &'static str,
    low_confidence: bool,
    degraded: bool,
    no_results: bool,
    reason: &'static str,
    query: String,
    trace_id: String,
    index_version: String,
    result_count: usize,
    results: Vec<KnowledgeSearchCandidate>,
    /// Compatibility bridge for model prompts/tests that consumed the former
    /// plain-text result. New callers should use the typed fields above.
    summary: String,
}

#[derive(Serialize)]
struct KnowledgeSearchCandidate {
    rank: usize,
    score: f32,
    text: String,
    document_id: String,
}

/// Format a `RetrieveResponse` into a machine-readable JSON envelope (pure;
/// testable without a live Data Plane). `selected_route=hybrid` means the
/// existing Data Plane `Retrieve` contract; dense/sparse weights remain
/// server-managed because the current gRPC request has no caller route field.
fn format_candidates(query: &str, response: &RetrieveResponse) -> String {
    use std::fmt::Write as _;
    let no_results = response.candidates.is_empty();
    let status = if no_results {
        "no_results"
    } else if response.low_confidence {
        "low_confidence"
    } else {
        "ok"
    };
    let reason = if no_results {
        "No results returned; reformulate the query with different terms before retrying."
    } else if response.low_confidence {
        "Results are low confidence; reformulate or broaden the query before relying on them."
    } else {
        ""
    };
    let summary = if no_results {
        let suffix = if response.low_confidence {
            " (low confidence)"
        } else {
            ""
        };
        format!("No knowledge found for \"{query}\"{suffix}.")
    } else {
        let mut summary = format!(
            "Knowledge results for \"{query}\" ({} chunk(s)):\n",
            response.candidates.len()
        );
        for (i, c) in response.candidates.iter().enumerate() {
            let text = truncate_chars(c.text.trim(), MAX_CHUNK_CHARS);
            let _ = write!(
                summary,
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
        summary
    };
    let results = response
        .candidates
        .iter()
        .enumerate()
        .map(|(index, candidate)| KnowledgeSearchCandidate {
            rank: index + 1,
            score: if candidate.final_score.is_finite() {
                candidate.final_score
            } else {
                0.0
            },
            text: truncate_chars(candidate.text.trim(), MAX_CHUNK_CHARS),
            document_id: if candidate.document_id.is_empty() {
                "?".to_owned()
            } else {
                candidate.document_id.clone()
            },
        })
        .collect();
    serde_json::to_string(&KnowledgeSearchEnvelope {
        kind: "knowledge_search_result",
        selected_route: "hybrid",
        route_control: "server_managed",
        status,
        low_confidence: response.low_confidence,
        degraded: false,
        no_results,
        reason,
        query: query.to_owned(),
        trace_id: response.trace_id.clone(),
        index_version: response.index_version.clone(),
        result_count: response.candidates.len(),
        results,
        summary,
    })
    .expect("knowledge search envelope contains only finite serializable values")
}

/// Typed degraded-state error for dependency/configuration failures. It is
/// carried in the tool error channel, so callers cannot mistake it for a valid
/// empty corpus result while the next inference round can still parse it.
pub(crate) fn format_degraded_error(reason: &str) -> String {
    serde_json::json!({
        "kind": "knowledge_search_result",
        "selected_route": "hybrid",
        "route_control": "server_managed",
        "status": "degraded",
        "low_confidence": false,
        "degraded": true,
        "no_results": false,
        "reason": reason,
        // Do not echo the model-authored query into an error that may be kept
        // for operational diagnosis; this remains content-free under ZDR.
        "query": "",
        "trace_id": "",
        "index_version": "",
        "result_count": 0,
        "results": [],
        "summary": "Knowledge retrieval is unavailable; do not infer that the corpus is empty."
    })
    .to_string()
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
                candidate("Verevon onboarding flow.", "doc-1", 0.42),
                candidate("Billing trial is 14 days.", "doc-2", 0.31),
            ],
            ..Default::default()
        };
        let out = format_candidates("onboarding", &resp);
        let parsed: serde_json::Value =
            serde_json::from_str(&out).expect("knowledge result is machine readable JSON");
        assert_eq!(parsed["selected_route"], "hybrid");
        assert_eq!(parsed["route_control"], "server_managed");
        assert_eq!(parsed["status"], "ok");
        assert_eq!(parsed["low_confidence"], false);
        assert_eq!(parsed["degraded"], false);
        assert_eq!(parsed["no_results"], false);
        assert_eq!(parsed["results"][0]["document_id"], "doc-1");
        assert!(out.contains("2 chunk(s)"));
        assert!(out.contains("1. [score 0.420] Verevon onboarding flow."));
        assert!(out.contains("(document doc-1)"));
        assert!(out.contains("2. [score 0.310] Billing trial is 14 days."));
    }

    #[test]
    fn empty_results_are_informative_not_error() {
        let out = format_candidates("zzz", &RetrieveResponse::default());
        let parsed: serde_json::Value = serde_json::from_str(&out).expect("valid JSON envelope");
        assert_eq!(parsed["status"], "no_results");
        assert_eq!(parsed["no_results"], true);
        assert!(parsed["reason"]
            .as_str()
            .expect("reason")
            .contains("reformulate"));
        assert!(out.contains("No knowledge found"));
    }

    #[test]
    fn empty_results_note_low_confidence() {
        let resp = RetrieveResponse {
            low_confidence: true,
            ..Default::default()
        };
        let out = format_candidates("q", &resp);
        let parsed: serde_json::Value = serde_json::from_str(&out).expect("valid JSON envelope");
        assert_eq!(parsed["status"], "no_results");
        assert_eq!(parsed["low_confidence"], true);
        assert!(out.contains("low confidence"));
    }

    #[test]
    fn candidates_preserve_low_confidence_as_a_distinct_machine_signal() {
        let resp = RetrieveResponse {
            candidates: vec![candidate("weak match", "doc-1", 0.05)],
            low_confidence: true,
            trace_id: "trace-1".to_owned(),
            ..Default::default()
        };

        let out = format_candidates("ambiguous", &resp);
        let parsed: serde_json::Value = serde_json::from_str(&out).expect("valid JSON envelope");
        assert_eq!(parsed["status"], "low_confidence");
        assert_eq!(parsed["low_confidence"], true);
        assert_eq!(parsed["degraded"], false);
        assert_eq!(parsed["trace_id"], "trace-1");
        assert!(parsed["reason"]
            .as_str()
            .expect("reason")
            .contains("reformulate"));
    }

    #[test]
    fn dependency_failure_is_degraded_not_no_results() {
        let out = format_degraded_error("retrieval dependency unavailable");
        let parsed: serde_json::Value = serde_json::from_str(&out).expect("valid JSON envelope");
        assert_eq!(parsed["status"], "degraded");
        assert_eq!(parsed["degraded"], true);
        assert_eq!(parsed["no_results"], false);
        assert_eq!(parsed["query"], "");
        assert!(parsed["reason"]
            .as_str()
            .expect("reason")
            .contains("unavailable"));
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

    #[test]
    fn knowledge_request_forwards_verified_bearer_and_zdr_without_identity_headers() {
        let request = build_request("org-a", "user-a", "query", 5, true, "signed-token")
            .expect("build authenticated request");

        assert_eq!(
            request
                .metadata()
                .get("authorization")
                .and_then(|value| value.to_str().ok()),
            Some("Bearer signed-token")
        );
        assert!(request.metadata().get("x-api-key").is_none());
        assert!(request.metadata().get("x-user-id").is_none());
        assert_eq!(request.get_ref().org_id, "org-a");
        assert_eq!(request.get_ref().user_id.as_deref(), Some("user-a"));
        assert_eq!(request.get_ref().zdr_mode.as_deref(), Some("ephemeral"));
    }
}
