//! Z-41: a ZDR retrieval with no attested query embedding route answers from lexical search only,
//! with an explicit notice. The dense route's egress decision is taken before any provider call, and
//! an attestation changes that decision only when it describes the route it is bound to.
//!
//! The tests drive the real `RetrievalPipeline::retrieve` with the production constructors and the
//! real egress attestation, resolved from configuration. The Postgres pool is closed and Qdrant is a
//! closed local port, so no database or provider is reached. The sparse arm is a double that counts
//! its calls: it is the lexical store the degrade falls back to, and the rule under test is the
//! orchestrator's routing, not the store's. With no candidates the source join and the visibility
//! gate return before any query. A Reject request also attempts its trace write, which fails against
//! the closed pool and is recorded as an unpersisted trace, as the orchestrator tolerates, so no
//! request here needs a live database.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use chrono::NaiveDate;
use retrieval_engine::authz::{
    NoopPolicyClient, NoopVisibilityClient, PolicyClient, VisibilityClient,
};
use retrieval_engine::config::Config;
use retrieval_engine::egress_attestation::{EgressAttestation, EmbedSurface};
use retrieval_engine::embed::media::MediaQueryEmbedder;
use retrieval_engine::embed::visual::VisualQueryEmbedder;
use retrieval_engine::embed::{is_zdr_refusal, EmbeddingClient};
use retrieval_engine::pipeline::orchestrator::RetrievalPipeline;
use retrieval_engine::pipeline::types::{RetrievalRequest, ScoredCandidate};
use retrieval_engine::search::graph_remote::GraphTraverseClient;
use retrieval_engine::search::keyword::MeilisearchQueryClient;
use retrieval_engine::search::sparse::{DynSparseSearchBackend, SparseSearchBackend};
use sqlx::PgPool;

/// A closed local port: a call to it fails at once, and a refusal is observable as the absence of
/// any such failure.
const UNREACHABLE: &str = "http://127.0.0.1:1/unreachable";

/// The attested Foundry resource the configuration below names. Its digest is the one the runbook's
/// `printf ... | sha256sum` pipeline produces over the five fields.
const ATTESTATION: &[(&str, &str)] = &[
    (
        "AZURE_COHERE_ZDR_RESOURCE_ID",
        "/subscriptions/0000/resourceGroups/rg-data/providers/Microsoft.CognitiveServices/accounts/aquatiq-cohere",
    ),
    ("AZURE_COHERE_ZDR_APPROVAL_REF", "MAM-2026-0042"),
    ("AZURE_COHERE_ZDR_EFFECTIVE_DATE", "2026-06-01"),
    ("AZURE_COHERE_ZDR_REVIEW_BY", "2027-06-01"),
    ("AZURE_COHERE_ZDR_REVIEWER", "ima@aquatiq.com"),
    (
        "AZURE_COHERE_ZDR_DIGEST",
        "b287e12686ffe907defa848e8887ed6389b8da3910041e5e8e6bd33ef368f76c",
    ),
];

const ATTESTED_FOUNDRY: &str = "https://aquatiq-cohere.services.ai.azure.com";

fn today() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 10, 11).expect("static date")
}

/// The production configuration read from a minimal environment. `extra` adds to it.
fn config(extra: &[(&str, &str)]) -> Config {
    let mut env: Vec<(String, String)> = vec![
        (
            "DATABASE_URL".to_string(),
            "postgres://unused:unused@127.0.0.1:1/unused".to_string(),
        ),
        ("QDRANT_URL".to_string(), "http://127.0.0.1:1".to_string()),
        ("EMBEDDING_DIMENSION".to_string(), "1536".to_string()),
    ];
    env.extend(
        extra
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string())),
    );
    envy::from_iter(env).expect("the configuration deserializes with the production defaults")
}

/// The lexical store: counts its calls and answers with no candidates.
struct CountingSparse {
    calls: AtomicUsize,
}

#[async_trait]
impl SparseSearchBackend for CountingSparse {
    fn name(&self) -> &'static str {
        "counting-double"
    }

    async fn search(
        &self,
        _query: &str,
        _org_id: &str,
        _top_k: usize,
    ) -> anyhow::Result<Vec<ScoredCandidate>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(Vec::new())
    }
}

