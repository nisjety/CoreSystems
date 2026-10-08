//! Z-08 contract: the `zdr` claim of a verified USER token is a floor under
//! every ZDR decision quarry-edge makes, and a SERVICE token's `zdr` claim is
//! never read.
//!
//! Why the service half matters as much as the user half. Auth Core stamps
//! `zdr` on every plane token. For a user it is the organisation's posture. For
//! a service principal it is the registry row's `retentionByAudience`, and the
//! two principals that call Quarry on behalf of a chat run (model-gateway for
//! web search, execution-core for the agent web tools) both hold
//! `retentionByAudience.quarry = "zdr"`: their token says `zdr: true` for every
//! org on every call. Reading that claim would force ZDR onto every web search
//! of every org, turn off the paid providers and bypass the caches. For those
//! callers the run's posture is the request body flag, set by the gateway from
//! the run's resolved posture (false on a waived run).
//!
//! What this file does NOT do, on purpose: stub the auth. Every request runs
//! the real `require_auth` (RS256 verification against a JWKS endpoint, the
//! deployed `aud` and `iss`), the real `require_service_route_scope`, and the
//! real route handlers behind `quarry_edge::routes::router`. The claim sets are
//! the golden captured from Auth Core's real mint (see
//! `fixtures/auth-core/quarry-token-claims.golden.json`, regenerated and
//! compared by `quarry-token-zdr-claims.golden.spec.ts`): the service claims
//! come from the checked-in principal policy, not from a value typed here. The
//! older `zdr_behavior.rs` runs on the dev bypass, whose stub identity carries
//! no `zdr` claim at all, so it cannot see this behaviour either way.
//!
//! The effects asserted are the ones a ZDR request must have:
//! * search/answer: the provider is called with `zdr = true` and no paid-provider
//!   permission, and no durable `SearchIssued` event is emitted;
//! * scrape / scrape stream / extract: the driver is handed a privacy policy with
//!   `zdr` active, and no durable page event is emitted;
//! * agent runs: a ZDR run cannot take a persistent profile;
//! * crawl / batch: the durable job path has no ZDR support, so it is refused
//!   rather than silently run as non-ZDR;
//! * audio: the posture is forwarded to the Model Plane.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::sync::mpsc;
use url::Url;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use quarry_core::event::EventType;
use quarry_core::output::DriverKind;
use quarry_core::QuarryResult;
use quarry_edge::state::AppState;
use quarry_runtime::answer::{AnswerPipeline, SimpleHttpMarkdownFetcher};
use quarry_runtime::driver::{Driver, FetchHints};
use quarry_runtime::fetch::FetchResponse;
use quarry_runtime::serp::{SearchOptions, SearchProvider, SearchResult};
use quarry_runtime::{usage_metrics, EventSink, UsageEvent, UsageMeter};

mod support;

const GOLDEN: &str = include_str!("fixtures/auth-core/quarry-token-claims.golden.json");
const GOLDEN_SHA256: &str = include_str!("fixtures/auth-core/quarry-token-claims.golden.sha256");

/// The deployed values: `AUTH_CORE_AUDIENCE` and `AUTH_CORE_ISSUER` of the
/// quarry-edge service in `apps/Ingestion Plane/docker-compose.yml`.
const DEPLOYED_AUDIENCE: &str = "quarry";
const DEPLOYED_ISSUER: &str = "http://localhost:3011/api/convex-auth";
const KID: &str = "z08-contract-kid";

// ── real-token fixture ──────────────────────────────────────────────────────

struct TokenFixture {
    private_pem: String,
}

fn fixture() -> &'static TokenFixture {
    static FIXTURE: OnceLock<TokenFixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        use base64::Engine;
        use rsa::pkcs8::{EncodePrivateKey, LineEnding};
        use rsa::traits::PublicKeyParts;

        let mut rng = rand::thread_rng();
        let private = rsa::RsaPrivateKey::new(&mut rng, 2048).expect("rsa keygen");
        let public = rsa::RsaPublicKey::from(&private);
        let private_pem = private
            .to_pkcs8_pem(LineEnding::LF)
            .expect("encode private pem")
            .to_string();
        let n = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(public.n().to_bytes_be());
        let e = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(public.e().to_bytes_be());
        let jwks = json!({
            "keys": [{ "kty": "RSA", "use": "sig", "alg": "RS256", "kid": KID, "n": n, "e": e }]
        });

        // The JWKS endpoint lives on its own runtime so it outlives the
        // per-test runtimes `#[tokio::test]` creates.
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(1)
                .enable_all()
                .build()
                .expect("jwks runtime");
            runtime.block_on(async move {
                let server = MockServer::start().await;
                Mock::given(method("GET"))
                    .and(path("/jwks"))
                    .respond_with(ResponseTemplate::new(200).set_body_json(jwks))
                    .mount(&server)
                    .await;
                tx.send(server.uri()).expect("publish jwks uri");
                std::future::pending::<()>().await;
            });
        });
        let jwks_uri = rx.recv().expect("jwks uri");

        // Exactly the verification configuration of the deployed edge, and no
        // dev bypass: a token that fails real verification must be a 401.
        std::env::remove_var("QUARRY_EDGE_AUTH_DEV_BYPASS");
        std::env::remove_var("AUTH_CORE_AUDIENCE_OPTIONAL");
        std::env::set_var("AUTH_CORE_JWKS_URL", format!("{jwks_uri}/jwks"));
        std::env::set_var("AUTH_CORE_AUDIENCE", DEPLOYED_AUDIENCE);
        std::env::set_var("AUTH_CORE_ISSUER", DEPLOYED_ISSUER);

        TokenFixture { private_pem }
    })
}

