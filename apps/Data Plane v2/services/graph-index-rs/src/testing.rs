//! Test support: stand-ins for transports, and only for transports.
//!
//! The 2026-10-01 live proof found six defects that permissive doubles had hidden, so the rule
//! here is the one the other contract tests follow: a stand-in may carry bytes, it may not
//! decide. What it carries is captured from the real system:
//!
//! * [`GoldenAuthCore`] answers a token request with the bundle Auth Core's real mint produced
//!   (`services/inference-client-rs/tests/fixtures/auth-core/inference-service-tokens.golden.json`,
//!   vendored there byte for byte with a sha256 pin), selected by what was asked. Every rule
//!   about those tokens (claims, posture, lifetime, the cache) is the real client's, applied to
//!   real tokens, under a clock moved to the moment they were minted.
//! * [`Scripted`] stands in for inference-core's gRPC endpoint. A success looks like a real
//!   `InferResponse`; a failure is built from a case of the status contract inference-core
//!   generates from the code that builds its statuses (`infer-statuses.json`, same pin).
//!
//! Everything above those two (the extractor, the posture check, the metering, the typed
//! errors, the state machine) is the production code.

use std::collections::{HashSet, VecDeque};
use std::io;
use std::sync::{Arc, Mutex, PoisonError};

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::routing::post;
use axum::Router;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::{DateTime, TimeZone, Utc};
use inference_client_rs::Clock;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tonic::metadata::{MetadataKey, MetadataMap, MetadataValue};
use tonic::{Code, Request, Status};
use tracing_subscriber::fmt::MakeWriter;

use crate::config::Config;
use crate::extractor::model_plane::v1::{InferRequest, InferResponse, ProviderTiming};

const FIXTURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../inference-client-rs/tests/fixtures"
);
const TOKEN_GOLDEN: &str = "auth-core/inference-service-tokens.golden.json";
const STATUS_GOLDEN: &str = "inference-core/infer-statuses.json";
pub const TOKEN_PATH: &str = "/api/inference-core/internal-token";
pub const SERVICE_KEY: &str = "contract-test-service-key-0123456789";

/// The organizations the token golden knows.
pub const ORG_PLAIN: &str = "org-contract-plain";
pub const ORG_ZDR: &str = "org-contract-zdr";

/// Reads a vendored golden and checks it against its sha256 pin. A missing or altered golden
/// panics: nothing in these tests skips.
pub fn read_pinned(relative: &str) -> Value {
    let path = format!("{FIXTURES}/{relative}");
    let bytes = std::fs::read(&path)
        .unwrap_or_else(|error| panic!("contract golden {path} is missing ({error})"));
    let pin = std::fs::read_to_string(format!("{path}.sha256"))
        .unwrap_or_else(|error| panic!("pin {path}.sha256 is missing ({error})"));
    let expected = pin.split_whitespace().next().expect("a pin holds a sha256");
    // Pinned over the LF-normalised bytes: a Windows checkout may rewrite line endings.
    let normalised: Vec<u8> = bytes
        .iter()
        .copied()
        .filter(|byte| *byte != b'\r')
        .collect();
    assert_eq!(
        format!("{:x}", Sha256::digest(&normalised)),
        expected,
        "{relative} no longer matches its sha256 pin; it is vendored from its owner and never edited here"
    );
    serde_json::from_slice(&bytes).expect("a golden is JSON")
}

// ── the status contract ──────────────────────────────────────────────────────

/// The pinned status contract of inference-core.
pub fn status_golden() -> Value {
    read_pinned(STATUS_GOLDEN)
}

/// The status as it arrives: the code and the trailers of a case of the contract in the
/// metadata, and a message that names a DIFFERENT failure from the one the trailers carry. A
/// decoder that read the message would be talked into the wrong answer by it.
pub fn status_as_received(case: &Value) -> Status {
    let code = Code::from_i32(
        i32::try_from(case["grpc_code_number"].as_i64().expect("code number")).expect("fits"),
    );
    let mut metadata = MetadataMap::new();
    for (key, value) in case["trailers"].as_object().expect("trailers") {
        metadata.insert(
            MetadataKey::from_bytes(key.as_bytes()).expect("trailer key"),
            MetadataValue::try_from(value.as_str().expect("trailer value")).expect("ascii"),
        );
    }
    Status::with_metadata(
        code,
        "the model answered fine, nothing to see here",
        metadata,
    )
}