/// The production wiring from `main.rs`, with the Cohere query embedder attached to `egress` and
/// pointed at `embed_endpoint`. The pool is closed, so nothing reaches Postgres.
async fn pipeline(
    cfg: Config,
    egress: &EgressAttestation,
    embed_endpoint: &str,
) -> (RetrievalPipeline, Arc<CountingSparse>) {
    let pool = PgPool::connect_lazy(&cfg.database_url).expect("lazy pool configuration");
    pool.close().await;
    let qdrant = qdrant_client::Qdrant::from_url(&cfg.qdrant_url)
        .skip_compatibility_check()
        .build()
        .expect("qdrant client builds without connecting");
    let embedder = EmbeddingClient::cohere(
        embed_endpoint,
        "fake-key",
        &cfg.cohere_embed_v4_deployment,
        &cfg.cohere_embed_v4_api_version,
        cfg.embedding_dimension,
    )
    .expect("cohere client builds")
    .with_egress_attestation(egress);
    let sparse = Arc::new(CountingSparse {
        calls: AtomicUsize::new(0),
    });
    let sparse_backend: DynSparseSearchBackend = sparse.clone();
    let policy: Arc<dyn PolicyClient> = Arc::new(NoopPolicyClient);
    let visibility: Arc<dyn VisibilityClient> = Arc::new(NoopVisibilityClient);
    let pipeline = RetrievalPipeline {
        pool,
        qdrant,
        embedder,
        reranker: None,
        cache: None,
        policy,
        visibility,
        nats: None,
        event_signer: None,
        sparse_backend,
        visual_embedder: VisualQueryEmbedder::from_config(&cfg)
            .expect("visual embedder config is valid"),
        media_embedder: MediaQueryEmbedder::from_config(&cfg),
        colqwen: None,
        graph_remote: GraphTraverseClient::from_config(
            &cfg.graph_index_url,
            cfg.graph_remote_timeout_ms,
        ),
        keyword_client: MeilisearchQueryClient::from_config(
            &cfg.meilisearch_url,
            &cfg.meilisearch_api_key,
            &cfg.meilisearch_index_uid,
        ),
        config: cfg,
    };
    (pipeline, sparse)
}

/// A request whose blend routes the dense arm (`w_dense`) and the lexical arm (`w_bm25`).
fn request(
    zdr_mode: Option<&str>,
    sovereign_required: Option<bool>,
    w_dense: f64,
) -> RetrievalRequest {
    let mut body = serde_json::json!({
        "org_id": "org-z41",
        "query": "how often is the conveyor belt cleaned",
        "top_n": 5,
        "mode_mix": {
            "w_dense": w_dense,
            "w_bm25": 0.2,
            "w_graph": 0.0,
            "w_wiki": 0.0,
            "w_visual": 0.0,
            "w_keyword": 0.0,
            "rerank": false
        }
    });
    if let Some(mode) = zdr_mode {
        body["zdr_mode"] = serde_json::json!(mode);
    }
    if let Some(required) = sovereign_required {
        body["sovereign_required"] = serde_json::json!(required);
    }
    serde_json::from_value(body).expect("the request shape parses")
}

/// The attestation for the Foundry resource, bound to the embedding endpoint `embed_endpoint`.
fn attested_config(embed_endpoint: &str) -> Config {
    let mut pairs: Vec<(&str, &str)> = ATTESTATION.to_vec();
    pairs.push(("COHERE_EMBED_V4_ENDPOINT", embed_endpoint));
    // The visual query embedder shares the Foundry endpoint and refuses to build without a key.
    pairs.push(("COHERE_EMBED_V4_API_KEY", "fake-key"));
    config(&pairs)
}

#[tokio::test]
async fn a_zdr_query_without_an_attested_route_answers_lexically_with_a_notice() {
    let (pipeline, sparse) =
        pipeline(config(&[]), &EgressAttestation::default(), UNREACHABLE).await;

    let response = pipeline
        .retrieve(request(Some("ephemeral"), Some(false), 0.45))
        .await
        .expect("a ZDR retrieval answers lexically; it does not fail");

    let notice = response
        .notice
        .as_ref()
        .expect("the reduced answer carries an explicit notice");
    assert_eq!(notice.code, "zdr_dense_unavailable");
    assert!(
        response
            .zdr_actions_applied
            .iter()
            .any(|action| action == "zdr_dense_skipped_unattested"),
        "the audit trail records the skipped dense arm: {:?}",
        response.zdr_actions_applied
    );
    assert_eq!(
        sparse.calls.load(Ordering::SeqCst),
        1,
        "the lexical arm answered"
    );
    let wire = serde_json::to_value(&response).expect("the response serialises");
    assert_eq!(wire["notice"]["code"], "zdr_dense_unavailable");
}

#[tokio::test]
async fn a_reject_query_without_an_attested_route_degrades_the_same_way() {
    let (pipeline, _sparse) =
        pipeline(config(&[]), &EgressAttestation::default(), UNREACHABLE).await;

    let response = pipeline
        .retrieve(request(Some("reject"), Some(false), 0.45))
        .await
        .expect("a Reject retrieval answers lexically; it does not fail");

    assert_eq!(
        response.notice.map(|notice| notice.code).as_deref(),
        Some("zdr_dense_unavailable")
    );
}