fn unix_now() -> i64 {
    i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock")
            .as_secs(),
    )
    .expect("unix time fits i64")
}

/// A token whose claims are the golden case `case`, signed RS256 with the
/// fixture key and stamped with fresh time claims by the golden's own rule
/// (`nbf = iat - 5`, `exp = iat + ttl_seconds`). `tweak` edits the claim map
/// before signing, to build the shapes a test needs (claim removed, flipped).
fn token(case: &str, tweak: impl FnOnce(&mut serde_json::Map<String, Value>)) -> String {
    use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};

    let golden: Value = serde_json::from_str(GOLDEN).expect("golden is JSON");
    let entry = &golden["cases"][case];
    assert!(entry.is_object(), "golden has no case `{case}`");
    let mut claims = entry["claims"].as_object().expect("claims object").clone();
    let ttl = entry["ttl_seconds"].as_i64().expect("ttl_seconds");
    let now = unix_now();
    claims.insert("iat".into(), json!(now));
    claims.insert("nbf".into(), json!(now - 5));
    claims.insert("exp".into(), json!(now + ttl));
    tweak(&mut claims);

    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some(KID.to_owned());
    let key = EncodingKey::from_rsa_pem(fixture().private_pem.as_bytes()).expect("signing key");
    encode(&header, &Value::Object(claims), &key).expect("sign token")
}

fn user_in_zdr_org() -> String {
    token("user_in_zdr_org", |_| {})
}

fn user_in_standard_org() -> String {
    token("user_in_standard_org", |_| {})
}

// ── recording doubles ───────────────────────────────────────────────────────

#[derive(Default)]
struct RecordingSearch {
    /// `(zdr, allow_paid_providers)` per provider call.
    calls: Mutex<Vec<(bool, bool)>>,
}

#[async_trait]
impl SearchProvider for RecordingSearch {
    async fn search(&self, _query: &str, opts: &SearchOptions) -> QuarryResult<Vec<SearchResult>> {
        self.calls
            .lock()
            .unwrap()
            .push((opts.zdr, opts.allow_paid_providers));
        Ok(vec![SearchResult {
            url: "https://page.test/a".into(),
            title: Some("A".into()),
            snippet: Some("a snippet".into()),
            rank: 1,
            provider: "recording".into(),
            ..Default::default()
        }])
    }

    fn name(&self) -> &str {
        "recording"
    }
}

/// Same provider surface, no results: lets the answer pipeline return before
/// any page fetch or model call while still having called the provider.
#[derive(Default)]
struct EmptySearch {
    calls: Mutex<Vec<(bool, bool)>>,
}

#[async_trait]
impl SearchProvider for EmptySearch {
    async fn search(&self, _query: &str, opts: &SearchOptions) -> QuarryResult<Vec<SearchResult>> {
        self.calls
            .lock()
            .unwrap()
            .push((opts.zdr, opts.allow_paid_providers));
        Ok(Vec::new())
    }

    fn name(&self) -> &str {
        "empty"
    }
}

#[derive(Default)]
struct RecordingMeter {
    events: Mutex<Vec<UsageEvent>>,
}

#[async_trait]
impl UsageMeter for RecordingMeter {
    async fn meter(&self, event: UsageEvent) {
        self.events.lock().unwrap().push(event);
    }
}

/// Static-kind driver that records the ZDR posture of the privacy policy it is
/// handed on each fetch. That policy is how `/v1/scrape`, `/v1/scrape/stream`
/// and `/v1/extract` carry the posture into the fetch layer.
#[derive(Default)]
struct RecordingDriver {
    fetch_zdr: Mutex<Vec<bool>>,
}

#[async_trait]
impl Driver for RecordingDriver {
    fn kind(&self) -> DriverKind {
        DriverKind::Static
    }

    async fn fetch(&self, url: &Url) -> QuarryResult<FetchResponse> {
        self.fetch_conditional(url, &FetchHints::default()).await
    }

    async fn fetch_conditional(
        &self,
        url: &Url,
        hints: &FetchHints,
    ) -> QuarryResult<FetchResponse> {
        self.fetch_zdr
            .lock()
            .unwrap()
            .push(hints.privacy.zdr.is_active());
        Ok(FetchResponse {
            status: 200,
            final_url: url.clone(),
            headers: vec![],
            // The URL is in the body so each URL has its own content
            // fingerprint (and so its own page-cache key).
            body: format!(
                "<html><head><title>T</title></head><body><p>plain page text for the contract test of {url}, long enough to read as content.</p></body></html>"
            )
            .into_bytes(),
            duration_ms: 1,
            served_by: DriverKind::Static,
        })
    }
}

// ── harness ─────────────────────────────────────────────────────────────────

struct Harness {
    addr: SocketAddr,
    client: reqwest::Client,
    search: Arc<RecordingSearch>,
    empty_search: Arc<EmptySearch>,
    usage: Arc<RecordingMeter>,
    driver: Arc<RecordingDriver>,
    /// Event types that reached the DURABLE channel (the mpsc the HTTP
    /// publisher drains). ZDR events go to live subscribers only.
    durable_events: Arc<Mutex<Vec<EventType>>>,
}

impl Harness {
    async fn start() -> Self {
        Self::start_with(|state| state).await
    }

