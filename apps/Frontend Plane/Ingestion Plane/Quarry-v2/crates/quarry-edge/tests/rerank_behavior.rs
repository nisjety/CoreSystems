//! `POST /v1/search` with the semantic rerank wired to the Data Plane (`rerank-v1`, QB-option B).
//!
//! What this file does NOT do, on purpose: stub the auth or the rerank policy. Every request runs
//! the real `require_auth` (RS256 verification against a JWKS endpoint, the deployed `aud` and
//! `iss`), the real search route, and the real `RerankingSearchProvider` over the real
//! `DataPlaneSearchReranker`. The Data Plane is a mock that answers with the vendored `rerank-v1`
//! golden (its request id echoed, as the route echoes it), and Auth Core is a mock that mints
//! through the real token provider. The claim sets are the golden captured from Auth Core's real
//! mint, so a service principal's `zdr` claim is the one a registry row really produces.
//!
//! What it proves: a service token's `zdr` claim is never read (a body that says `false` still
//! reranks), a ZDR run makes no Data Plane call, a service caller must state its posture, scores
//! reach the wire only for the kind a caller asked for, a non-tenant org is never sent, and shadow
//! mode keeps the order.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use async_trait::async_trait;
use reqwest::StatusCode;
use serde_json::{json, Value};
use tokio::sync::mpsc;
use url::Url;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

use quarry_core::event::Event;
use quarry_core::output::DriverKind;
use quarry_core::QuarryResult;
use quarry_edge::state::AppState;
use quarry_runtime::driver::{Driver, FetchHints};
use quarry_runtime::fetch::FetchResponse;
use quarry_runtime::serp::{SearchOptions, SearchProvider, SearchResult};
use quarry_runtime::service_tokens::AuthCoreServiceTokenProvider;
use quarry_runtime::{DataPlaneSearchReranker, EventSink, RerankingSearchProvider};

mod support;

const GOLDEN_CLAIMS: &str = include_str!("fixtures/auth-core/quarry-token-claims.golden.json");
const OK_GOLDEN: &str =
    include_str!("../../quarry-runtime/tests/fixtures/dp-contracts/rerank-v1/ok.golden.json");
const KID: &str = "rerank-behavior-kid";
/// The deployed values: `AUTH_CORE_AUDIENCE` and `AUTH_CORE_ISSUER` of the quarry-edge service.
const DEPLOYED_AUDIENCE: &str = "quarry";
const DEPLOYED_ISSUER: &str = "http://localhost:3011/api/convex-auth";
const MINT_ROUTE: &str = "/api/data-plane/internal-token";
const RERANK_ROUTE: &str = "/v1/rerank";
const MINTED: &str = "minted.service.token";

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

        // The JWKS endpoint lives on its own runtime so it outlives the per-test runtimes.
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

        // The verification configuration of the deployed edge, with no dev bypass.
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

/// A token whose claims are the golden case `case`, signed RS256 with the fixture key and
/// stamped with fresh time claims. `tweak` edits the claim map before signing.
fn token(case: &str, tweak: impl FnOnce(&mut serde_json::Map<String, Value>)) -> String {
    use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};

    let golden: Value = serde_json::from_str(GOLDEN_CLAIMS).expect("golden is JSON");
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

/// A service token exactly as Auth Core mints it for model-gateway: `zdr` is `true` because the
/// registry row says so, for every org.
fn model_gateway_service() -> String {
    token("service_model_gateway", |_| {})
}

// ── doubles ─────────────────────────────────────────────────────────────────

/// The search under the rerank: four public candidates, and a record of each call's ZDR posture.
#[derive(Default)]
struct RecordingSearch {
    zdr_calls: Mutex<Vec<bool>>,
}

#[async_trait]
impl SearchProvider for RecordingSearch {
    async fn search(&self, _query: &str, opts: &SearchOptions) -> QuarryResult<Vec<SearchResult>> {
        self.zdr_calls.lock().unwrap().push(opts.zdr);
        Ok((0..4)
            .map(|i| SearchResult {
                url: format!("https://page.test/{i}"),
                title: Some(format!("Page {i}")),
                snippet: Some(format!("A public snippet number {i}.")),
                rank: i + 1,
                provider: "recording".into(),
                ..Default::default()
            })
            .collect())
    }

    fn name(&self) -> &str {
        "recording"
    }
}