#[tokio::test]
async fn a_sovereign_request_refuses_the_dense_route_before_any_embedding_even_under_zdr() {
    let (pipeline, sparse) =
        pipeline(config(&[]), &EgressAttestation::default(), UNREACHABLE).await;

    let err = pipeline
        .retrieve(request(Some("ephemeral"), Some(true), 0.45))
        .await
        .expect_err("an Azure-hosted embedder can never satisfy sovereignty");

    assert!(err.to_string().contains("sovereign"), "{err:#}");
    assert_eq!(
        sparse.calls.load(Ordering::SeqCst),
        0,
        "the refusal comes before any arm runs"
    );
}

#[tokio::test]
async fn without_zdr_a_failed_embedding_stays_fatal_as_it_was() {
    let (pipeline, _sparse) =
        pipeline(config(&[]), &EgressAttestation::default(), UNREACHABLE).await;

    let err = pipeline
        .retrieve(request(None, Some(false), 0.45))
        .await
        .expect_err("the closed embedding endpoint cannot answer");

    let text = format!("{err:#}");
    assert!(
        text.contains("Cohere text embedding API call failed"),
        "the dense arm reached its provider: {text}"
    );
    assert!(!is_zdr_refusal(&err), "{text}");
}

#[tokio::test]
async fn an_attested_zdr_route_runs_the_dense_arm_instead_of_degrading() {
    let cfg = attested_config(ATTESTED_FOUNDRY);
    let egress = EgressAttestation::from_config(&cfg, today());
    assert!(
        egress.permits_embed(EmbedSurface::CohereFoundry),
        "the fixture attestation must be in force: {:?}",
        egress.problems()
    );
    // The client calls a closed local port, because the attested host cannot be reached from a
    // test. The attested flag comes from the real attestation, resolved from `ATTESTED_FOUNDRY`.
    let (pipeline, _sparse) = pipeline(cfg, &egress, UNREACHABLE).await;

    let err = pipeline
        .retrieve(request(Some("ephemeral"), Some(false), 0.45))
        .await
        .expect_err("the closed endpoint cannot answer, so the dense arm fails");

    let text = format!("{err:#}");
    assert!(
        text.contains("Cohere text embedding API call failed"),
        "the dense arm reached its provider: {text}"
    );
    assert!(
        !is_zdr_refusal(&err),
        "an attested route is not refused: {text}"
    );
}

#[tokio::test]
async fn an_attestation_for_another_host_degrades_rather_than_permitting() {
    let cfg = attested_config("https://other-foundry.services.ai.azure.com");
    let egress = EgressAttestation::from_config(&cfg, today());
    assert!(
        !egress.permits_embed(EmbedSurface::CohereFoundry),
        "an attestation describes only the host of its own resource"
    );
    assert!(
        egress
            .problems()
            .iter()
            .any(|problem| problem.contains("COHERE_EMBED_V4_ENDPOINT")),
        "{:?}",
        egress.problems()
    );
    let (pipeline, _sparse) = pipeline(cfg, &egress, UNREACHABLE).await;

    let response = pipeline
        .retrieve(request(Some("ephemeral"), Some(false), 0.45))
        .await
        .expect("the unattested route degrades; it does not fail");

    assert_eq!(
        response.notice.map(|notice| notice.code).as_deref(),
        Some("zdr_dense_unavailable")
    );
}

#[tokio::test]
async fn a_zdr_query_with_hybrid_search_off_fails_closed_instead_of_an_empty_notice() {
    let (pipeline, sparse) = pipeline(
        config(&[("HYBRID_ENABLED", "false")]),
        &EgressAttestation::default(),
        UNREACHABLE,
    )
    .await;

    let err = pipeline
        .retrieve(request(Some("ephemeral"), Some(false), 0.45))
        .await
        .expect_err("with no lexical arm there is no reduced answer to give");

    assert!(
        is_zdr_refusal(&err),
        "the refusal is surfaced unchanged: {err:#}"
    );
    assert_eq!(sparse.calls.load(Ordering::SeqCst), 0, "no lexical arm ran");
}

#[tokio::test]
async fn an_ordinary_lexical_answer_carries_no_notice_on_the_wire() {
    let (pipeline, sparse) =
        pipeline(config(&[]), &EgressAttestation::default(), UNREACHABLE).await;

    let response = pipeline
        .retrieve(request(None, Some(false), 0.0))
        .await
        .expect("a lexical blend never embeds, so it answers");

    assert!(response.notice.is_none());
    assert_eq!(sparse.calls.load(Ordering::SeqCst), 1);
    let wire = serde_json::to_value(&response).expect("the response serialises");
    assert!(
        wire.get("notice").is_none(),
        "an ordinary answer's wire form is unchanged: {wire}"
    );
}