    async fn start_with(configure: impl FnOnce(AppState) -> AppState) -> Self {
        fixture();

        let durable_events = Arc::new(Mutex::new(Vec::new()));
        let (tx, mut rx) = mpsc::channel::<quarry_core::event::Event>(1024);
        let sink_events = durable_events.clone();
        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                sink_events.lock().unwrap().push(event.event_type);
            }
        });

        let search = Arc::new(RecordingSearch::default());
        let empty_search = Arc::new(EmptySearch::default());
        let usage = Arc::new(RecordingMeter::default());
        let driver = Arc::new(RecordingDriver::default());

        let formats = quarry_runtime::ai_formats::AiFormatRunner::new(Arc::new(
            quarry_runtime::mp_client::ModelPlaneClient::new("http://127.0.0.1:9")
                .expect("model plane client"),
        ));
        let pipeline = AnswerPipeline::new(
            empty_search.clone(),
            Arc::new(SimpleHttpMarkdownFetcher::new()),
            formats,
        );

        let state = AppState {
            search: Some(search.clone()),
            usage: usage.clone(),
            answer_pipeline: Some(Arc::new(pipeline)),
            #[cfg(feature = "browser-agent")]
            agent_driver: support::governed_agent_driver(),
            ..support::base_state(driver.clone(), EventSink::new(tx))
        };
        let app = quarry_edge::routes::router(configure(state));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        Self {
            addr,
            client: reqwest::Client::new(),
            search,
            empty_search,
            usage,
            driver,
            durable_events,
        }
    }

    async fn post(&self, bearer: &str, route: &str, body: Value) -> reqwest::Response {
        self.client
            .post(format!("http://{}{route}", self.addr))
            .bearer_auth(bearer)
            .json(&body)
            .send()
            .await
            .expect("request reaches the edge")
    }

    /// Give the spawned drain task a moment to forward what the handler sent.
    async fn settled_events(&self) -> Vec<EventType> {
        tokio::time::sleep(Duration::from_millis(150)).await;
        self.durable_events.lock().unwrap().clone()
    }

    fn search_calls(&self) -> Vec<(bool, bool)> {
        self.search.calls.lock().unwrap().clone()
    }

    fn answer_search_calls(&self) -> Vec<(bool, bool)> {
        self.empty_search.calls.lock().unwrap().clone()
    }

    fn fetch_zdr(&self) -> Vec<bool> {
        self.driver.fetch_zdr.lock().unwrap().clone()
    }

    fn metered_zdr(&self, metric: &str) -> Vec<Option<bool>> {
        self.usage
            .events
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e.metric == metric)
            .map(|e| e.metadata.get("zdr").and_then(Value::as_bool))
            .collect()
    }
}

fn is_search_issued(events: &[EventType]) -> bool {
    events.iter().any(|e| *e == EventType::SearchIssued)
}

fn is_page_fetched(events: &[EventType]) -> bool {
    events.iter().any(|e| *e == EventType::PageFetched)
}

// ── the golden itself ───────────────────────────────────────────────────────

#[test]
fn the_golden_is_the_one_its_pin_names() {
    let digest = format!("{:x}", Sha256::digest(GOLDEN.as_bytes()));
    let pinned = GOLDEN_SHA256
        .split_whitespace()
        .next()
        .expect("pin has a digest");
    assert_eq!(
        digest, pinned,
        "quarry-token-claims.golden.json drifted from its .sha256 pin: regenerate both with \
         UPDATE_GOLDENS=1 from auth-core's quarry-token-zdr-claims.golden.spec.ts"
    );
}

#[test]
fn the_golden_still_says_what_the_floor_depends_on() {
    let golden: Value = serde_json::from_str(GOLDEN).unwrap();
    let zdr = |case: &str| golden["cases"][case]["claims"]["zdr"].as_bool();
    assert_eq!(zdr("user_in_zdr_org"), Some(true));
    assert_eq!(zdr("user_in_standard_org"), Some(false));
    // The trap: both chat-run callers carry zdr:true whatever the org's posture.
    assert_eq!(zdr("service_model_gateway"), Some(true));
    assert_eq!(zdr("service_execution_core"), Some(true));
    for case in ["service_model_gateway", "service_execution_core"] {
        assert_eq!(
            golden["cases"][case]["claims"]["principal_type"], "service",
            "{case}"
        );
    }
}

#[tokio::test]
async fn a_token_that_fails_real_verification_is_refused() {
    // Guards the harness itself: the dev bypass is off and verification is
    // real, so these tests cannot be passing on a stub identity.
    let harness = Harness::start().await;
    let response = harness
        .post("not-a-jwt", "/v1/search", json!({ "query": "x" }))
        .await;
    assert_eq!(response.status().as_u16(), 401);
    assert!(harness.search_calls().is_empty());
}

// ── /v1/search ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn a_user_token_from_a_zdr_org_runs_search_as_zdr_even_when_the_body_says_false() {
    let harness = Harness::start().await;
    let response = harness
        .post(
            &user_in_zdr_org(),
            "/v1/search",
            json!({ "query": "zdr floor", "zdr": false, "allow_paid_providers": true }),
        )
        .await;
    assert_eq!(response.status().as_u16(), 200);

    assert_eq!(
        harness.search_calls(),
        vec![(true, false)],
        "the provider must see zdr=true and no paid-provider permission"
    );
    assert!(
        !is_search_issued(&harness.settled_events().await),
        "a ZDR search must not write the durable SearchIssued event"
    );
    assert_eq!(
        harness.metered_zdr(usage_metrics::SEARCH_QUERY),
        vec![Some(true)]
    );
}