/// Answers `POST /v1/rerank` with the golden body, its request id echoed.
struct GoldenAnswer;

impl Respond for GoldenAnswer {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let sent: Value = serde_json::from_slice(&request.body).expect("the request is JSON");
        let mut body: Value = serde_json::from_str(OK_GOLDEN).expect("the golden is JSON");
        body["request_id"] = sent["request_id"].clone();
        ResponseTemplate::new(200).set_body_json(body)
    }
}

/// Static-kind driver: the search route never fetches a page here.
struct NoFetch;

#[async_trait]
impl Driver for NoFetch {
    fn kind(&self) -> DriverKind {
        DriverKind::Static
    }

    async fn fetch(&self, url: &Url) -> QuarryResult<FetchResponse> {
        self.fetch_conditional(url, &FetchHints::default()).await
    }

    async fn fetch_conditional(
        &self,
        url: &Url,
        _hints: &FetchHints,
    ) -> QuarryResult<FetchResponse> {
        Ok(FetchResponse {
            status: 200,
            final_url: url.clone(),
            headers: vec![],
            body: format!("<html><body><p>{url}</p></body></html>").into_bytes(),
            duration_ms: 1,
            served_by: DriverKind::Static,
            captures: None,
        })
    }
}

// ── harness ─────────────────────────────────────────────────────────────────

struct Harness {
    addr: SocketAddr,
    client: reqwest::Client,
    inner: Arc<RecordingSearch>,
    data_plane: MockServer,
    auth: MockServer,
}

impl Harness {
    /// The edge with the rerank wired to a Data Plane that must receive exactly `dp_calls`
    /// calls. `shadow` selects shadow mode.
    async fn start(dp_calls: u64, shadow: bool) -> Self {
        fixture();

        let auth = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(MINT_ROUTE))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "token": MINTED,
                "expiresInSeconds": 300
            })))
            .mount(&auth)
            .await;

        let data_plane = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(RERANK_ROUTE))
            .respond_with(GoldenAnswer)
            .expect(dp_calls)
            .mount(&data_plane)
            .await;

        let tokens = Arc::new(
            AuthCoreServiceTokenProvider::new(auth.uri(), "credential-long-enough")
                .expect("a long enough credential"),
        );
        let reranker =
            DataPlaneSearchReranker::new(data_plane.uri(), tokens).expect("the reranker is built");
        let inner = Arc::new(RecordingSearch::default());
        let rerank = RerankingSearchProvider::new(inner.clone(), Arc::new(reranker), 3)
            .with_deadline(Duration::from_secs(2))
            .with_shadow(shadow);

        let (tx, mut rx) = mpsc::channel::<Event>(1024);
        tokio::spawn(async move { while rx.recv().await.is_some() {} });
        let driver = Arc::new(NoFetch);
        let state = AppState {
            search: Some(Arc::new(rerank)),
            ..support::base_state(driver, EventSink::new(tx))
        };
        let app = quarry_edge::routes::router(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        Self {
            addr,
            client: reqwest::Client::new(),
            inner,
            data_plane,
            auth,
        }
    }

    async fn search(&self, bearer: &str, body: Value) -> (StatusCode, Value) {
        let response = self
            .client
            .post(format!("http://{}/v1/search", self.addr))
            .bearer_auth(bearer)
            .json(&body)
            .send()
            .await
            .expect("request reaches the edge");
        let status = response.status();
        let json = response.json::<Value>().await.unwrap_or(Value::Null);
        (status, json)
    }

    fn zdr_calls(&self) -> Vec<bool> {
        self.inner.zdr_calls.lock().unwrap().clone()
    }

    async fn data_plane_calls(&self) -> usize {
        self.data_plane
            .received_requests()
            .await
            .expect("request recording is on")
            .len()
    }

    async fn mint_count(&self) -> usize {
        self.auth
            .received_requests()
            .await
            .expect("request recording is on")
            .iter()
            .filter(|request| request.url.path() == MINT_ROUTE)
            .count()
    }
}

// ── the rules, end to end ───────────────────────────────────────────────────

