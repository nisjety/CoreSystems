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
//!
//! Sovereignty posture: this crate holds no signed `sovereign` claim (that
//! lives on the auth token, not the step), so the only signal it can offer
//! Data Plane v2 is the run's `min_privacy_tier` — see `build_request` and
//! `mp_contracts::dataplane_posture`.

// `# Errors` prose for `Result<String, String>` is noise; `doc_markdown`
// over-flags wire tokens. Low-signal pedantic lints — scoped-allowed.
#![allow(clippy::missing_errors_doc, clippy::doc_markdown)]

use mp_contracts::dataplane::retrieval_v2::{
    retrieval_service_client::RetrievalServiceClient, RetrieveRequest, RetrieveResponse,
};
use serde::Serialize;
use std::time::Duration;
use tonic::metadata::MetadataValue;
use tonic::transport::{Channel, Endpoint};

const MAX_TOP_K: i32 = 20;
const MAX_CHUNK_CHARS: usize = 600;

/// The Data Plane retrieval gRPC endpoint, as configured for this process.
///
/// The ONE resolver for everything that talks to retrieval: dispatch
/// ([`KnowledgeClient::from_env`]) and the health probe
/// ([`retrieval_auth_boundary_alive`]) both call it, so the endpoint a
/// `cap.retrieval.query` attestation vouches for cannot drift from the endpoint
/// `knowledge_search` really dials. `DATAPLANE_RETRIEVAL_URL` wins over
/// `DATAPLANE_RETRIEVAL_ADDR`; a blank value is "not configured", not a reason to
/// fall through to the other name.
#[must_use]
pub(crate) fn retrieval_endpoint_from_env() -> Option<String> {
    retrieval_endpoint_from_values(
        std::env::var("DATAPLANE_RETRIEVAL_URL").ok(),
        std::env::var("DATAPLANE_RETRIEVAL_ADDR").ok(),
    )
}

/// Pure core of [`retrieval_endpoint_from_env`] (env access stays out of the
/// precedence rule so the rule is testable without touching the process env).
fn retrieval_endpoint_from_values(url: Option<String>, addr: Option<String>) -> Option<String> {
    url.or(addr).filter(|value| !value.trim().is_empty())
}

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
        let url = retrieval_endpoint_from_env()?;
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
        // The run's `ExecuteStepRequest.min_privacy_tier` (field 10), the wire
        // numeric of the shared `PrivacyTier` enum. Only the SOVEREIGN value
        // says anything on the sovereignty axis; see
        // `mp_contracts::dataplane_posture::sovereign_required_from_privacy_tier`.
        min_privacy_tier: i32,
        bearer: &str,
    ) -> Result<String, String> {
        let top_k = top_k.clamp(1, MAX_TOP_K);
        let request = build_request(org_id, user_id, query, top_k, zdr, min_privacy_tier, bearer)?;
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
    min_privacy_tier: i32,
    bearer: &str,
) -> Result<tonic::Request<RetrieveRequest>, String> {
    if bearer.trim().is_empty() || bearer != bearer.trim() {
        return Err("knowledge retrieval requires a verified user credential".to_owned());
    }
    let authorization = MetadataValue::try_from(format!("Bearer {bearer}"))
        .map_err(|_| "knowledge retrieval credential is malformed".to_owned())?;
    // Jurisdiction axis, and it must be populated: Data Plane v2 reads an
    // absent `sovereign_required` as `true` (fail-closed, since absence of
    // proof is not proof that egress is permitted), which Azure-hosted Cohere
    // Embed v4 can never satisfy — so an omitted field made every governed-loop
    // `knowledge_search` fail before retrieval ran.
    //
    // This crate holds no signed sovereignty claim (`ExecuteStepRequest` has
    // no such field, and none is planned — that lives on the auth token, not
    // the step), so `claim` is always `None` here; the run's privacy floor is
    // the only signal available, exactly like every other caller with no claim
    // of its own. A signed `sovereign = true` on the forwarded user bearer
    // still wins: retrieval-engine-rs re-applies its own floor on receipt, so
    // this can fail to raise the posture but never relax one.
    let sovereign_required = mp_contracts::dataplane_posture::effective_sovereign_required(
        None,
        mp_contracts::dataplane_posture::sovereign_required_from_privacy_tier(min_privacy_tier),
    );
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
        sovereign_required: Some(sovereign_required),
        ..Default::default()
    });
    request
        .metadata_mut()
        .insert("authorization", authorization);
    Ok(request)
}

