//! A trimmed copy of the real-auth harness in `zdr_claim_floor.rs`, for the
//! tests that need their own binary (a process-global tracing subscriber must
//! not share a process with tests that install another one).
//!
//! Same rules as the original: no dev bypass, every request runs the real
//! `require_auth` against a JWKS endpoint, and the claim sets are the golden
//! Auth Core minted. Declared with
//! `#[path = "support/zdr_harness.rs"] mod zdr_harness;` so `support/mod.rs`
//! is not edited.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex, OnceLock};

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::mpsc;
use url::Url;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use quarry_core::output::DriverKind;
use quarry_core::QuarryResult;
use quarry_edge::state::AppState;
use quarry_runtime::driver::{Driver, FetchHints};
use quarry_runtime::fetch::FetchResponse;
use quarry_runtime::{EventSink, UsageEvent, UsageMeter};

const GOLDEN: &str = include_str!("../fixtures/auth-core/quarry-token-claims.golden.json");

const DEPLOYED_AUDIENCE: &str = "quarry";
const DEPLOYED_ISSUER: &str = "http://localhost:3011/api/convex-auth";
const KID: &str = "zdr-log-capture-kid";

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

/// A token whose claims are the golden case `case`, freshly timed.
fn token(case: &str) -> String {
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

    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some(KID.to_owned());
    let key = EncodingKey::from_rsa_pem(fixture().private_pem.as_bytes()).expect("signing key");
    encode(&header, &Value::Object(claims), &key).expect("sign token")
}

pub fn user_in_zdr_org() -> String {
    token("user_in_zdr_org")
}

pub fn user_in_standard_org() -> String {
    token("user_in_standard_org")
}

#[derive(Default)]
pub struct RecordingMeter {
    pub events: Mutex<Vec<UsageEvent>>,
}

#[async_trait]
impl UsageMeter for RecordingMeter {
    async fn meter(&self, event: UsageEvent) {
        self.events.lock().unwrap().push(event);
    }
}

/// Static driver serving a page whose title and body both carry the URL it
/// was asked for, so a leak of either shows up as the URL.
struct EchoDriver;

#[async_trait]
impl Driver for EchoDriver {
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
            body: format!(
                "<html><head><title>Title of {url}</title></head><body><p>plain page text for the log capture test of {url}, long enough to read as content.</p></body></html>"
            )
            .into_bytes(),
            duration_ms: 1,
            served_by: DriverKind::Static,
        })
    }
}

pub struct Harness {
    addr: SocketAddr,
    client: reqwest::Client,
    pub usage: Arc<RecordingMeter>,
}

impl Harness {
    pub async fn start() -> Self {
        fixture();

        let (tx, mut rx) = mpsc::channel::<quarry_core::event::Event>(1024);
        tokio::spawn(async move { while rx.recv().await.is_some() {} });

        let usage = Arc::new(RecordingMeter::default());
        let state = AppState {
            usage: usage.clone(),
            ..crate::support::base_state(Arc::new(EchoDriver), EventSink::new(tx))
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
            usage,
        }
    }

    pub async fn post(&self, bearer: &str, route: &str, body: Value) -> reqwest::Response {
        self.client
            .post(format!("http://{}{route}", self.addr))
            .bearer_auth(bearer)
            .json(&body)
            .send()
            .await
            .expect("request reaches the edge")
    }
}