#[tokio::test]
async fn a_user_token_from_a_zdr_org_runs_search_as_zdr_when_the_body_omits_the_flag() {
    let harness = Harness::start().await;
    let response = harness
        .post(
            &user_in_zdr_org(),
            "/v1/search",
            json!({ "query": "zdr floor", "allow_paid_providers": true }),
        )
        .await;
    assert_eq!(response.status().as_u16(), 200);
    assert_eq!(harness.search_calls(), vec![(true, false)]);
}

#[tokio::test]
async fn service_tokens_whose_zdr_comes_from_the_registry_do_not_force_zdr_on_search() {
    // model-gateway and execution-core hold retentionByAudience quarry=zdr, so
    // their golden claims say zdr:true. They must still run a normal search
    // when the run's posture (the body flag) is false, paid providers included.
    for case in ["service_model_gateway", "service_execution_core"] {
        let harness = Harness::start().await;
        let response = harness
            .post(
                &token(case, |_| {}),
                "/v1/search",
                json!({ "query": "chat web search", "zdr": false, "allow_paid_providers": true }),
            )
            .await;
        assert_eq!(response.status().as_u16(), 200, "{case}");

        assert_eq!(
            harness.search_calls(),
            vec![(false, true)],
            "{case}: a service token's zdr claim must not lift the run to ZDR"
        );
        assert!(
            is_search_issued(&harness.settled_events().await),
            "{case}: a non-ZDR run keeps its durable SearchIssued event"
        );
        assert_eq!(
            harness.metered_zdr(usage_metrics::SEARCH_QUERY),
            vec![Some(false)],
            "{case}"
        );
    }
}

#[tokio::test]
async fn a_service_token_still_gets_zdr_when_the_run_body_asks_for_it() {
    // The body flag is how a waived/unwaived run's posture reaches Quarry; for
    // a service caller it is the whole decision, in both directions.
    let harness = Harness::start().await;
    let response = harness
        .post(
            &token("service_model_gateway", |_| {}),
            "/v1/search",
            json!({ "query": "zdr run", "zdr": true, "allow_paid_providers": true }),
        )
        .await;
    assert_eq!(response.status().as_u16(), 200);
    assert_eq!(harness.search_calls(), vec![(true, false)]);
}

#[tokio::test]
async fn a_user_token_from_a_standard_org_with_body_zdr_true_is_still_zdr() {
    // Monotonic: a caller can raise the posture, never lower it.
    let harness = Harness::start().await;
    let response = harness
        .post(
            &user_in_standard_org(),
            "/v1/search",
            json!({ "query": "temporary chat", "zdr": true, "allow_paid_providers": true }),
        )
        .await;
    assert_eq!(response.status().as_u16(), 200);
    assert_eq!(harness.search_calls(), vec![(true, false)]);
    assert!(!is_search_issued(&harness.settled_events().await));
}

#[tokio::test]
async fn a_user_token_from_a_standard_org_searches_as_before() {
    let harness = Harness::start().await;
    let response = harness
        .post(
            &user_in_standard_org(),
            "/v1/search",
            json!({ "query": "ordinary search", "allow_paid_providers": true }),
        )
        .await;
    assert_eq!(response.status().as_u16(), 200);
    assert_eq!(harness.search_calls(), vec![(false, true)]);
    assert!(is_search_issued(&harness.settled_events().await));
    assert_eq!(
        harness.metered_zdr(usage_metrics::SEARCH_QUERY),
        vec![Some(false)]
    );
}

#[tokio::test]
async fn a_user_token_without_a_zdr_claim_is_accepted_and_adds_no_floor() {
    // A token minted before Auth Core stamped the claim has nothing to lift
    // the posture from. Missing is `None`, not an auth failure, and not ZDR.
    let harness = Harness::start().await;
    let bearer = token("user_in_zdr_org", |claims| {
        claims.remove("zdr");
    });
    let response = harness
        .post(
            &bearer,
            "/v1/search",
            json!({ "query": "legacy token", "allow_paid_providers": true }),
        )
        .await;
    assert_eq!(response.status().as_u16(), 200);
    assert_eq!(harness.search_calls(), vec![(false, true)]);
}

// ── /v1/answer and /v1/answer/stream ────────────────────────────────────────

#[tokio::test]
async fn answer_runs_as_zdr_for_a_zdr_org_user_and_as_before_otherwise() {
    let harness = Harness::start().await;
    let zdr_org = harness
        .post(
            &user_in_zdr_org(),
            "/v1/answer",
            json!({ "query": "what is zdr", "zdr": false }),
        )
        .await;
    assert_eq!(zdr_org.status().as_u16(), 200);
    let standard = harness
        .post(
            &user_in_standard_org(),
            "/v1/answer",
            json!({ "query": "what is zdr" }),
        )
        .await;
    assert_eq!(standard.status().as_u16(), 200);
    let raised = harness
        .post(
            &user_in_standard_org(),
            "/v1/answer",
            json!({ "query": "what is zdr", "zdr": true }),
        )
        .await;
    assert_eq!(raised.status().as_u16(), 200);

    assert_eq!(
        harness.answer_search_calls(),
        vec![(true, false), (false, false), (true, false)]
    );
}

#[tokio::test]
async fn answer_stream_runs_as_zdr_for_a_zdr_org_user_and_as_before_otherwise() {
    let harness = Harness::start().await;
    for (bearer, body) in [
        (
            user_in_zdr_org(),
            json!({ "query": "what is zdr", "zdr": false }),
        ),
        (user_in_standard_org(), json!({ "query": "what is zdr" })),
        (
            user_in_standard_org(),
            json!({ "query": "what is zdr", "zdr": true }),
        ),
    ] {
        let response = harness.post(&bearer, "/v1/answer/stream", body).await;
        assert_eq!(response.status().as_u16(), 200);
        // Drain the SSE body so the stream (and the provider call) completes.
        let _ = response.text().await.expect("sse body");
    }
    assert_eq!(
        harness.answer_search_calls(),
        vec![(true, false), (false, false), (true, false)]
    );
}