/// Is the retrieval engine's authentication boundary alive on `endpoint`?
///
/// Half of the `cap.retrieval.query` health probe (the other half is the
/// engine's own `/readyz`). It sends ONE `Retrieve` with NO credential to the
/// exact endpoint [`KnowledgeClient`] dispatches to, and counts the engine as
/// alive only when that call is answered `Unauthenticated`. That single answer
/// proves DNS, TCP, HTTP/2, the `RetrievalService` route and the JWT interceptor
/// in front of it are all up, which a TCP connect alone does not. `Unavailable`,
/// `Unimplemented`, a timeout, a transport failure, an unparseable endpoint or
/// anything else (including an unexpected success, which would mean there is no
/// auth boundary at all) is NOT alive.
///
/// No bearer is ever sent: the probe has no user to speak for, and attaching a
/// real credential to a health check would be a privilege leak with no upside.
/// The request names no org, user or query, so it carries no data either, and
/// the engine rejects it at the interceptor before any handler (and so any
/// embedding or index work) runs. `sovereign_required` is set explicitly, to the
/// fail-closed value, only because `retrieval_sovereignty_contract` requires
/// every `RetrieveRequest` literal in the workspace to declare it.
///
/// `timeout` bounds the whole probe (connect plus call), so a hung dependency
/// cannot stall the health heartbeat.
pub(crate) async fn retrieval_auth_boundary_alive(endpoint: &str, timeout: Duration) -> bool {
    let Ok(endpoint) = Endpoint::from_shared(endpoint.to_owned()) else {
        return false;
    };
    let outcome = tokio::time::timeout(timeout, async {
        // An eager connect, unlike dispatch's lazy channel: the probe exists to
        // find out whether the connection can be made at all.
        let channel = endpoint
            .connect_timeout(timeout)
            .connect()
            .await
            .map_err(|_| tonic::Code::Unavailable)?;
        RetrievalServiceClient::new(channel)
            .retrieve(tonic::Request::new(RetrieveRequest {
                sovereign_required: Some(true),
                ..Default::default()
            }))
            .await
            .map(|_| ())
            .map_err(|status| status.code())
    })
    .await;
    outcome.is_ok_and(is_auth_boundary_answer)
}