#[tokio::test]
async fn a_user_token_from_a_zdr_org_makes_no_data_plane_call() {
    let h = Harness::start(0, false).await;
    let (status, body) = h
        .search(&user_in_zdr_org(), json!({ "query": "solar panels" }))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["rerank"]["status"], "skipped");
    assert_eq!(body["rerank"]["reason"], "zdr");
    assert_eq!(h.zdr_calls(), vec![true], "the run is ZDR");
    assert_eq!(h.mint_count().await, 0, "no token is minted for a ZDR run");
}

#[tokio::test]
async fn a_service_token_whose_zdr_claim_is_true_still_reranks_when_the_body_says_false() {
    // The trap: model-gateway's token says `zdr: true` for every org. Reading that claim would
    // silence the rerank for every web search. The run's posture is the body flag.
    let h = Harness::start(1, false).await;
    let (status, body) = h
        .search(
            &model_gateway_service(),
            json!({ "query": "solar panels", "zdr": false }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["rerank"]["status"], "applied", "{body}");
    assert_eq!(body["rerank"]["profile"], "web-v1");
    assert_eq!(h.zdr_calls(), vec![false]);
    assert_eq!(h.data_plane_calls().await, 1);
}

#[tokio::test]
async fn a_service_caller_must_state_its_posture_and_is_refused_without_it() {
    let h = Harness::start(0, false).await;
    let (status, body) = h
        .search(&model_gateway_service(), json!({ "query": "solar panels" }))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "ZDR_REQUIRED");
    assert!(h.zdr_calls().is_empty(), "the search is never run");
    assert_eq!(h.data_plane_calls().await, 0);
}

#[tokio::test]
async fn a_score_reaches_the_wire_only_for_a_caller_that_asks_for_its_kind() {
    let h = Harness::start(2, false).await;
    let token = user_in_standard_org();

    let (status, plain) = h.search(&token, json!({ "query": "solar panels" })).await;
    assert_eq!(status, StatusCode::OK, "{plain}");
    assert_eq!(plain["rerank"]["status"], "applied");
    assert!(plain.get("score_kind").is_none(), "{plain}");
    for result in plain["results"].as_array().unwrap() {
        assert!(
            result.get("score").is_none(),
            "the default is the order alone: {result}"
        );
    }

    let (status, asked) = h
        .search(
            &token,
            json!({ "query": "solar panels", "score_kinds": ["cross_encoder.v1"] }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{asked}");
    assert_eq!(asked["score_kind"], "cross_encoder.v1");
    let first = &asked["results"][0];
    assert_eq!(first["score"], json!(1.0), "{first}");
}

#[tokio::test]
async fn an_unknown_score_kind_is_refused_and_the_search_is_not_run() {
    let h = Harness::start(0, false).await;
    let (status, body) = h
        .search(
            &user_in_standard_org(),
            json!({ "query": "solar panels", "score_kinds": ["cross_encoder.v9"] }),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "SCORE_KIND_UNSUPPORTED");
    assert!(h.zdr_calls().is_empty());
}

#[tokio::test]
async fn an_onboarding_org_is_never_sent_to_the_reranker() {
    let h = Harness::start(0, false).await;
    let token = token("user_in_standard_org", |claims| {
        claims.insert("org_id".into(), json!("onboarding"));
    });
    let (status, body) = h.search(&token, json!({ "query": "solar panels" })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["rerank"]["status"], "skipped");
    assert_eq!(body["rerank"]["reason"], "input_not_tenant");
    assert_eq!(h.mint_count().await, 0);
}

#[tokio::test]
async fn shadow_mode_keeps_the_order_and_emits_no_score() {
    let h = Harness::start(1, true).await;
    let (status, body) = h
        .search(
            &user_in_standard_org(),
            json!({ "query": "solar panels", "score_kinds": ["cross_encoder.v1"] }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["rerank"]["status"], "shadow", "{body}");
    assert_eq!(body["rerank"]["reason"], "would_apply");
    assert!(body.get("score_kind").is_none());
    let urls: Vec<&str> = body["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["url"].as_str().unwrap())
        .collect();
    assert_eq!(
        urls,
        vec![
            "https://page.test/0",
            "https://page.test/1",
            "https://page.test/2",
            "https://page.test/3"
        ],
        "shadow keeps the input order"
    );
    assert!(body["results"]
        .as_array()
        .unwrap()
        .iter()
        .all(|r| r.get("score").is_none()));
    assert_eq!(
        h.data_plane_calls().await,
        1,
        "the Data Plane is asked, for measurement"
    );
}