// ── /v1/scrape, /v1/scrape/stream, /v1/extract ──────────────────────────────

#[tokio::test]
async fn scrape_runs_as_zdr_for_a_zdr_org_user_and_not_for_a_registry_zdr_service() {
    let harness = Harness::start().await;
    let page = json!({ "url": "https://page.test/a", "zdr": false });

    let zdr_org = harness
        .post(&user_in_zdr_org(), "/v1/scrape", page.clone())
        .await;
    assert_eq!(zdr_org.status().as_u16(), 200);
    assert_eq!(harness.fetch_zdr(), vec![true]);
    assert_eq!(
        harness.metered_zdr(usage_metrics::SCRAPE_PAGE),
        vec![Some(true)]
    );
    assert!(
        !is_page_fetched(&harness.settled_events().await),
        "a ZDR scrape keeps page events off the durable channel"
    );

    let service = harness
        .post(&token("service_model_gateway", |_| {}), "/v1/scrape", page)
        .await;
    assert_eq!(service.status().as_u16(), 200);
    assert_eq!(
        harness.fetch_zdr(),
        vec![true, false],
        "a service token's zdr claim must not lift the scrape to ZDR"
    );
    assert_eq!(
        harness.metered_zdr(usage_metrics::SCRAPE_PAGE),
        vec![Some(true), Some(false)]
    );
    assert!(is_page_fetched(&harness.settled_events().await));
}

#[tokio::test]
async fn scrape_stream_runs_as_zdr_for_a_zdr_org_user_and_not_for_a_registry_zdr_service() {
    let harness = Harness::start().await;
    let page = json!({ "url": "https://page.test/a", "zdr": false });

    let zdr_org = harness
        .post(&user_in_zdr_org(), "/v1/scrape/stream", page.clone())
        .await;
    assert_eq!(zdr_org.status().as_u16(), 200);
    let _ = zdr_org.text().await.expect("sse body");
    assert_eq!(harness.fetch_zdr(), vec![true]);
    assert!(!is_page_fetched(&harness.settled_events().await));

    let service = harness
        .post(
            &token("service_model_gateway", |_| {}),
            "/v1/scrape/stream",
            page,
        )
        .await;
    // `/v1/scrape/stream` is not a service route, so the route-scope middleware
    // answers 403 before the handler: there is no service posture to get wrong.
    assert_eq!(service.status().as_u16(), 403);
    assert_eq!(harness.fetch_zdr(), vec![true]);

    let standard = harness
        .post(
            &user_in_standard_org(),
            "/v1/scrape/stream",
            json!({ "url": "https://page.test/a" }),
        )
        .await;
    assert_eq!(standard.status().as_u16(), 200);
    let _ = standard.text().await.expect("sse body");
    assert_eq!(harness.fetch_zdr(), vec![true, false]);
    assert!(is_page_fetched(&harness.settled_events().await));
}

#[tokio::test]
async fn extract_runs_as_zdr_for_a_zdr_org_user_and_not_for_a_registry_zdr_service() {
    let harness = Harness::start().await;
    let body = json!({ "urls": ["https://page.test/a"], "zdr": false });

    let zdr_org = harness
        .post(&user_in_zdr_org(), "/v1/extract", body.clone())
        .await;
    assert_eq!(zdr_org.status().as_u16(), 200);
    assert_eq!(harness.fetch_zdr(), vec![true]);

    let service = harness
        .post(
            &token("service_execution_core", |_| {}),
            "/v1/extract",
            body.clone(),
        )
        .await;
    assert_eq!(service.status().as_u16(), 200);
    assert_eq!(
        harness.fetch_zdr(),
        vec![true, false],
        "a service token's zdr claim must not lift extract to ZDR"
    );

    let raised = harness
        .post(
            &user_in_standard_org(),
            "/v1/extract",
            json!({ "urls": ["https://page.test/a"], "zdr": true }),
        )
        .await;
    assert_eq!(raised.status().as_u16(), 200);
    assert_eq!(harness.fetch_zdr(), vec![true, false, true]);
}

// ── /v1/agent/runs ──────────────────────────────────────────────────────────

#[cfg(feature = "browser-agent")]
#[tokio::test]
async fn a_zdr_org_users_agent_run_cannot_take_a_persistent_profile() {
    let harness = Harness::start().await;
    let response = harness
        .post(
            &user_in_zdr_org(),
            "/v1/agent/runs",
            json!({ "zdr": false, "persist_profile": true }),
        )
        .await;
    assert_eq!(response.status().as_u16(), 400);
    let body: Value = response.json().await.unwrap();
    let message = body["error"]["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("zdr_persistent_profile_forbidden"),
        "unexpected error body: {body}"
    );
}

#[cfg(feature = "browser-agent")]
#[tokio::test]
async fn a_registry_zdr_service_agent_run_is_not_refused_by_the_zdr_guard() {
    // execution-core's agent runs: the token says zdr:true by registry, the run's
    // own posture is the body flag. The ZDR guard must not fire.
    let harness = Harness::start().await;
    let response = harness
        .post(
            &token("service_execution_core", |_| {}),
            "/v1/agent/runs",
            json!({ "zdr": false, "persist_profile": true }),
        )
        .await;
    if response.status().as_u16() == 400 {
        let body: Value = response.json().await.unwrap();
        let message = body["error"]["message"].as_str().unwrap_or_default();
        assert!(
            !message.contains("zdr_persistent_profile_forbidden"),
            "a service token's zdr claim must not make its run ZDR: {body}"
        );
    }
}

