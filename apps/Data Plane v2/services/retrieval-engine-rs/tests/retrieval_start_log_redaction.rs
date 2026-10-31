//! The `retrieval.start` log line must not carry the query text while the
//! posture is restrictive: ZDR `reject` or `ephemeral`, or sovereignty required
//! (an absent `sovereign_required` resolves to required, see
//! `sovereign_required_or_strict`).
//!
//! The tests drive the real `RetrievalPipeline::retrieve` with the production
//! constructors. The Postgres pool is closed before use, so every database call
//! fails at once. The lexical cases make no embedding call. The dense case
//! reaches the embedder and stops at a closed local port during token minting.
//! No provider or network service is reached. What is under test is what the
//! process writes to its log: every event the test binary emits, at every
//! level, is captured and searched for the query text.

use std::io::{self, Write};
use std::sync::{Arc, Mutex, OnceLock};

use retrieval_engine::authz::{
    NoopPolicyClient, NoopVisibilityClient, PolicyClient, VisibilityClient,
};
use retrieval_engine::config::Config;
use retrieval_engine::embed::media::MediaQueryEmbedder;
use retrieval_engine::embed::visual::VisualQueryEmbedder;
use retrieval_engine::embed::EmbeddingClient;
use retrieval_engine::pipeline::orchestrator::RetrievalPipeline;
use retrieval_engine::pipeline::types::RetrievalRequest;
use retrieval_engine::search::graph_remote::GraphTraverseClient;
use retrieval_engine::search::keyword::MeilisearchQueryClient;
use retrieval_engine::search::sparse::{DynSparseSearchBackend, PostgresSparseBackend};
use sqlx::PgPool;
use tracing_subscriber::fmt::MakeWriter;

/// Every byte the test binary's global subscriber writes.
#[derive(Clone, Default)]
struct LogBuffer(Arc<Mutex<Vec<u8>>>);

impl LogBuffer {
    fn contents(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().expect("log buffer lock")).into_owned()
    }
}

