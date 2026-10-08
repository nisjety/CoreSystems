//! What the tests of contextual retrieval stand on: the pinned contracts, the
//! migration's own vocabulary, and a loopback stand-in for Auth Core that answers
//! with what Auth Core's real mint produced.
//!
//! None of it is a rule. The rules under test are the real ones (the shared token
//! client with its claim checks, the decoder of inference-core's statuses, this
//! crate's own mapping), and the data they are given is data the owners
//! generated: `tests/fixtures/contracts/README.md` says where each file comes from.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::routing::post;
use axum::Router;
use chrono::{DateTime, TimeZone, Utc};
use inference_client_rs::{Clock, InferenceTokenClient, RetentionPosture, TokenClientConfig};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

pub const CONTRACTS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/contracts");
pub const STATUS_GOLDEN: &str = "infer-statuses.json";
pub const TOKEN_GOLDEN: &str = "inference-service-tokens.golden.json";
pub const AZURE_CAPTURE: &str = "infer-response-azure-live.json";
pub const SERVICE_KEY: &str = "contract-test-service-key-0123456789";
pub const TOKEN_PATH: &str = "/api/inference-core/internal-token";
/// The migration that defines the vocabulary, relative to the crate.
const MIGRATION: &str = "../../infra/postgres/migrations/20261003130000_context_provenance.sql";

/// A pipeline that expects the build to be complete (`REQUIRE_CONTRACT`, or CI):
/// a contract that cannot be checked is then a failure, not a skip.
pub fn require_contract() -> bool {
    ["REQUIRE_CONTRACT", "CI"]
        .iter()
        .any(|name| std::env::var(name).is_ok_and(|value| !value.is_empty()))
}

/// The same, for the one contract that only the live proof can produce (the real
/// Azure answer). A plain CI run of the unit tests does not demand it; a run that
/// says the build is complete does.
pub fn require_capture() -> bool {
    std::env::var("REQUIRE_CONTRACT").is_ok_and(|value| !value.is_empty())
}

/// A vendored contract, checked against its sha256 pin. A missing or altered file
/// panics: nothing in these tests skips. The pin is over the LF-normalised bytes, so
/// a Windows checkout that rewrote the line endings still matches.
pub fn read_pinned(relative: &str) -> Value {
    let path = PathBuf::from(CONTRACTS).join(relative);
    let bytes = std::fs::read(&path).unwrap_or_else(|error| {
        panic!(
            "contract golden {} is missing ({error}); vendor it from its owner",
            path.display()
        )
    });
    let pin_path = PathBuf::from(format!("{}.sha256", path.display()));
    let pin = std::fs::read_to_string(&pin_path)
        .unwrap_or_else(|error| panic!("pin {} is missing ({error})", pin_path.display()));
    let expected = pin
        .split_whitespace()
        .next()
        .expect("a pin file holds a sha256");
    let normalised: Vec<u8> = bytes
        .iter()
        .copied()
        .filter(|byte| *byte != b'\r')
        .collect();
    let actual = format!("{:x}", Sha256::digest(&normalised));
    assert_eq!(
        actual, expected,
        "{relative} no longer matches its sha256 pin; it is vendored from its owner and never edited here"
    );
    serde_json::from_slice(&bytes).expect("a golden is JSON")
}

/// The migration's SQL, when this checkout has it. The Docker build context does not
/// (it holds `services/` only), and that build runs no tests; a full checkout does.
pub fn migration_sql() -> Option<String> {
    std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(MIGRATION)).ok()
}

/// Every `'quoted'` token in the statement that follows `marker`, up to its `;`.
pub fn quoted_set_after(sql: &str, marker: &str) -> BTreeSet<String> {
    let start = sql
        .find(marker)
        .unwrap_or_else(|| panic!("the migration no longer defines `{marker}`"));
    let statement = &sql[start..];
    let statement = &statement[..statement.find(';').expect("a statement ends with `;`")];
    statement
        .split('\'')
        .skip(1)
        .step_by(2)
        .map(str::to_owned)
        .collect()
}