#[cfg(feature = "browser-agent")]
#[tokio::test]
async fn a_standard_org_users_agent_run_is_not_refused_by_the_zdr_guard() {
    let harness = Harness::start().await;
    let response = harness
        .post(
            &user_in_standard_org(),
            "/v1/agent/runs",
            json!({ "zdr": false, "persist_profile": true }),
        )
        .await;
    if response.status().as_u16() == 400 {
        let body: Value = response.json().await.unwrap();
        let message = body["error"]["message"].as_str().unwrap_or_default();
        assert!(
            !message.contains("zdr_persistent_profile_forbidden"),
            "{body}"
        );
    }
}

// ── /v1/crawl and /v1/batch (durable job path) ──────────────────────────────

async fn control_plane_double() -> MockServer {
    let control = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/jobs"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "data": { "id": "job_z08" } })),
        )
        .mount(&control)
        .await;
    control
}

#[tokio::test]
async fn durable_crawl_and_batch_are_refused_for_a_zdr_org_user_and_open_otherwise() {
    // The orchestrator never forwards a ZDR posture to run_page, so a durable
    // job cannot honour ZDR. Refusing is the fail-closed answer; running it as
    // non-ZDR would be a silent retention of the org's URLs and pages.
    let control = control_plane_double().await;
    let control_url = control.uri();
    let harness = Harness::start_with(move |state| AppState {
        control_base_url: control_url,
        ..state
    })
    .await;

    for (route, body) in [
        (
            "/v1/crawl",
            json!({ "url": "https://page.test/", "max_pages": 1 }),
        ),
        ("/v1/batch", json!({ "urls": ["https://page.test/a"] })),
    ] {
        let refused = harness.post(&user_in_zdr_org(), route, body.clone()).await;
        assert_eq!(refused.status().as_u16(), 403, "{route}");
        let payload: Value = refused.json().await.unwrap();
        assert!(
            payload["error"]["message"]
                .as_str()
                .unwrap_or_default()
                .contains("ZDR"),
            "{route}: the refusal must say why: {payload}"
        );

        // Asking for ZDR explicitly is honoured the same way, not ignored.
        let explicit = {
            let mut with_flag = body.clone();
            with_flag["zdr"] = json!(true);
            harness
                .post(&user_in_standard_org(), route, with_flag)
                .await
        };
        assert_eq!(
            explicit.status().as_u16(),
            403,
            "{route} with body zdr=true"
        );
    }
    assert!(
        control.received_requests().await.unwrap().is_empty(),
        "a refused durable job must never reach the control plane"
    );

    for (route, body) in [
        (
            "/v1/crawl",
            json!({ "url": "https://page.test/", "max_pages": 1 }),
        ),
        ("/v1/batch", json!({ "urls": ["https://page.test/a"] })),
    ] {
        let accepted = harness.post(&user_in_standard_org(), route, body).await;
        assert_eq!(accepted.status().as_u16(), 200, "{route}");
    }
    assert_eq!(control.received_requests().await.unwrap().len(), 2);
}

// ── /v1/audio ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn audio_forwards_the_floored_posture_to_the_model_plane() {
    let model_plane = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/ai/speech"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "ok": true })))
        .mount(&model_plane)
        .await;
    let model_plane_url = model_plane.uri();
    let harness = Harness::start_with(move |state| AppState {
        model_plane_url: Some(model_plane_url),
        model_plane_token: Some("model-plane-test-token".into()),
        ..state
    })
    .await;

    let zdr_org = harness
        .post(
            &user_in_zdr_org(),
            "/v1/audio",
            json!({ "input": "hello", "zdr": false }),
        )
        .await;
    assert_eq!(zdr_org.status().as_u16(), 200);
    let standard = harness
        .post(
            &user_in_standard_org(),
            "/v1/audio",
            json!({ "input": "hello" }),
        )
        .await;
    assert_eq!(standard.status().as_u16(), 200);

    let forwarded: Vec<Option<bool>> = model_plane
        .received_requests()
        .await
        .unwrap()
        .iter()
        .map(|r| {
            serde_json::from_slice::<Value>(&r.body)
                .unwrap()
                .get("zdr")
                .and_then(Value::as_bool)
        })
        .collect();
    assert_eq!(
        forwarded,
        vec![Some(true), None],
        "a ZDR org's audio request must reach the Model Plane flagged zdr; an ordinary one is forwarded untouched"
    );
}

#[tokio::test]
async fn audio_transcription_forwards_the_floored_posture_to_the_model_plane() {
    // The STT branch carries the user's recording to the Model Plane, so it is
    // the audio half that matters most; it has its own request shape
    // (`audio_url`) and its own line in the handler.
    let model_plane = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/ai/transcribe"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "text": "hello" })))
        .mount(&model_plane)
        .await;
    let model_plane_url = model_plane.uri();
    let harness = Harness::start_with(move |state| AppState {
        model_plane_url: Some(model_plane_url),
        model_plane_token: Some("model-plane-test-token".into()),
        ..state
    })
    .await;

    let zdr_org = harness
        .post(
            &user_in_zdr_org(),
            "/v1/audio",
            json!({ "audio_url": "https://audio.test/a.mp3", "zdr": false }),
        )
        .await;
    assert_eq!(zdr_org.status().as_u16(), 200);
    let standard = harness
        .post(
            &user_in_standard_org(),
            "/v1/audio",
            json!({ "audio_url": "https://audio.test/a.mp3" }),
        )
        .await;
    assert_eq!(standard.status().as_u16(), 200);

    let forwarded: Vec<Option<bool>> = model_plane
        .received_requests()
        .await
        .unwrap()
        .iter()
        .map(|r| {
            serde_json::from_slice::<Value>(&r.body)
                .unwrap()
                .get("zdr")
                .and_then(Value::as_bool)
        })
        .collect();
    assert_eq!(
        forwarded,
        vec![Some(true), None],
        "a ZDR org's transcription request must reach the Model Plane flagged zdr; an ordinary one is forwarded untouched"
    );
}