/// The status of one case of the contract, by id.
pub fn golden_status(id: &str) -> Status {
    let golden = status_golden();
    let case = golden["cases"]
        .as_array()
        .expect("cases")
        .iter()
        .find(|case| case["id"].as_str() == Some(id))
        .unwrap_or_else(|| panic!("the status contract has no case {id}"));
    status_as_received(case)
}

// ── time ─────────────────────────────────────────────────────────────────────

/// A clock the test moves.
pub struct ManualClock(Mutex<DateTime<Utc>>);

impl ManualClock {
    pub fn at(seconds: i64) -> Arc<Self> {
        Arc::new(Self(Mutex::new(instant(seconds))))
    }

    /// The moment the token golden was minted.
    pub fn at_golden_mint() -> Arc<Self> {
        Self::at(
            read_pinned(TOKEN_GOLDEN)["frozenNowSeconds"]
                .as_i64()
                .expect("frozen now"),
        )
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

// ── Auth Core ────────────────────────────────────────────────────────────────

#[derive(Clone)]
struct Captured {
    service_id: String,
    org_id: String,
    reason: String,
    status: u16,
    body: Value,
}

struct AuthState {
    cases: Vec<Captured>,
    posture_unavailable: Value,
    /// Organizations whose posture Auth Core "cannot resolve" right now.
    down: Mutex<HashSet<String>>,
    /// Organizations (besides the golden's own) that are zero-retention.
    zdr: Mutex<HashSet<String>>,
    requests: Mutex<Vec<Value>>,
}

/// A loopback Auth Core token endpoint that replays what the real mint produced.
pub struct GoldenAuthCore {
    pub url: String,
    pub issuer: String,
    state: Arc<AuthState>,
    server: tokio::task::JoinHandle<()>,
}

impl Drop for GoldenAuthCore {
    fn drop(&mut self) {
        self.server.abort();
    }
}

impl GoldenAuthCore {
    pub async fn start() -> Self {
        let golden = read_pinned(TOKEN_GOLDEN);
        let mut cases = Vec::new();
        for entry in golden["cases"].as_object().expect("cases").values() {
            let request = &entry["request"];
            let text = |key: &str| request[key].as_str().expect(key).to_owned();
            cases.push(Captured {
                service_id: text("serviceId"),
                org_id: text("orgId"),
                reason: text("reason"),
                status: u16::try_from(entry["response"]["status"].as_u64().expect("status"))
                    .expect("http status"),
                body: entry["response"]["body"].clone(),
            });
        }
        let state = Arc::new(AuthState {
            cases,
            posture_unavailable: golden["cases"]["tenantPostureUnavailable"]["response"]["body"]
                .clone(),
            down: Mutex::new(HashSet::new()),
            zdr: Mutex::new(HashSet::new()),
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
            issuer: golden["issuer"].as_str().expect("issuer").to_owned(),
            state,
            server,
        }
    }

    /// From now on Auth Core answers 503 `retention_posture_unavailable` (the body its real code
    /// sends) for this organization, and issues no token.
    pub fn cannot_resolve_posture_of(&self, org_id: &str) {
        self.state
            .down
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(org_id.to_owned());
    }

    /// From now on this organization is zero-retention: Auth Core issues it the captured
    /// zero-retention bundle (`zdr: true`), re-addressed to it.
    pub fn is_zero_retention(&self, org_id: &str) {
        self.state
            .zdr
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(org_id.to_owned());
    }

    pub fn recovers(&self, org_id: &str) {
        self.state
            .down
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(org_id);
    }

    /// The token requests received, as `(org_id, reason)`.
    pub fn requested(&self) -> Vec<(String, String)> {
        self.state
            .requests
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .map(|body| {
                (
                    body["orgId"].as_str().unwrap_or_default().to_owned(),
                    body["reason"].as_str().unwrap_or_default().to_owned(),
                )
            })
            .collect()
    }
}

async fn handle(
    State(state): State<Arc<AuthState>>,
    headers: HeaderMap,
    body: Bytes,
) -> (StatusCode, [(header::HeaderName, &'static str); 1], Vec<u8>) {
    let asked: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    state
        .requests
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(asked.clone());
    let text = |key: &str| asked[key].as_str().unwrap_or_default().to_owned();
    let (org, reason) = (text("orgId"), text("reason"));
    let service = headers
        .get("x-service-id")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let down = state
        .down
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .contains(&org);
    let zdr = org == ORG_ZDR
        || state
            .zdr
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains(&org);
    let (status, body) = if down {
        (503, state.posture_unavailable.clone())
    } else {
        // The bundle Auth Core minted for the organization and reason asked, when the golden
        // holds one. Otherwise the captured bundle of a zero-retention or an ordinary
        // organization is re-addressed to the organization and reason asked: the client does not
        // verify signatures (inference-core does), and the claims it must judge, the
        // organization and the reason, are the ones edited. The `zdr` claim, the posture under
        // test, is always the captured one.
        let exact = state.cases.iter().find(|case| {
            case.service_id == service
                && case.org_id == org
                && case.reason == reason
                && case.status == 201
        });
        let base_org = if zdr { ORG_ZDR } else { ORG_PLAIN };
        let base = state.cases.iter().find(|case| {
            case.service_id == service && case.org_id == base_org && case.status == 201
        });
        match (exact, base) {
            (Some(case), _) => (case.status, case.body.clone()),
            (None, Some(case)) => {
                let mut body = case.body.clone();
                body["token"] = Value::String(readdress(
                    body["token"].as_str().expect("a token"),
                    &org,
                    &reason,
                ));
                (case.status, body)
            }
            (None, None) => (
                403,
                serde_json::json!({"statusCode":403,"message":"Service principal is not authorized","error":"Forbidden"}),
            ),
        }
    };
    (
        StatusCode::from_u16(status).expect("status"),
        [(header::CONTENT_TYPE, "application/json")],
        serde_json::to_vec(&body).expect("encode reply"),
    )
}

/// A token whose `org_id` and `reason` claims were edited after the mint. The signature is the
/// original one and no longer matches: the client does not verify signatures (inference-core
/// does), so the edit isolates the claims under test.
fn readdress(token: &str, org_id: &str, reason: &str) -> String {
    let mut parts: Vec<String> = token.split('.').map(str::to_owned).collect();
    let mut claims: Value =
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(&parts[1]).expect("payload"))
            .expect("claims");
    claims["org_id"] = Value::String(org_id.to_owned());
    claims["reason"] = Value::String(reason.to_owned());
    parts[1] = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).expect("encode claims"));
    parts.join(".")
}

// ── inference-core ───────────────────────────────────────────────────────────

/// What a stand-in for inference-core does with a request: answers, fails with a status, or
/// never answers at all (the extractor's own deadline then decides).
pub enum Canned {
    Answer(InferResponse),
    Fail(Status),
    Hang,
}

/// What a test's stand-in for inference-core does with a request.
pub trait CannedInference: Send + Sync {
    fn infer(&self, request: Request<InferRequest>) -> Canned;
}

/// What inference-core was asked.
#[derive(Clone, Debug)]
pub struct Call {
    pub org_id: String,
    pub authorization: Option<String>,
    pub zdr: bool,
    pub prompt: String,
    pub request_id: String,
}

#[derive(Clone)]
pub enum Reply {
    Answer(InferResponse),
    Fail(Status),
    /// The provider never answers.
    Hang,
}

#[derive(Default)]
struct ScriptState {
    queue: VecDeque<Reply>,
    default: Option<Reply>,
    calls: Vec<Call>,
}

/// Answers from a script, then from a default, and records every call it was asked for.
#[derive(Default)]
pub struct Scripted {
    state: Mutex<ScriptState>,
}

impl Scripted {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Every call from now on gets this reply (after the queued ones).
    pub fn always(self: &Arc<Self>, reply: Reply) -> Arc<Self> {
        self.lock().default = Some(reply);
        self.clone()
    }

    pub fn then(self: &Arc<Self>, reply: Reply) -> Arc<Self> {
        self.lock().queue.push_back(reply);
        self.clone()
    }

    pub fn calls(&self) -> Vec<Call> {
        self.lock().calls.clone()
    }

    pub fn call_count(&self) -> usize {
        self.lock().calls.len()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, ScriptState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl CannedInference for Scripted {
    fn infer(&self, request: Request<InferRequest>) -> Canned {
        let authorization = request
            .metadata()
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let message = request.into_inner();
        let mut state = self.lock();
        state.calls.push(Call {
            org_id: message.org_id.clone(),
            authorization,
            zdr: message.zdr,
            prompt: message
                .messages
                .last()
                .map(|message| message.content.clone())
                .unwrap_or_default(),
            request_id: message.request_id.clone(),
        });
        let reply = state
            .queue
            .pop_front()
            .or_else(|| state.default.clone())
            .unwrap_or_else(|| Reply::Fail(Status::internal("the test script ran out of replies")));
        match reply {
            Reply::Answer(mut response) => {
                response.request_id = message.request_id;
                Canned::Answer(response)
            }
            Reply::Fail(status) => Canned::Fail(status),
            Reply::Hang => Canned::Hang,
        }
    }
}

/// A real-looking answer: 612 prompt tokens of which 512 were served from the provider's prompt
/// cache, 48 completion tokens, served by the model the registry names.
pub fn answer(content: &str) -> Reply {
    Reply::Answer(InferResponse {
        content: content.to_owned(),
        model_used: "gpt-4o-mini-2024-07-18".to_owned(),
        provider_used: "azure-openai".to_owned(),
        input_tokens: 612,
        output_tokens: 48,
        cache_read_input_tokens: 512,
        provider_timing: Some(ProviderTiming {
            usage_reported: true,
            response_cache_hit: false,
            attempts: 1,
            ..Default::default()
        }),
        ..Default::default()
    })
}

/// An extraction with one entity, one relationship and one claim.
pub const RICH_EXTRACTION: &str = r#"{"entities":[{"entity_type":"Organization","entity_text":"Aquatiq","confidence":0.9},{"entity_type":"Concept","entity_text":"Hygiene","confidence":0.8}],"relationships":[{"source_entity":"Aquatiq","target_entity":"Hygiene","relation_type":"works_on","confidence":0.8}],"claims":[{"claim_text":"Aquatiq sells hygiene solutions","related_entities":["Aquatiq"],"confidence":0.8}]}"#;

/// An extraction that found nothing, which is a clean answer and not a failure.
pub const EMPTY_EXTRACTION: &str = r#"{"entities":[],"relationships":[],"claims":[]}"#;

/// A reply that is a failure of the case of the status contract named.
pub fn fail(id: &str) -> Reply {
    Reply::Fail(golden_status(id))
}

// ── configuration and the extractor ──────────────────────────────────────────

/// The configuration of a deployment whose Auth Core is `auth`, on the `tenant` posture.
pub fn config_for(auth: &GoldenAuthCore) -> Config {
    let mut config: Config = envy::from_iter(Vec::<(String, String)>::new()).expect("config");
    config.model_plane_extraction_model = "gpt-4o-mini".to_owned();
    config.model_plane_inference_token_url = auth.url.clone();
    config.model_plane_inference_token_issuer = auth.issuer.clone();
    config.model_plane_inference_service_id = "graph-index".to_owned();
    config.model_plane_inference_service_api_key = SERVICE_KEY.to_owned();
    config.model_plane_inference_retention_posture = "tenant".to_owned();
    config
}

// ── logs ─────────────────────────────────────────────────────────────────────

/// Collects what `tracing` writes, as the JSON lines the service logs in production.
#[derive(Clone, Default)]
pub struct LogCapture(Arc<Mutex<Vec<u8>>>);

pub struct LogWriter(Arc<Mutex<Vec<u8>>>);

impl io::Write for LogWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for LogCapture {
    type Writer = LogWriter;

    fn make_writer(&'a self) -> LogWriter {
        LogWriter(self.0.clone())
    }
}

impl LogCapture {
    /// Every event logged so far.
    pub fn events(&self) -> Vec<Value> {
        let bytes = self
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        String::from_utf8_lossy(&bytes)
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect()
    }

    /// The events whose message is exactly `message`; each is its `fields` object plus `level`.
    pub fn events_saying(&self, message: &str) -> Vec<Value> {
        self.events()
            .into_iter()
            .filter(|event| event["fields"]["message"].as_str() == Some(message))
            .map(|event| {
                let mut fields = event["fields"].clone();
                fields["level"] = event["level"].clone();
                fields
            })
            .collect()
    }

    /// The whole log, for a test that asserts nothing in it names a document.
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap_or_else(PoisonError::into_inner)).into_owned()
    }
}

/// Install a JSON subscriber for the current thread (a `#[tokio::test]` runs on one) that writes
/// to the returned capture. Keep the guard alive for the duration of the test.
pub fn capture_logs() -> (LogCapture, tracing::subscriber::DefaultGuard) {
    let capture = LogCapture::default();
    let subscriber = tracing_subscriber::fmt()
        .json()
        .with_writer(capture.clone())
        .with_max_level(tracing::Level::DEBUG)
        .finish();
    (capture, tracing::subscriber::set_default(subscriber))
}
