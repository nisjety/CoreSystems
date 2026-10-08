//! Helpers shared by the contract tests: the pinned golden loader, a manual clock,
//! a forger for the claims of a captured token, and a loopback stand-in for Auth
//! Core's token endpoint.
//!
//! The stand-in is a transport and nothing else. It answers with bytes that Auth
//! Core's real mint produced (the golden), selected by what the client asked, so
//! every rule these tests exercise (claim validation, posture, lifetime, cache)
//! is the client's own, applied to real tokens.

#![allow(dead_code)] // each test binary uses a different subset

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::routing::post;
use axum::Router;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::{DateTime, TimeZone, Utc};
use inference_client_rs::{Clock, InferenceTokenClient, RetentionPosture, TokenClientConfig};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");
pub const TOKEN_GOLDEN: &str = "auth-core/inference-service-tokens.golden.json";
pub const STATUS_GOLDEN: &str = "inference-core/infer-statuses.json";
pub const SERVICE_KEY: &str = "contract-test-service-key-0123456789";
pub const TOKEN_PATH: &str = "/api/inference-core/internal-token";

/// True when a pipeline expects the build to be complete: a contract that cannot
/// be checked is then a failure, not a skip.
pub fn require_contract() -> bool {
    ["REQUIRE_CONTRACT", "CI"]
        .iter()
        .any(|name| std::env::var(name).is_ok_and(|value| !value.is_empty()))
}

/// Reads a vendored golden and checks it against its sha256 pin. A missing or
/// altered golden panics: nothing in these tests skips.
pub fn read_pinned(relative: &str) -> Value {
    let path = format!("{FIXTURES}/{relative}");
    let bytes = std::fs::read(&path).unwrap_or_else(|error| {
        panic!("contract golden {path} is missing ({error}); vendor it from its owner")
    });
    let pin = std::fs::read_to_string(format!("{path}.sha256"))
        .unwrap_or_else(|error| panic!("pin {path}.sha256 is missing ({error})"));
    let expected = pin
        .split_whitespace()
        .next()
        .expect("pin file holds a sha256");
    // Pinned over the LF-normalised bytes: a Windows checkout may rewrite line endings.
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
    serde_json::from_slice(&bytes).expect("golden is JSON")
}

/// A clock the test moves.
pub struct ManualClock(Mutex<DateTime<Utc>>);

impl ManualClock {
    pub fn at(seconds: i64) -> Arc<Self> {
        Arc::new(Self(Mutex::new(instant(seconds))))
    }

    pub fn set(&self, seconds: i64) {
        *self.lock() = instant(seconds);
    }

    fn lock(&self) -> MutexGuard<'_, DateTime<Utc>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Clock for ManualClock {
    fn now(&self) -> DateTime<Utc> {
        *self.lock()
    }
}

pub fn instant(seconds: i64) -> DateTime<Utc> {
    Utc.timestamp_opt(seconds, 0).single().expect("valid time")
}

/// The captured token golden.
pub struct Golden {
    value: Value,
}

/// One captured exchange.
#[derive(Clone)]
pub struct Case {
    pub name: String,
    pub service_id: String,
    pub org_id: String,
    pub reason: String,
    pub registry_retention: String,
    pub status: u16,
    pub body: Value,
    pub claims: Option<Value>,
}

impl Case {
    /// The posture a client deployed against this registry row has.
    pub fn posture(&self) -> RetentionPosture {
        match self.registry_retention.as_str() {
            "persistent" => RetentionPosture::Persistent,
            "zdr" => RetentionPosture::ZeroRetention,
            "tenant" => RetentionPosture::Tenant,
            other => panic!("unknown registry retention {other}"),
        }
    }

    pub fn is_success(&self) -> bool {
        self.status == 201
    }

    pub fn token(&self) -> &str {
        self.body["token"]
            .as_str()
            .expect("a success carries a token")
    }

    pub fn zdr(&self) -> bool {
        self.claims.as_ref().expect("a success has claims")["zdr"]
            .as_bool()
            .expect("zdr claim")
    }