/// A status exactly as inference-core's contract says it arrives (code and
/// trailers of the case with this id), carrying a message that points the wrong way
/// on purpose: the decoder must not read it.
pub fn golden_status(id: &str) -> tonic::Status {
    let golden = read_pinned(STATUS_GOLDEN);
    let case = golden["cases"]
        .as_array()
        .expect("cases")
        .iter()
        .find(|case| case["id"] == id)
        .unwrap_or_else(|| panic!("the status golden has no case {id}"));
    let mut metadata = tonic::metadata::MetadataMap::new();
    for (key, value) in case["trailers"].as_object().expect("trailers") {
        metadata.insert(
            tonic::metadata::MetadataKey::from_bytes(key.as_bytes()).expect("a trailer key"),
            tonic::metadata::MetadataValue::try_from(value.as_str().expect("a trailer value"))
                .expect("ascii"),
        );
    }
    tonic::Status::with_metadata(
        tonic::Code::from_i32(
            i32::try_from(case["grpc_code_number"].as_i64().expect("code number")).expect("code"),
        ),
        "service unavailable, please retry; this is not a zero data retention refusal",
        metadata,
    )
}

#[derive(Deserialize)]
pub struct AzureCapture {
    pub cases: Vec<AzureCase>,
}

/// One real answer: the fields of inference-core's `InferResponse` that the
/// contextualizer reads, as the live proof recorded them.
#[derive(Deserialize)]
pub struct AzureCase {
    pub name: String,
    pub stop_reason: String,
    pub model_used: String,
    pub provider_used: String,
    pub residency: String,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cache_read_input_tokens: u32,
    pub usage_reported: bool,
    pub response_cache_hit: bool,
    pub content: String,
}

/// The real Azure answer, if the live proof has captured it.
pub fn captured_azure() -> Option<AzureCapture> {
    if !PathBuf::from(CONTRACTS).join(AZURE_CAPTURE).exists() {
        return None;
    }
    let value = read_pinned(AZURE_CAPTURE);
    Some(serde_json::from_value(value).expect("the capture has the documented shape"))
}

/// A clock the test moves. The tokens in the golden were minted at a frozen instant
/// long gone on the wall clock.
pub struct ManualClock(Mutex<DateTime<Utc>>);

impl ManualClock {
    pub fn at(seconds: i64) -> Arc<Self> {
        Arc::new(Self(Mutex::new(instant(seconds))))
    }

    /// Move the clock forward: a bearer minted before is then past its reuse.
    pub fn advance(&self, seconds: i64) {
        let mut now = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        *now += chrono::Duration::seconds(seconds);
    }
}