impl Write for LogBuffer {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .expect("log buffer lock")
            .extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for LogBuffer {
    type Writer = LogBuffer;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// Installs the capturing subscriber once for this test binary. `TRACE` keeps
/// every level, so a leak at any level fails the test.
fn captured_logs() -> LogBuffer {
    static LOGS: OnceLock<LogBuffer> = OnceLock::new();
    LOGS.get_or_init(|| {
        let buffer = LogBuffer::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(buffer.clone())
            .with_ansi(false)
            .with_max_level(tracing::Level::TRACE)
            .finish();
        tracing::subscriber::set_global_default(subscriber)
            .expect("this test binary installs exactly one subscriber");
        buffer
    })
    .clone()
}

/// The production wiring from `main.rs`, built from a minimal environment. The
/// database pool is closed, so nothing reaches Postgres, and the Qdrant client
/// skips its startup version probe. The model-plane identity is a dummy: it
/// makes the embedder construct the way production does, and the model plane is
/// a closed local port, so a token mint is refused rather than served. The policy
/// and visibility clients are the production no-op clients: neither is consulted
/// for a request without a user.
async fn pipeline() -> RetrievalPipeline {
    // The capture must be installed before the first event: an event emitted
    // before the global subscriber exists is never recorded.
    captured_logs();
    let config: Config = envy::from_iter(vec![
        (
            "DATABASE_URL".to_string(),
            "postgres://unused:unused@127.0.0.1:1/unused".to_string(),
        ),
        ("QDRANT_URL".to_string(), "http://127.0.0.1:1".to_string()),
        // Closed local ports: a dense-routed request fails at once instead of
        // reaching a model plane.
        (
            "MODEL_PLANE_AI_CORE_GRPC_URL".to_string(),
            "http://127.0.0.1:1".to_string(),
        ),
        (
            "MODEL_PLANE_INFERENCE_TOKEN_URL".to_string(),
            "http://127.0.0.1:1/api/inference-core/internal-token".to_string(),
        ),
        (
            "MODEL_PLANE_INFERENCE_SERVICE_ID".to_string(),
            "retrieval-engine-log-test".to_string(),
        ),
        (
            "MODEL_PLANE_INFERENCE_SERVICE_API_KEY".to_string(),
            "log-redaction-test-dummy-0001".to_string(),
        ),
    ])
    .expect("minimal environment deserializes with the production field defaults");

    let pool = PgPool::connect_lazy(&config.database_url).expect("lazy pool configuration");
    pool.close().await;

    let qdrant = qdrant_client::Qdrant::from_url(&config.qdrant_url)
        .skip_compatibility_check()
        .build()
        .expect("qdrant client builds without connecting");
    let embedder =
        EmbeddingClient::from_config(&config).expect("embedding backend builds without connecting");
    let sparse_backend: DynSparseSearchBackend = Arc::new(PostgresSparseBackend::new(pool.clone()));
    let policy: Arc<dyn PolicyClient> = Arc::new(NoopPolicyClient);
    let visibility: Arc<dyn VisibilityClient> = Arc::new(NoopVisibilityClient);

    RetrievalPipeline {
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
        visual_embedder: VisualQueryEmbedder::from_config(&config)
            .expect("visual embedder config is valid"),
        media_embedder: MediaQueryEmbedder::from_config(&config),
        colqwen: None,
        graph_remote: GraphTraverseClient::from_config(
            &config.graph_index_url,
            config.graph_remote_timeout_ms,
        ),
        keyword_client: MeilisearchQueryClient::from_config(
            &config.meilisearch_url,
            &config.meilisearch_api_key,
            &config.meilisearch_index_uid,
        ),
        config,
    }
}

/// A lexical-only request: no dense route (so no embedding call), and the
/// graph, wiki, visual and keyword arms are switched off.
fn request(
    org: &str,
    query: &str,
    zdr_mode: Option<&str>,
    sovereign_required: Option<bool>,
) -> RetrievalRequest {
    request_with_weights(org, query, zdr_mode, sovereign_required, 0.0, 1.0)
}

/// The same request with the dense and sparse weights given. The graph, wiki,
/// visual and keyword arms stay off.
fn request_with_weights(
    org: &str,
    query: &str,
    zdr_mode: Option<&str>,
    sovereign_required: Option<bool>,
    w_dense: f64,
    w_bm25: f64,
) -> RetrievalRequest {
    let mut body = serde_json::json!({
        "org_id": org,
        "query": query,
        "top_n": 5,
        "mode_mix": {
            "w_dense": w_dense,
            "w_bm25": w_bm25,
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

/// The `retrieval.start` events written for `org`. Each test uses its own org,
/// so the filter isolates its events from the other tests in this binary.
fn retrieval_start_lines<'a>(logs: &'a str, org: &str) -> Vec<&'a str> {
    logs.lines()
        .filter(|line| line.contains("retrieval.start") && line.contains(org))
        .collect()
}

/// The `(len, hash)` pair a withheld `retrieval.start` line carries.
fn withheld_fields(line: &str) -> (usize, String) {
    const OPEN: &str = "query=withheld(";
    let start = line
        .find(OPEN)
        .unwrap_or_else(|| panic!("no withheld query field in: {line}"))
        + OPEN.len();
    let rest = &line[start..];
    let body = &rest[..rest.find(')').expect("closing parenthesis")];
    let (len, hash) = body.split_once(",hash=").expect("len and hash fields");
    let len = len
        .strip_prefix("len=")
        .expect("len field")
        .parse()
        .expect("numeric length");
    (len, hash.to_owned())
}

/// Lines in the captured logs that mention `org`, for failure messages.
fn lines_for(logs: &str, org: &str) -> String {
    logs.lines()
        .filter(|line| line.contains(org))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The restrictive contract for one lexical request (see [`request`]).
async fn assert_text_withheld(
    org: &str,
    probe: &str,
    zdr_mode: Option<&str>,
    sovereign_required: Option<bool>,
) {
    assert_withheld_for(
        org,
        probe,
        request(org, probe, zdr_mode, sovereign_required),
    )
    .await;
}

/// The restrictive contract for one request: exactly one `retrieval.start`
/// line, the query text nowhere in any captured line, and the withheld form
/// with the byte length and a 16-hex-digit hash. Returns that line and the
/// retrieval's outcome. The outcome is not asserted here: it may fail once it
/// reaches the closed database or the closed model plane, and the line under
/// test was written before that.
async fn assert_withheld_for(org: &str, probe: &str, req: RetrievalRequest) -> (String, String) {
    let outcome = match pipeline().await.retrieve(req).await {
        Ok(_) => "ok".to_owned(),
        Err(error) => format!("{error:#}"),
    };
    let logs = captured_logs().contents();

    let starts = retrieval_start_lines(&logs, org);
    assert_eq!(
        starts.len(),
        1,
        "one retrieval.start line for {org}; org lines:\n{}",
        lines_for(&logs, org)
    );

    let leaked: Vec<&str> = logs.lines().filter(|line| line.contains(probe)).collect();
    assert!(
        leaked.is_empty(),
        "the query text reached the logs: {leaked:#?}"
    );

    let (len, hash) = withheld_fields(starts[0]);
    assert_eq!(len, probe.len(), "byte length of the withheld query");
    assert!(
        hash.len() == 16 && hash.bytes().all(|b| b.is_ascii_hexdigit()),
        "correlation hash is 16 hex digits, got {hash:?}"
    );
    (starts[0].to_owned(), outcome)
}

#[tokio::test]
async fn ephemeral_posture_withholds_the_query_text_from_every_log_line() {
    assert_text_withheld(
        "org-ephemeral-probe",
        "quarterly conveyor lubricant audit ephemeral",
        Some("ephemeral"),
        Some(false),
    )
    .await;
}

#[tokio::test]
async fn reject_posture_withholds_the_query_text_from_every_log_line() {
    assert_text_withheld(
        "org-reject-probe",
        "quarterly conveyor lubricant audit reject",
        Some("reject"),
        Some(false),
    )
    .await;
}

#[tokio::test]
async fn sovereignty_required_withholds_the_query_text_without_a_zdr_posture() {
    assert_text_withheld(
        "org-sovereign-probe",
        "quarterly conveyor lubricant audit sovereign",
        Some("disabled"),
        Some(true),
    )
    .await;
}

#[tokio::test]
async fn an_absent_sovereign_required_is_strict_and_withholds_the_query_text() {
    assert_text_withheld(
        "org-absent-probe",
        "quarterly conveyor lubricant audit absent",
        None,
        None,
    )
    .await;
}

#[tokio::test]
async fn the_dense_route_adds_no_query_text_to_the_logs_under_ephemeral() {
    // Dense routing embeds the query through the model plane. The embedding
    // path's own logs, its retries and the error it returns, must not carry the
    // text either.
    let org = "org-dense-probe";
    let probe = "quarterly conveyor lubricant audit dense";
    let (line, outcome) = assert_withheld_for(
        org,
        probe,
        request_with_weights(org, probe, Some("ephemeral"), Some(false), 1.0, 0.0),
    )
    .await;
    assert!(
        line.contains("route_dense=true"),
        "the dense route ran: {line}"
    );
    // The embedding call stops at the token mint on the closed endpoint; the
    // text is not yet sent anywhere.
    assert!(
        outcome.contains("inference service-token endpoint unavailable"),
        "the request reached the embedding path: {outcome}"
    );
}

#[tokio::test]
async fn a_disabled_posture_keeps_the_query_text_in_the_log_as_before() {
    let org = "org-disabled-probe";
    let probe = "quarterly conveyor lubricant audit disabled";
    let _ = pipeline()
        .await
        .retrieve(request(org, probe, Some("disabled"), Some(false)))
        .await;
    let logs = captured_logs().contents();

    let starts = retrieval_start_lines(&logs, org);
    assert_eq!(
        starts.len(),
        1,
        "one retrieval.start line for {org}; org lines:\n{}",
        lines_for(&logs, org)
    );
    assert!(
        starts[0].contains(probe),
        "a non-restrictive call logs its query text as before: {}",
        starts[0]
    );
    assert!(
        !starts[0].contains("withheld("),
        "a non-restrictive call is not withheld: {}",
        starts[0]
    );
}

#[tokio::test]
async fn the_withheld_hash_correlates_repeats_and_separates_different_queries() {
    let org = "org-keyed-probe";
    let repeated = "quarterly conveyor lubricant audit repeated";
    let other = "quarterly conveyor lubricant audit other query";
    let pipeline = pipeline().await;
    for query in [repeated, repeated, other] {
        let _ = pipeline
            .retrieve(request(org, query, Some("ephemeral"), Some(false)))
            .await;
    }
    let logs = captured_logs().contents();

    let starts = retrieval_start_lines(&logs, org);
    assert_eq!(
        starts.len(),
        3,
        "three retrieval.start lines for {org}; org lines:\n{}",
        lines_for(&logs, org)
    );
    let fields: Vec<(usize, String)> = starts.iter().map(|line| withheld_fields(line)).collect();

    assert_eq!(
        fields[0], fields[1],
        "the same query in the same process correlates"
    );
    assert_eq!(fields[0].0, repeated.len());
    assert_eq!(fields[2].0, other.len());
    assert_ne!(
        fields[0].1, fields[2].1,
        "different queries get different hashes"
    );

    // The hash is keyed. The plain BLAKE3 prefix of the query, which any reader
    // of the logs could recompute to test a guess, must not be what is logged.
    let plain = blake3::hash(repeated.as_bytes()).to_hex().to_string();
    assert_ne!(
        fields[0].1.as_str(),
        &plain[..16],
        "the logged hash is keyed, not the plain digest"
    );
}