    pub fn exp(&self) -> i64 {
        self.claims.as_ref().expect("a success has claims")["exp"]
            .as_i64()
            .expect("exp claim")
    }

    pub fn iat(&self) -> i64 {
        self.claims.as_ref().expect("a success has claims")["iat"]
            .as_i64()
            .expect("iat claim")
    }
}

impl Golden {
    pub fn load() -> Self {
        Self {
            value: read_pinned(TOKEN_GOLDEN),
        }
    }

    pub fn value(&self) -> &Value {
        &self.value
    }

    pub fn issuer(&self) -> String {
        self.value["issuer"].as_str().expect("issuer").to_owned()
    }

    pub fn frozen_now(&self) -> i64 {
        self.value["frozenNowSeconds"].as_i64().expect("frozen now")
    }

    pub fn reasons(&self) -> Vec<String> {
        self.value["reasons"]
            .as_array()
            .expect("reasons")
            .iter()
            .map(|reason| reason.as_str().expect("reason").to_owned())
            .collect()
    }

    pub fn cases(&self) -> BTreeMap<String, Case> {
        self.value["cases"]
            .as_object()
            .expect("cases")
            .iter()
            .map(|(name, entry)| {
                let request = &entry["request"];
                let text = |key: &str| request[key].as_str().expect(key).to_owned();
                (
                    name.clone(),
                    Case {
                        name: name.clone(),
                        service_id: text("serviceId"),
                        org_id: text("orgId"),
                        reason: text("reason"),
                        registry_retention: text("registryRetention"),
                        status: u16::try_from(
                            entry["response"]["status"].as_u64().expect("status"),
                        )
                        .expect("http status"),
                        body: entry["response"]["body"].clone(),
                        claims: entry.get("claims").cloned(),
                    },
                )
            })
            .collect()
    }

    pub fn case(&self, name: &str) -> Case {
        self.cases()
            .remove(name)
            .unwrap_or_else(|| panic!("the golden has no case {name}"))
    }

    /// Every case Auth Core answered with a token.
    pub fn successes(&self) -> Vec<Case> {
        self.cases()
            .into_values()
            .filter(Case::is_success)
            .collect()
    }
}

/// A token whose payload was edited after the mint. The signature is the original
/// one and no longer matches: the client does not verify signatures (inference-core
/// does), so each edit isolates the one claim the client is meant to judge.
pub fn forge_claims(token: &str, edit: impl FnOnce(&mut Value)) -> String {
    let mut parts: Vec<String> = token.split('.').map(str::to_owned).collect();
    let mut claims: Value =
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(&parts[1]).expect("payload"))
            .expect("claims");
    edit(&mut claims);
    parts[1] = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).expect("encode claims"));
    parts.join(".")
}

/// A token whose header was edited after the mint.
pub fn forge_header(token: &str, edit: impl FnOnce(&mut Value)) -> String {
    let mut parts: Vec<String> = token.split('.').map(str::to_owned).collect();
    let mut header: Value =
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(&parts[0]).expect("header"))
            .expect("header");
    edit(&mut header);
    parts[0] = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header).expect("encode header"));
    parts.join(".")
}

/// What the stand-in saw.
#[derive(Clone, Debug)]
pub struct SeenRequest {
    pub service_id: Option<String>,
    pub api_key: Option<String>,
    pub has_zdr_header: bool,
    pub body: Value,
}

/// What the stand-in answers.
#[derive(Clone)]
pub struct Reply {
    pub status: u16,
    pub body: Vec<u8>,
}

impl Reply {
    pub fn json(status: u16, body: &Value) -> Self {
        Self {
            status,
            body: serde_json::to_vec(body).expect("encode reply"),
        }
    }

    pub fn of(case: &Case) -> Self {
        Self::json(case.status, &case.body)
    }
}

type Responder = Box<dyn Fn(&SeenRequest) -> Reply + Send + Sync>;