// ── caches (needs a real Redis) ─────────────────────────────────────────────

/// A cache that is not actually there cannot be bypassed, so the cache half of
/// the contract runs against a real Redis (`REDIS_URL`, default localhost).
/// Without one it skips on a developer machine, but never under
/// `REQUIRE_CONTRACT`: a contract that quietly skips is not a contract. `CI` is
/// deliberately NOT a trigger: GitHub Actions sets it on every job, and
/// `quarry-v2-ci.yml` runs `cargo test --workspace` without a Redis service, so
/// keying on it turned that job red. A job that provides a Redis opts in with
/// `REQUIRE_CONTRACT=1`, as the other contract tests of this repo do.
async fn redis_connection() -> Option<redis::aio::ConnectionManager> {
    let url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());
    let connected = async {
        let client = redis::Client::open(url).ok()?;
        tokio::time::timeout(Duration::from_secs(2), client.get_connection_manager())
            .await
            .ok()?
            .ok()
    }
    .await;
    if connected.is_none() {
        assert!(
            std::env::var_os("REQUIRE_CONTRACT").is_none(),
            "REQUIRE_CONTRACT is set but REDIS_URL is unreachable: the cache half of the \
             ZDR floor contract cannot be skipped"
        );
        eprintln!("skip: REDIS_URL unreachable (the cache half of the Z-08 contract needs it)");
    }
    connected
}

fn golden_org(case: &str) -> String {
    let golden: Value = serde_json::from_str(GOLDEN).expect("golden is JSON");
    golden["cases"][case]["claims"]["org_id"]
        .as_str()
        .expect("golden org_id")
        .to_owned()
}

async fn delete_keys(conn: &mut redis::aio::ConnectionManager, pattern: &str) {
    use redis::AsyncCommands;
    let keys: Vec<String> = conn.keys(pattern).await.expect("redis KEYS");
    if !keys.is_empty() {
        let _: i64 = conn.del(&keys).await.expect("redis DEL");
    }
}