impl Clock for ManualClock {
    fn now(&self) -> DateTime<Utc> {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn instant(seconds: i64) -> DateTime<Utc> {
    Utc.timestamp_opt(seconds, 0).single().expect("valid time")
}

/// One exchange Auth Core's real mint captured.
#[derive(Clone)]
pub struct Case {
    pub name: String,
    pub service_id: String,
    pub org_id: String,
    pub reason: String,
    pub registry_retention: String,
    pub status: u16,
    pub body: Value,
}

impl Case {
    pub fn posture(&self) -> RetentionPosture {
        RetentionPosture::parse(&self.registry_retention).expect("a known posture")
    }
}

pub struct TokenGolden {
    value: Value,
}

impl TokenGolden {
    pub fn load() -> Self {
        Self {
            value: read_pinned(TOKEN_GOLDEN),
        }
    }

    pub fn issuer(&self) -> String {
        self.value["issuer"].as_str().expect("issuer").to_owned()
    }

    pub fn frozen_now(&self) -> i64 {
        self.value["frozenNowSeconds"].as_i64().expect("frozen now")
    }

    pub fn case(&self, name: &str) -> Case {
        let entry = &self.value["cases"][name];
        assert!(!entry.is_null(), "the token golden has no case {name}");
        let text = |key: &str| entry["request"][key].as_str().expect(key).to_owned();
        Case {
            name: name.to_owned(),
            service_id: text("serviceId"),
            org_id: text("orgId"),
            reason: text("reason"),
            registry_retention: text("registryRetention"),
            status: u16::try_from(entry["response"]["status"].as_u64().expect("status"))
                .expect("http status"),
            body: entry["response"]["body"].clone(),
        }
    }
}

type Responder = Box<dyn Fn(&Value, &HeaderMap) -> (u16, Vec<u8>) + Send + Sync>;

struct StubState {
    responder: Responder,
    requests: Mutex<Vec<Value>>,
}

/// A loopback Auth Core token endpoint. A transport: it answers with the bytes
/// Auth Core produced, selected by what was asked. Every rule applied to them is the
/// client's own.
pub struct TokenStub {
    pub url: String,
    state: Arc<StubState>,
    server: tokio::task::JoinHandle<()>,
}

impl Drop for TokenStub {
    fn drop(&mut self) {
        self.server.abort();
    }
}

impl TokenStub {
    /// Answers each request with the captured exchange that matches what was asked
    /// (service, organization and reason). A request nothing matches is answered
    /// 403, as Auth Core answers a principal it does not know.
    pub async fn replaying(cases: Vec<Case>) -> Self {
        Self::start(Box::new(move |body, headers| {
            let asked_org = body["orgId"].as_str().unwrap_or_default();
            let asked_reason = body["reason"].as_str().unwrap_or_default();
            let asked_service = headers
                .get("x-service-id")
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default();
            match cases.iter().find(|case| {
                case.service_id == asked_service
                    && case.org_id == asked_org
                    && case.reason == asked_reason
            }) {
                Some(case) => (case.status, serde_json::to_vec(&case.body).expect("encode")),
                None => (
                    403,
                    serde_json::to_vec(&serde_json::json!({
                        "statusCode": 403,
                        "message": "Service principal is not authorized",
                        "error": "Forbidden"
                    }))
                    .expect("encode"),
                ),
            }
        }))
        .await
    }

    async fn start(responder: Responder) -> Self {
        let state = Arc::new(StubState {
            responder,
            requests: Mutex::new(Vec::new()),
        });
        let app = Router::new()
            .route(TOKEN_PATH, post(handle))
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind loopback");
        let address = listener.local_addr().expect("local address");
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Self {
            url: format!("http://{address}{TOKEN_PATH}"),
            state,
            server,
        }
    }

    /// The bodies of the requests the stub has received.
    pub fn requests(&self) -> Vec<Value> {
        self.state
            .requests
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub fn request_count(&self) -> usize {
        self.requests().len()
    }

    /// A stub whose answers the test scripts, for what a captured exchange cannot
    /// show: the same organization answered differently at a later mint (it turned
    /// zero-retention in between).
    pub async fn scripted(
        responder: impl Fn(&Value, &HeaderMap) -> (u16, Vec<u8>) + Send + Sync + 'static,
    ) -> Self {
        Self::start(Box::new(responder)).await
    }

    /// A real token client, configured for the embedding engine as Auth Core's
    /// registry row says (`posture`), reading the frozen clock of the golden.
    pub fn client(&self, golden: &TokenGolden, posture: RetentionPosture) -> InferenceTokenClient {
        self.client_on(golden, posture, ManualClock::at(golden.frozen_now() + 1))
    }

    /// The same, on a clock the test holds and moves.
    pub fn client_on(
        &self,
        golden: &TokenGolden,
        posture: RetentionPosture,
        clock: Arc<ManualClock>,
    ) -> InferenceTokenClient {
        InferenceTokenClient::with_clock(
            TokenClientConfig {
                token_url: self.url.clone(),
                expected_issuer: golden.issuer(),
                service_id: "embedding-engine".to_owned(),
                service_api_key: SERVICE_KEY.to_owned(),
                posture,
                allow_unconfigured: false,
            },
            clock,
        )
        .expect("a valid client configuration")
    }
}

async fn handle(
    State(state): State<Arc<StubState>>,
    headers: HeaderMap,
    body: Bytes,
) -> (StatusCode, [(header::HeaderName, &'static str); 1], Vec<u8>) {
    let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    state
        .requests
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(body.clone());
    let (status, bytes) = (state.responder)(&body, &headers);
    (
        StatusCode::from_u16(status).expect("status"),
        [(header::CONTENT_TYPE, "application/json")],
        bytes,
    )
}