struct StubState {
    responder: Mutex<Responder>,
    requests: Mutex<Vec<SeenRequest>>,
    delay: Mutex<Duration>,
}

/// A loopback Auth Core token endpoint.
pub struct Stub {
    pub url: String,
    state: Arc<StubState>,
    server: tokio::task::JoinHandle<()>,
}

impl Drop for Stub {
    fn drop(&mut self) {
        self.server.abort();
    }
}

impl Stub {
    pub async fn start(responder: impl Fn(&SeenRequest) -> Reply + Send + Sync + 'static) -> Self {
        let state = Arc::new(StubState {
            responder: Mutex::new(Box::new(responder)),
            requests: Mutex::new(Vec::new()),
            delay: Mutex::new(Duration::ZERO),
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

    /// Answers every request with the same captured exchange.
    pub async fn always(case: &Case) -> Self {
        let reply = Reply::of(case);
        Self::start(move |_| reply.clone()).await
    }

    /// Answers each request with the captured exchange that matches what was asked
    /// (service, organization and reason), from `cases`. A request nothing
    /// matches is answered 403, as Auth Core answers a principal it does not know.
    pub async fn replaying(cases: Vec<Case>) -> Self {
        Self::start(move |request| {
            let asked_org = request.body["orgId"].as_str().unwrap_or_default();
            let asked_reason = request.body["reason"].as_str().unwrap_or_default();
            let asked_service = request.service_id.as_deref().unwrap_or_default();
            cases
                .iter()
                .find(|case| {
                    case.service_id == asked_service
                        && case.org_id == asked_org
                        && case.reason == asked_reason
                })
                .map_or_else(
                    || {
                        Reply::json(
                            403,
                            &json!({"statusCode":403,"message":"Service principal is not authorized","error":"Forbidden"}),
                        )
                    },
                    Reply::of,
                )
        })
        .await
    }

    pub fn respond_with(&self, responder: impl Fn(&SeenRequest) -> Reply + Send + Sync + 'static) {
        *self
            .state
            .responder
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Box::new(responder);
    }

    pub fn delay(&self, delay: Duration) {
        *self
            .state
            .delay
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = delay;
    }

    pub fn requests(&self) -> Vec<SeenRequest> {
        self.state
            .requests
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub fn request_count(&self) -> usize {
        self.requests().len()
    }
}

async fn handle(
    State(state): State<Arc<StubState>>,
    headers: HeaderMap,
    body: Bytes,
) -> (StatusCode, [(header::HeaderName, &'static str); 1], Vec<u8>) {
    let header_text = |name: &str| {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
    };
    let seen = SeenRequest {
        service_id: header_text("x-service-id"),
        api_key: header_text("x-service-api-key"),
        has_zdr_header: headers.contains_key("x-zdr"),
        body: serde_json::from_slice(&body).unwrap_or(Value::Null),
    };
    let delay = *state.delay.lock().unwrap_or_else(PoisonError::into_inner);
    state
        .requests
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(seen.clone());
    if !delay.is_zero() {
        tokio::time::sleep(delay).await;
    }
    let reply = (state
        .responder
        .lock()
        .unwrap_or_else(PoisonError::into_inner))(&seen);
    (
        StatusCode::from_u16(reply.status).expect("status"),
        [(header::CONTENT_TYPE, "application/json")],
        reply.body,
    )
}

/// A client wired the way a service wires it, against `token_url`.
pub fn client(
    token_url: &str,
    golden: &Golden,
    service_id: &str,
    posture: RetentionPosture,
    clock: Arc<ManualClock>,
) -> InferenceTokenClient {
    InferenceTokenClient::with_clock(
        TokenClientConfig {
            token_url: token_url.to_owned(),
            expected_issuer: golden.issuer(),
            service_id: service_id.to_owned(),
            service_api_key: SERVICE_KEY.to_owned(),
            posture,
            allow_unconfigured: false,
        },
        clock,
    )
    .expect("client")
}