#[tokio::test]
async fn a_zdr_org_user_never_reads_or_writes_a_cache_and_a_registry_zdr_service_still_does() {
    use redis::AsyncCommands;

    let Some(mut conn) = redis_connection().await else {
        return;
    };
    let zdr_org = golden_org("user_in_zdr_org");
    let standard_org = golden_org("user_in_standard_org");
    let tag = format!("z08-{}", unix_now());
    delete_keys(&mut conn, &format!("quarry:search:{zdr_org}:*")).await;

    let harness = Harness::start_with({
        let conn = conn.clone();
        move |state| AppState {
            redis: Some(conn.clone()),
            cache: Some(quarry_edge::cache::PageCache::new(
                conn,
                Duration::from_secs(300),
            )),
            ..state
        }
    })
    .await;
    let zdr_user = user_in_zdr_org();
    let standard_user = user_in_standard_org();
    let gateway = token("service_model_gateway", |_| {});
    // The same service principal calling for the ZDR org: the shape of a waived
    // run. The body says false, so the run is not ZDR, whatever the claim says.
    let gateway_for_zdr_org = token("service_model_gateway", |claims| {
        claims.insert("org_id".into(), json!(zdr_org));
    });

    // ── /v1/search: the org-keyed result cache ──────────────────────────────
    // The cache key carries neither the ZDR flag nor the paid-provider flag, so
    // an entry written by one request in an org is readable by the next one.
    let search = |query: String, zdr: bool| json!({ "query": query, "zdr": zdr, "allow_paid_providers": true });

    // (1) The harness cache is live: a standard org's second search is a hit.
    for _ in 0..2 {
        let response = harness
            .post(
                &standard_user,
                "/v1/search",
                search(format!("standard {tag}"), false),
            )
            .await;
        assert_eq!(response.status().as_u16(), 200);
    }
    assert_eq!(
        harness.search_calls(),
        vec![(false, true)],
        "the harness cache must be live: the second identical search is a hit"
    );

    // (2) A ZDR org's user is never served from the cache and never writes to it.
    for _ in 0..2 {
        let response = harness
            .post(
                &zdr_user,
                "/v1/search",
                search(format!("zdr org {tag}"), false),
            )
            .await;
        assert_eq!(response.status().as_u16(), 200);
    }
    assert_eq!(
        harness.search_calls(),
        vec![(false, true), (true, false), (true, false)],
        "a ZDR org's user must go to the provider every time"
    );
    let zdr_search_keys: Vec<String> = conn
        .keys(format!("quarry:search:{zdr_org}:*"))
        .await
        .expect("redis KEYS");
    assert!(
        zdr_search_keys.is_empty(),
        "a ZDR search must leave nothing in the cache: {zdr_search_keys:?}"
    );

    // (3) A service call for the same org with the body false is an ordinary
    // search: the token's registry claim does not switch its cache off.
    let waived = format!("waived run {tag}");
    for _ in 0..2 {
        let response = harness
            .post(
                &gateway_for_zdr_org,
                "/v1/search",
                search(waived.clone(), false),
            )
            .await;
        assert_eq!(response.status().as_u16(), 200);
    }
    assert_eq!(
        harness.search_calls(),
        vec![(false, true), (true, false), (true, false), (false, true)],
        "a service token's zdr claim must not switch the cache off for its run"
    );
    let waived_keys: Vec<String> = conn
        .keys(format!("quarry:search:{zdr_org}:*"))
        .await
        .expect("redis KEYS");
    assert_eq!(
        waived_keys.len(),
        1,
        "the waived run is cached: {waived_keys:?}"
    );

    // (4) ...and a ZDR user asking the same question must not be served that
    // entry (it was fetched with paid providers allowed).
    let response = harness
        .post(&zdr_user, "/v1/search", search(waived, false))
        .await;
    assert_eq!(response.status().as_u16(), 200);
    assert_eq!(
        harness.search_calls().last(),
        Some(&(true, false)),
        "a ZDR user must not read the entry a non-ZDR run left in the cache"
    );
    delete_keys(&mut conn, &format!("quarry:search:{zdr_org}:*")).await;
    delete_keys(&mut conn, &format!("quarry:search:{standard_org}:*")).await;

    // ── /v1/answer/stream: the cached value is the answer text itself ───────
    let question = format!("what is cached {tag}");
    for org in [&zdr_org, &standard_org] {
        let key = quarry_edge::cache::SearchCache::key(org, &question, "answer:stream:v1");
        quarry_edge::cache::SearchCache::new(conn.clone())
            .put_with_ttl(&key, &"PLANTED ANSWER".to_string(), 300)
            .await
            .expect("plant an answer");
    }
    let from_zdr_org = harness
        .post(&zdr_user, "/v1/answer/stream", json!({ "query": question }))
        .await
        .text()
        .await
        .expect("sse body");
    assert!(
        !from_zdr_org.contains("PLANTED ANSWER"),
        "a ZDR org's answer must not be served from the answer cache: {from_zdr_org}"
    );
    let from_standard_org = harness
        .post(
            &standard_user,
            "/v1/answer/stream",
            json!({ "query": question }),
        )
        .await
        .text()
        .await
        .expect("sse body");
    assert!(
        from_standard_org.contains("PLANTED ANSWER") && from_standard_org.contains("\"cached\":true"),
        "the planted entry must be served to a standard org (the cache is live): {from_standard_org}"
    );
    // The ZDR request ran the pipeline (one provider call), the cached one did not.
    assert_eq!(harness.answer_search_calls(), vec![(true, false)]);
    let key = quarry_edge::cache::SearchCache::key(&zdr_org, &question, "answer:stream:v1");
    let _: i64 = conn.del(&key).await.expect("remove planted answer");
    let key = quarry_edge::cache::SearchCache::key(&standard_org, &question, "answer:stream:v1");
    let _: i64 = conn.del(&key).await.expect("remove planted answer");

    // ── /v1/scrape: the page cache, keyed by content fingerprint ────────────
    let cached_url = format!("https://page.test/{tag}/cached");
    let zdr_url = format!("https://page.test/{tag}/zdr");
    let scrape = |url: &str, prev: Option<&str>| {
        let mut body = json!({ "url": url, "zdr": false, "cache": { "mode": "read_write" } });
        if let Some(prev) = prev {
            body["prev_fingerprint"] = json!(prev);
        }
        body
    };
    let first: Value = harness
        .post(&standard_user, "/v1/scrape", scrape(&cached_url, None))
        .await
        .json()
        .await
        .expect("scrape envelope");
    let cached_fingerprint = first["data"]["fingerprint"]
        .as_str()
        .expect("fingerprint")
        .to_owned();
    assert_eq!(harness.fetch_zdr(), vec![false]);

    let hit = harness
        .post(
            &standard_user,
            "/v1/scrape",
            scrape(&cached_url, Some(&cached_fingerprint)),
        )
        .await;
    assert_eq!(hit.status().as_u16(), 200);
    assert_eq!(
        harness.fetch_zdr(),
        vec![false],
        "the harness cache must be live: a known fingerprint is served without a fetch"
    );

    let bypassed = harness
        .post(
            &zdr_user,
            "/v1/scrape",
            scrape(&cached_url, Some(&cached_fingerprint)),
        )
        .await;
    assert_eq!(bypassed.status().as_u16(), 200);
    assert_eq!(
        harness.fetch_zdr(),
        vec![false, true],
        "a ZDR org's user must not be served a cached page"
    );

    let service_hit = harness
        .post(
            &gateway,
            "/v1/scrape",
            scrape(&cached_url, Some(&cached_fingerprint)),
        )
        .await;
    assert_eq!(service_hit.status().as_u16(), 200);
    assert_eq!(
        harness.fetch_zdr(),
        vec![false, true],
        "a service token's zdr claim must not switch the page cache off for its run"
    );

    let zdr_page: Value = harness
        .post(&zdr_user, "/v1/scrape", scrape(&zdr_url, None))
        .await
        .json()
        .await
        .expect("scrape envelope");
    let zdr_fingerprint = zdr_page["data"]["fingerprint"]
        .as_str()
        .expect("fingerprint")
        .to_owned();
    let written: bool = conn
        .exists(format!("quarry:page:{zdr_fingerprint}"))
        .await
        .expect("redis EXISTS");
    assert!(!written, "a ZDR scrape must not write the page cache");

    let _: i64 = conn
        .del(format!("quarry:page:{cached_fingerprint}"))
        .await
        .expect("cleanup");
}