/// The probe's verdict, pure so it can be tabulated: ONLY `Unauthenticated`
/// counts. See [`retrieval_auth_boundary_alive`].
fn is_auth_boundary_answer(outcome: Result<(), tonic::Code>) -> bool {
    matches!(outcome, Err(tonic::Code::Unauthenticated))
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

// `pub(crate)` so `health_attest`'s tests can reuse the stub retrieval server
// below instead of growing a second copy of it.
#[cfg(test)]
pub(crate) mod tests {
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
        let request = build_request("org-a", "user-a", "query", 5, true, 0, "signed-token")
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

    /// The gap this module used to name in a comment: a run pinned to
    /// SOVEREIGN privacy tier now gets a sovereign retrieval posture too, not
    /// just sovereign model serving. `4` is `PrivacyTier::PRIVACY_TIER_SOVEREIGN`
    /// — pinned against the generated enum in `dataplane_posture`'s own test,
    /// so this one is free to use the literal.
    #[test]
    fn a_sovereign_privacy_tier_raises_the_retrieval_posture_too() {
        const PRIVACY_TIER_SOVEREIGN: i32 = 4;
        let request = build_request(
            "org-a",
            "user-a",
            "query",
            5,
            false,
            PRIVACY_TIER_SOVEREIGN,
            "signed-token",
        )
        .expect("build authenticated request");
        assert_eq!(request.get_ref().sovereign_required, Some(true));
    }

    /// Every other tier says nothing about jurisdiction, so it must not raise
    /// OR lower the posture — it lands on the ordinary no-signal default.
    #[test]
    fn a_non_sovereign_privacy_tier_defers_to_the_no_signal_default() {
        for tier in [0, 1, 2, 3] {
            let request = build_request("org-a", "user-a", "query", 5, false, tier, "signed-token")
                .expect("build authenticated request");
            assert_eq!(
                request.get_ref().sovereign_required,
                Some(mp_contracts::dataplane_posture::SOVEREIGN_REQUIRED_WITHOUT_SIGNAL),
                "tier {tier} must not decide the sovereignty axis"
            );
        }
    }

    // ---- the unauthenticated retrieval probe (cap.retrieval.query health) ------

    use mp_contracts::dataplane::retrieval_v2::retrieval_service_server::{
        RetrievalService, RetrievalServiceServer,
    };
    use mp_contracts::dataplane::retrieval_v2::{
        GetChunksRequest, GetChunksResponse, GetSourcesRequest, GetSourcesResponse,
        GetTraceRequest, GetTraceResponse, PackContextRequest, PackContextResponse, RetrievalChunk,
    };
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};
    use tokio::net::TcpListener;
    use tokio_stream::wrappers::TcpListenerStream;
    use tonic::{transport::Server, Request, Response, Status};

    /// What the stub `Retrieve` answers an unauthenticated caller with.
    #[derive(Clone, Copy, Debug)]
    pub(crate) enum Answer {
        /// The real engine's behaviour: its JWT interceptor rejects a request with
        /// no credential before any handler runs.
        Unauthenticated,
        Unavailable,
        Unimplemented,
        PermissionDenied,
        /// A server that answers an unauthenticated call successfully has NO
        /// working auth boundary; that is not "healthy".
        Success,
        /// Answers `Unauthenticated`, but only after far longer than any probe waits.
        TooSlow,
    }

    /// What the stub saw of the probe's one request.
    #[derive(Clone, Debug, Default)]
    pub(crate) struct Seen {
        metadata_keys: Vec<String>,
        org_id: String,
        query: String,
        user_id: Option<String>,
    }

    struct StubRetrieval {
        answer: Answer,
        seen: Arc<Mutex<Vec<Seen>>>,
    }

    type NoStream =
        Pin<Box<dyn futures::Stream<Item = Result<RetrievalChunk, Status>> + Send + 'static>>;

    #[tonic::async_trait]
    impl RetrievalService for StubRetrieval {
        async fn retrieve(
            &self,
            request: Request<RetrieveRequest>,
        ) -> Result<Response<RetrieveResponse>, Status> {
            self.seen.lock().expect("seen lock").push(Seen {
                metadata_keys: request
                    .metadata()
                    .clone()
                    .into_headers()
                    .keys()
                    .map(|key| key.as_str().to_owned())
                    .collect(),
                org_id: request.get_ref().org_id.clone(),
                query: request.get_ref().query.clone(),
                user_id: request.get_ref().user_id.clone(),
            });
            match self.answer {
                Answer::Unauthenticated => {
                    Err(Status::unauthenticated("invalid or missing credential"))
                }
                Answer::Unavailable => Err(Status::unavailable("stub is down")),
                Answer::Unimplemented => Err(Status::unimplemented("stub has no retrieval")),
                Answer::PermissionDenied => Err(Status::permission_denied("stub denies")),
                Answer::Success => Ok(Response::new(RetrieveResponse::default())),
                Answer::TooSlow => {
                    tokio::time::sleep(Duration::from_secs(30)).await;
                    Err(Status::unauthenticated("too late"))
                }
            }
        }

        type RetrieveStreamStream = NoStream;
        async fn retrieve_stream(
            &self,
            _: Request<RetrieveRequest>,
        ) -> Result<Response<Self::RetrieveStreamStream>, Status> {
            Err(Status::unimplemented("not needed in this test"))
        }
        async fn get_trace(
            &self,
            _: Request<GetTraceRequest>,
        ) -> Result<Response<GetTraceResponse>, Status> {
            Err(Status::unimplemented("not needed in this test"))
        }
        async fn get_sources(
            &self,
            _: Request<GetSourcesRequest>,
        ) -> Result<Response<GetSourcesResponse>, Status> {
            Err(Status::unimplemented("not needed in this test"))
        }
        async fn get_chunks(
            &self,
            _: Request<GetChunksRequest>,
        ) -> Result<Response<GetChunksResponse>, Status> {
            Err(Status::unimplemented("not needed in this test"))
        }
        async fn pack_context(
            &self,
            _: Request<PackContextRequest>,
        ) -> Result<Response<PackContextResponse>, Status> {
            Err(Status::unimplemented("not needed in this test"))
        }
    }

    pub(crate) async fn spawn_stub(answer: Answer) -> (String, Arc<Mutex<Vec<Seen>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind stub");
        let address = listener.local_addr().expect("stub address");
        let seen = Arc::new(Mutex::new(Vec::new()));
        let stub = StubRetrieval {
            answer,
            seen: seen.clone(),
        };
        tokio::spawn(async move {
            Server::builder()
                .add_service(RetrievalServiceServer::new(stub))
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await
                .ok();
        });
        (format!("http://{address}"), seen)
    }

    const TEST_PROBE_TIMEOUT: Duration = Duration::from_secs(2);

    #[tokio::test]
    async fn an_unauthenticated_answer_proves_the_route_and_its_jwt_boundary_are_alive() {
        let (endpoint, seen) = spawn_stub(Answer::Unauthenticated).await;
        assert!(retrieval_auth_boundary_alive(&endpoint, TEST_PROBE_TIMEOUT).await);

        // The probe is credential-free and data-free: no bearer (or any other
        // credential header), and a request that names no org, user or query.
        let seen = seen.lock().expect("seen lock").clone();
        assert_eq!(seen.len(), 1, "exactly one Retrieve per probe");
        for key in &seen[0].metadata_keys {
            assert!(
                !matches!(key.as_str(), "authorization" | "x-api-key" | "x-user-id"),
                "the probe must not send credential header {key}"
            );
        }
        assert!(seen[0].org_id.is_empty());
        assert!(seen[0].query.is_empty());
        assert_eq!(seen[0].user_id, None);
    }

    #[tokio::test]
    async fn every_answer_other_than_unauthenticated_is_unhealthy() {
        for answer in [
            Answer::Unavailable,
            Answer::Unimplemented,
            Answer::PermissionDenied,
            Answer::Success,
        ] {
            let (endpoint, _) = spawn_stub(answer).await;
            assert!(
                !retrieval_auth_boundary_alive(&endpoint, TEST_PROBE_TIMEOUT).await,
                "{answer:?} must not read as a live auth boundary"
            );
        }
    }

    #[tokio::test]
    async fn a_server_slower_than_the_timeout_is_unhealthy_and_the_probe_returns_in_time() {
        let (endpoint, _) = spawn_stub(Answer::TooSlow).await;
        let started = std::time::Instant::now();
        assert!(!retrieval_auth_boundary_alive(&endpoint, Duration::from_millis(300)).await);
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the probe must give up at its own timeout"
        );
    }

    #[tokio::test]
    async fn a_server_that_accepts_the_connection_and_never_speaks_is_unhealthy_in_time() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind mute");
        let address = listener.local_addr().expect("mute address");
        tokio::spawn(async move {
            let mut held = Vec::new();
            while let Ok((socket, _)) = listener.accept().await {
                held.push(socket);
            }
        });
        let started = std::time::Instant::now();
        assert!(
            !retrieval_auth_boundary_alive(
                &format!("http://{address}"),
                Duration::from_millis(300)
            )
            .await
        );
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[tokio::test]
    async fn a_closed_port_and_an_unparseable_endpoint_are_unhealthy() {
        let port = {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("reserve a port");
            listener.local_addr().expect("address").port()
        };
        assert!(
            !retrieval_auth_boundary_alive(&format!("http://127.0.0.1:{port}"), TEST_PROBE_TIMEOUT)
                .await
        );
        assert!(!retrieval_auth_boundary_alive("not a url", TEST_PROBE_TIMEOUT).await);
    }

    /// The verdict is the whole safety argument, so it is pinned as a table:
    /// ONLY `Unauthenticated` is healthy.
    #[test]
    fn only_unauthenticated_is_a_healthy_verdict() {
        use tonic::Code;
        assert!(is_auth_boundary_answer(Err(Code::Unauthenticated)));
        assert!(!is_auth_boundary_answer(Ok(())));
        for code in [
            Code::Cancelled,
            Code::Unknown,
            Code::InvalidArgument,
            Code::DeadlineExceeded,
            Code::NotFound,
            Code::AlreadyExists,
            Code::PermissionDenied,
            Code::ResourceExhausted,
            Code::FailedPrecondition,
            Code::Aborted,
            Code::OutOfRange,
            Code::Unimplemented,
            Code::Internal,
            Code::Unavailable,
            Code::DataLoss,
        ] {
            assert!(
                !is_auth_boundary_answer(Err(code)),
                "{code:?} must not be a healthy verdict"
            );
        }
    }

    // ---- one endpoint resolver for dispatch and probe -------------------------

    #[test]
    fn the_endpoint_resolver_keeps_dispatchs_precedence_and_blank_handling() {
        let some = |value: &str| Some(value.to_owned());
        // URL wins over ADDR.
        assert_eq!(
            retrieval_endpoint_from_values(some("http://url:50052"), some("http://addr:50052")),
            some("http://url:50052")
        );
        // ADDR is used only when URL is absent.
        assert_eq!(
            retrieval_endpoint_from_values(None, some("http://addr:50052")),
            some("http://addr:50052")
        );
        // A blank URL is "not configured" — it does NOT fall through to ADDR,
        // exactly as dispatch always behaved.
        assert_eq!(
            retrieval_endpoint_from_values(some("  "), some("http://addr:50052")),
            None
        );
        assert_eq!(retrieval_endpoint_from_values(None, None), None);
        assert_eq!(retrieval_endpoint_from_values(None, some("")), None);
    }

    /// Dispatch must resolve its endpoint ONLY through the shared helper. If
    /// `from_env` grew its own `env::var` reads again, the probe could vouch for
    /// one endpoint while `knowledge_search` dials another.
    #[test]
    fn dispatch_reads_the_endpoint_only_through_the_shared_resolver() {
        let source = include_str!("knowledge_tools.rs");
        let production = source
            .split("#[cfg(test)]")
            .next()
            .expect("source has a production part");
        assert_eq!(
            production
                .matches("std::env::var(\"DATAPLANE_RETRIEVAL_URL\")")
                .count(),
            1,
            "the retrieval endpoint env must be read in exactly one place"
        );
        assert_eq!(
            production
                .matches("std::env::var(\"DATAPLANE_RETRIEVAL_ADDR\")")
                .count(),
            1
        );
        assert!(
            production.contains("let url = retrieval_endpoint_from_env()?;"),
            "KnowledgeClient::from_env must use the shared resolver"
        );
    }
}
