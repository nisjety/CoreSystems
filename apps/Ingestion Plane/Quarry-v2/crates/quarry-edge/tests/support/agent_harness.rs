//! The real-router harness for the agent-run control and sweeper tests.
//!
//! What it runs: `quarry_edge::routes::router` on a loopback socket, so every
//! request passes the real `require_auth` (RS256 against a JWKS endpoint, with
//! the deployed `aud` and `iss`), the real `require_service_route_scope`, the
//! real agent handlers and the real `retry_after_header` layer. The claim sets
//! are the golden Auth Core mints; the token helpers below re-sign one of them
//! with the fixture key and change only the fields a test needs.
//!
//! What it does not run: Chromium. `RecordingDriver` starts a run and answers
//! every operation a run needs from memory, and it records each lease it
//! acquires and releases, keyed by the run id (the lease's session affinity key).
//!
//! Declared with `#[path = "support/agent_harness.rs"] mod agent_harness;` so
//! `support/mod.rs` is not edited; `base_state` is reached through `crate::support`.

#![allow(dead_code)]

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use reqwest::header::HeaderMap;
use serde_json::{json, Value};
use tokio::sync::mpsc;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use quarry_browser::{
    BrowserDriver, BrowserDriverCapabilities, BrowserEgressPolicy, BrowserSession, BrowserTab,
    LiveFrame, LiveFrameOptions, SessionInner,
};
use quarry_core::error::{ErrorCode, QuarryError};
use quarry_core::event::{Event, EventType};
use quarry_core::ids::kinds::RunKind;
use quarry_core::lease::BrowserLease;
use quarry_core::QuarryResult;
use quarry_edge::agent_routes::{new_runs_with, now_ms, RunHandle};
use quarry_edge::run_limits::{RunLimits, ENV_TAKEOVER_ENFORCEMENT};
use quarry_edge::state::AppState;
use quarry_runtime::fetch::StaticDriver;
use quarry_runtime::EventSink;

const GOLDEN: &str = include_str!("../fixtures/auth-core/quarry-token-claims.golden.json");

/// The deployed values: `AUTH_CORE_AUDIENCE` and `AUTH_CORE_ISSUER` of the
/// quarry-edge service in `apps/Ingestion Plane/docker-compose.yml`.
const DEPLOYED_AUDIENCE: &str = "quarry";
const DEPLOYED_ISSUER: &str = "http://localhost:3011/api/convex-auth";
const KID: &str = "agent-run-harness-kid";

const PAGE: &[u8] =
    b"<html><head><title>Recorded test page</title></head><body><p>A fixed page.</p></body></html>";

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

        // The JWKS endpoint lives on its own runtime so it outlives the per-test
        // runtimes `#[tokio::test]` creates.
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

        // Exactly the deployed verification configuration, and no dev bypass.
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

/// A token whose claims are the golden case `case`, re-signed RS256 with the
/// fixture key and stamped with fresh time claims. `tweak` edits the claim map.
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

/// A member of the standard org (`user-golden` in `org-standard-golden`).
pub fn member_token() -> String {
    token("user_in_standard_org", |_| {})
}

/// Another member of the same standard org.
pub fn other_member_token() -> String {
    token("user_in_standard_org", |claims| {
        claims.insert("sub".into(), json!("user-other"));
        claims.insert("user_id".into(), json!("user-other"));
    })
}

/// A member of a second organization.
pub fn stranger_token() -> String {
    token("user_in_standard_org", |claims| {
        claims.insert("org_id".into(), json!("org-stranger"));
        claims.insert("sub".into(), json!("user-stranger"));
        claims.insert("user_id".into(), json!("user-stranger"));
    })
}

/// A member of the ZDR org (`zdr: true` from the org's posture).
pub fn zdr_member_token() -> String {
    token("user_in_zdr_org", |_| {})
}

/// execution-core: a service principal holding `browser:execute`, in the standard org.
pub fn service_token() -> String {
    token("service_execution_core", |_| {})
}

/// A service principal without `browser:execute` (it holds scrape and search scopes
/// only), so the browser routes refuse it.
pub fn model_gateway_service_token() -> String {
    token("service_model_gateway", |_| {})
}

// ── browser driver double ───────────────────────────────────────────────────

/// A browser driver that starts runs and remembers what it was asked to do.
///
/// Every operation a run needs succeeds in memory, so an observation completes;
/// nothing here launches a browser. `acquire` and `release` are recorded by the
/// session's affinity key (the run id), so a test can count releases per run.
#[derive(Default)]
pub struct RecordingDriver {
    acquired: Mutex<Vec<String>>,
    released: Mutex<Vec<String>>,
    tab_urls: Mutex<Vec<Option<String>>>,
    tab_ids: Mutex<Vec<String>>,
    refuse_next_acquire: AtomicBool,
    tab_seq: AtomicU32,
    tab_selects: Mutex<Vec<String>>,
    tab_closes: Mutex<Vec<String>>,
    evaluate_calls: AtomicU32,
    /// How the next `goto` holds before it fails with the deadline `Timeout`.
    timeout_next_goto: Mutex<Option<GotoHold>>,
    /// How many `goto` calls have started (the call is then running, so the step that made
    /// it holds the run lock): tests wait on this instead of sleeping and hoping.
    goto_entered: AtomicU32,
}

/// What ends a held `goto`: a fixed time, or a gate the test opens itself.
enum GotoHold {
    After(Duration),
    Gate(Arc<tokio::sync::Notify>),
}

impl RecordingDriver {
    /// Run ids, one per successful `acquire`, in order.
    pub fn acquired(&self) -> Vec<String> {
        self.acquired.lock().unwrap().clone()
    }

    /// Run ids, one per `release`, in order.
    pub fn released(&self) -> Vec<String> {
        self.released.lock().unwrap().clone()
    }

    /// How many tabs were opened on the driver.
    pub fn tabs_opened(&self) -> usize {
        self.tab_urls.lock().unwrap().len()
    }

    /// Tab ids selected on the driver, in order.
    pub fn selected_tabs(&self) -> Vec<String> {
        self.tab_selects.lock().unwrap().clone()
    }

    /// Tab ids closed on the driver, in order.
    pub fn closed_tabs(&self) -> Vec<String> {
        self.tab_closes.lock().unwrap().clone()
    }

    /// Make a tab disappear the way a page closing itself or the user closing it in the
    /// browser does: it is no longer listed and no close call was made, so `closed_tabs`
    /// does not record it.
    pub fn vanish_tab(&self, tab_id: &str) {
        self.tab_ids.lock().unwrap().retain(|id| id != tab_id);
    }

    /// How many times the driver was asked to run page script (`evaluate`).
    pub fn evaluate_calls(&self) -> u32 {
        self.evaluate_calls.load(Ordering::SeqCst)
    }

    /// Make the next `goto` fail the way the real driver's navigation deadline does: a
    /// typed `Timeout` naming `navigate`, effect unknown, not retryable. It fails at once.
    pub fn time_out_next_goto(&self) {
        self.time_out_next_goto_after(Duration::ZERO);
    }

    /// Like [`Self::time_out_next_goto`], but the call is still running (it holds the run
    /// lock and the in-flight count of the step that made it) for `hold` before the
    /// deadline fires, the way a navigation stuck against its deadline does.
    pub fn time_out_next_goto_after(&self, hold: Duration) {
        *self.timeout_next_goto.lock().unwrap() = Some(GotoHold::After(hold));
    }

    /// Like [`Self::time_out_next_goto_after`], but the call is held until the test calls
    /// `notify_one` on the returned gate (the permit is kept, so opening it early is fine).
    /// Nothing depends on a clock: the call is running until the test says otherwise.
    pub fn time_out_next_goto_when_released(&self) -> Arc<tokio::sync::Notify> {
        let gate = Arc::new(tokio::sync::Notify::new());
        *self.timeout_next_goto.lock().unwrap() = Some(GotoHold::Gate(gate.clone()));
        gate
    }

    /// How many `goto` calls have started on the driver.
    pub fn goto_entered(&self) -> u32 {
        self.goto_entered.load(Ordering::SeqCst)
    }

    /// Wait (at most 20 s, then panic with `what`) until `goto_entered` is at least `count`.
    pub async fn wait_goto_entered(&self, count: u32, what: &str) {
        let waited = tokio::time::timeout(Duration::from_secs(20), async {
            while self.goto_entered() < count {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await;
        assert!(waited.is_ok(), "the driver never reached goto #{count}: {what}");
    }

    /// Make the next `acquire` fail as a driver would.
    pub fn refuse_next_acquire(&self) {
        self.refuse_next_acquire.store(true, Ordering::SeqCst);
    }
}

#[async_trait]
impl BrowserDriver for RecordingDriver {
    fn capabilities(&self) -> BrowserDriverCapabilities {
        BrowserDriverCapabilities {
            // What `start_run`'s admission gate needs, and what the browser socket
            // needs before it streams frames; everything else stays at the
            // deny-by-default value.
            isolated_egress: true,
            security_evidence: true,
            persistent_profile: true,
            full_visual_fidelity: true,
            ..BrowserDriverCapabilities::default()
        }
    }

    async fn acquire(&self, lease: &BrowserLease) -> QuarryResult<BrowserSession> {
        if self.refuse_next_acquire.swap(false, Ordering::SeqCst) {
            return Err(QuarryError::new(
                ErrorCode::DriverFailed,
                "the test driver refused the lease",
            ));
        }
        self.acquired
            .lock()
            .unwrap()
            .push(lease.session_affinity_key.clone());
        Ok(BrowserSession {
            lease: lease.clone(),
            inner: Arc::new(tokio::sync::Mutex::new(SessionInner::default())),
        })
    }

    async fn release(&self, session: BrowserSession) -> QuarryResult<()> {
        self.released
            .lock()
            .unwrap()
            .push(session.lease.session_affinity_key.clone());
        Ok(())
    }

    async fn configure_egress_policy(
        &self,
        _session: &BrowserSession,
        _policy: BrowserEgressPolicy,
    ) -> QuarryResult<()> {
        Ok(())
    }

    async fn goto(&self, _session: &BrowserSession, _url: &str) -> QuarryResult<()> {
        self.goto_entered.fetch_add(1, Ordering::SeqCst);
        let hold = self.timeout_next_goto.lock().unwrap().take();
        if let Some(hold) = hold {
            match hold {
                GotoHold::After(hold) if hold.is_zero() => {}
                GotoHold::After(hold) => tokio::time::sleep(hold).await,
                GotoHold::Gate(gate) => gate.notified().await,
            }
            let mut error = QuarryError::new(ErrorCode::Timeout, "browser operation navigate timed out")
                .with_details(json!({ "op": "navigate", "deadline_ms": 30_000, "effect": "unknown" }));
            error.retryable = false;
            return Err(error);
        }
        Ok(())
    }

    async fn content(&self, _session: &BrowserSession) -> QuarryResult<Bytes> {
        Ok(Bytes::from_static(PAGE))
    }

    async fn screenshot(&self, _session: &BrowserSession, _full_page: bool) -> QuarryResult<Bytes> {
        Ok(Bytes::from_static(b"\x89PNG recorded"))
    }

    /// The browser socket streams a frame on every tick; the frame's content is not
    /// what these tests look at.
    async fn live_frame(
        &self,
        _session: &BrowserSession,
        _options: LiveFrameOptions,
    ) -> QuarryResult<LiveFrame> {
        Ok(LiveFrame {
            mime_type: "image/png".into(),
            data_base64: "cmVjb3JkZWQ=".into(),
        })
    }

    async fn pdf(&self, _session: &BrowserSession) -> QuarryResult<Bytes> {
        Ok(Bytes::from_static(b"%PDF recorded"))
    }

    async fn list_tabs(&self, _session: &BrowserSession) -> QuarryResult<Vec<BrowserTab>> {
        let mut tabs = vec![BrowserTab {
            tab_id: "tab_home".into(),
            title: None,
            url: None,
            active: true,
        }];
        for tab_id in self.tab_ids.lock().unwrap().iter() {
            tabs.push(BrowserTab {
                tab_id: tab_id.clone(),
                title: None,
                url: None,
                active: false,
            });
        }
        Ok(tabs)
    }

    async fn new_tab(
        &self,
        _session: &BrowserSession,
        url: Option<&str>,
    ) -> QuarryResult<BrowserTab> {
        self.tab_urls.lock().unwrap().push(url.map(str::to_owned));
        let tab_id = format!("tab_{}", self.tab_seq.fetch_add(1, Ordering::SeqCst) + 1);
        self.tab_ids.lock().unwrap().push(tab_id.clone());
        Ok(BrowserTab {
            tab_id,
            title: None,
            url: url.map(str::to_owned),
            active: true,
        })
    }

    /// Counts the call and answers, so a test can prove the edge never got this far.
    async fn evaluate(
        &self,
        _session: &BrowserSession,
        _script: &str,
    ) -> QuarryResult<Value> {
        self.evaluate_calls.fetch_add(1, Ordering::SeqCst);
        Ok(Value::Null)
    }

    async fn select_tab(&self, _session: &BrowserSession, tab_id: &str) -> QuarryResult<()> {
        self.tab_selects.lock().unwrap().push(tab_id.to_owned());
        Ok(())
    }

    async fn close_tab(&self, _session: &BrowserSession, tab_id: &str) -> QuarryResult<()> {
        self.tab_closes.lock().unwrap().push(tab_id.to_owned());
        // A closed tab is no longer listed, as with a real browser.
        self.tab_ids.lock().unwrap().retain(|id| id != tab_id);
        Ok(())
    }
}

// ── the edge ────────────────────────────────────────────────────────────────

/// A running edge: the real router on a loopback port, plus handles on the
/// state it shares with that router (the registry, the receipt store, the
/// driver double and the event sink).
pub struct Edge {
    pub addr: SocketAddr,
    pub client: reqwest::Client,
    /// The same `AppState` the router serves; clones share every `Arc`.
    pub state: AppState,
    pub driver: Arc<RecordingDriver>,
    durable: Arc<Mutex<Vec<(EventType, Value)>>>,
}

/// One response: its status, headers and JSON body (`Null` when there is none).
#[derive(Debug)]
pub struct Reply {
    pub status: u16,
    pub headers: HeaderMap,
    pub body: Value,
}

impl Reply {
    /// `error.code`, or `""` for a success.
    pub fn code(&self) -> &str {
        self.body["error"]["code"].as_str().unwrap_or("")
    }

    /// `error.details`, or `Null`.
    pub fn details(&self) -> &Value {
        &self.body["error"]["details"]
    }

    /// `error.message`, or `""`.
    pub fn message(&self) -> &str {
        self.body["error"]["message"].as_str().unwrap_or("")
    }

    /// `data`, or `Null`.
    pub fn data(&self) -> &Value {
        &self.body["data"]
    }
}

impl Edge {
    /// The router with the given run limits, and the default driver double.
    pub async fn start(limits: RunLimits) -> Self {
        Self::start_with(limits, |state| state).await
    }

    /// As [`Edge::start`], with `configure` applied to the state before the
    /// router is built.
    pub async fn start_with(
        limits: RunLimits,
        configure: impl FnOnce(AppState) -> AppState,
    ) -> Self {
        fixture();

        let durable: Arc<Mutex<Vec<(EventType, Value)>>> = Arc::default();
        let (tx, mut rx) = mpsc::channel::<Event>(1024);
        let sink = durable.clone();
        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                sink.lock().unwrap().push((event.event_type, event.payload));
            }
        });

        let driver = Arc::new(RecordingDriver::default());
        let agent_driver: Arc<dyn BrowserDriver> = driver.clone();
        let static_driver = Arc::new(
            StaticDriver::new(Duration::from_secs(5), "agent-run-harness").expect("static driver"),
        );
        let base = crate::support::base_state(static_driver, EventSink::new(tx));
        let state = configure(AppState {
            agent_driver,
            agent_runs: new_runs_with(limits),
            ..base
        });

        let app = quarry_edge::routes::router(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind loopback");
        let addr = listener.local_addr().expect("local addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });

        Self {
            addr,
            client: reqwest::Client::new(),
            state,
            driver,
            durable,
        }
    }

    fn url(&self, route: &str) -> String {
        format!("http://{}{route}", self.addr)
    }

    pub async fn send(
        &self,
        verb: reqwest::Method,
        bearer: &str,
        route: &str,
        body: Option<Value>,
    ) -> Reply {
        let mut request = self
            .client
            .request(verb, self.url(route))
            .bearer_auth(bearer);
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.expect("request reaches the edge");
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let text = response.text().await.expect("response body");
        let body = if text.trim().is_empty() {
            Value::Null
        } else {
            serde_json::from_str(&text).unwrap_or(Value::String(text))
        };
        Reply {
            status,
            headers,
            body,
        }
    }

    pub async fn post(&self, bearer: &str, route: &str, body: Value) -> Reply {
        self.send(reqwest::Method::POST, bearer, route, Some(body))
            .await
    }

    pub async fn get(&self, bearer: &str, route: &str) -> Reply {
        self.send(reqwest::Method::GET, bearer, route, None).await
    }

    pub async fn delete(&self, bearer: &str, route: &str) -> Reply {
        self.send(reqwest::Method::DELETE, bearer, route, None)
            .await
    }

    /// Open a stream and hand back the live response. The caller keeps it alive
    /// for as long as the stream should count as a watching person.
    pub async fn open_stream(&self, bearer: &str, route: &str) -> reqwest::Response {
        let response = self
            .client
            .get(self.url(route))
            .bearer_auth(bearer)
            .send()
            .await
            .expect("stream request reaches the edge");
        assert_eq!(response.status().as_u16(), 200, "stream refused");
        response
    }

    /// Start a run and return its id; panics unless the start succeeded.
    pub async fn start_run(&self, bearer: &str, body: Value) -> String {
        let reply = self.post(bearer, "/v1/agent/runs", body).await;
        assert_eq!(reply.status, 200, "start refused: {}", reply.body);
        reply.data()["run_id"]
            .as_str()
            .expect("start answers a run_id")
            .to_owned()
    }

    /// One action on a run. `hint` is the body's `initiated_by` (`"human"` or
    /// `"agent"`), omitted when `None`.
    pub async fn step(
        &self,
        bearer: &str,
        run_id: &str,
        action: Value,
        hint: Option<&str>,
    ) -> Reply {
        let mut body = json!({ "action": action });
        if let Some(hint) = hint {
            body["initiated_by"] = json!(hint);
        }
        self.post(bearer, &format!("/v1/agent/runs/{run_id}/step"), body)
            .await
    }

    /// Open a tab on a run; `initiated_by` is the body's `initiatedBy`.
    pub async fn new_tab(&self, bearer: &str, run_id: &str, initiated_by: Option<&str>) -> Reply {
        let mut body = json!({});
        if let Some(initiated_by) = initiated_by {
            body["initiatedBy"] = json!(initiated_by);
        }
        self.post(bearer, &format!("/v1/agent/runs/{run_id}/tabs"), body)
            .await
    }

    pub async fn control(&self, bearer: &str, run_id: &str, body: Value) -> Reply {
        self.post(
            bearer,
            &format!("/v1/agent/runs/{run_id}/browser-session/control"),
            body,
        )
        .await
    }

    pub async fn projection(&self, bearer: &str, run_id: &str) -> Reply {
        self.get(bearer, &format!("/v1/agent/runs/{run_id}/browser-session"))
            .await
    }

    /// The owner's receipt stream, as the route returns it. Panics unless 200.
    pub async fn receipts(&self, bearer: &str, run_id: &str) -> Vec<Value> {
        let reply = self
            .get(bearer, &format!("/v1/agent/runs/{run_id}/receipts"))
            .await;
        assert_eq!(reply.status, 200, "receipts refused: {}", reply.body);
        reply.data().as_array().cloned().unwrap_or_default()
    }

    /// The owner's timeline items, as the route returns them. Panics unless 200.
    pub async fn timeline(&self, bearer: &str, run_id: &str) -> Vec<Value> {
        let reply = self
            .get(
                bearer,
                &format!("/v1/agent/runs/{run_id}/browser-session/timeline"),
            )
            .await;
        assert_eq!(reply.status, 200, "timeline refused: {}", reply.body);
        reply.data()["items"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }

    /// The live handle for a run, if the registry still holds it. The lock is
    /// released before this returns.
    pub fn live(&self, run_id: &str) -> Option<RunHandle> {
        self.state
            .agent_runs
            .lock()
            .expect("agent_runs mutex")
            .get(run_id)
            .cloned()
    }

    /// The durable event channel's contents so far. The drain runs on a spawned
    /// task, so this waits briefly for it.
    pub async fn durable_events(&self) -> Vec<(EventType, Value)> {
        tokio::time::sleep(Duration::from_millis(100)).await;
        self.durable.lock().unwrap().clone()
    }
}

// ── the browser socket ──────────────────────────────────────────────────────

/// A client for the browser WebSocket, over a plain TCP stream. It speaks just enough
/// of RFC 6455 for these tests: a masked text frame out, unmasked frames in, no
/// extensions and no fragmentation. The handshake is checked by its status line only.
pub struct TestSocket {
    stream: tokio::net::TcpStream,
}

impl TestSocket {
    /// Upgrade a GET on `route` as `bearer`. Panics unless the edge answers 101.
    pub async fn connect(edge: &Edge, bearer: &str, route: &str) -> Self {
        use base64::Engine;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let mut stream = tokio::net::TcpStream::connect(edge.addr)
            .await
            .expect("the socket connects");
        // The key only has to be a 16-byte nonce in base64; the edge does not check it.
        let key = base64::engine::general_purpose::STANDARD.encode([0x5a_u8; 16]);
        let request = format!(
            "GET {route} HTTP/1.1\r\nHost: {addr}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
             Sec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\
             Authorization: Bearer {bearer}\r\n\r\n",
            addr = edge.addr,
        );
        stream
            .write_all(request.as_bytes())
            .await
            .expect("the handshake is sent");
        let mut head = Vec::new();
        while !head.ends_with(b"\r\n\r\n") {
            head.push(stream.read_u8().await.expect("the handshake is answered"));
        }
        let head = String::from_utf8_lossy(&head).into_owned();
        assert!(
            head.starts_with("HTTP/1.1 101"),
            "the upgrade was refused: {head}"
        );
        Self { stream }
    }

    /// Send one text message, masked as a client must mask it.
    pub async fn send_text(&mut self, text: &str) {
        use tokio::io::AsyncWriteExt;

        let payload = text.as_bytes();
        let mut frame = vec![0x81];
        match payload.len() {
            len @ 0..=125 => frame.push(0x80 | len as u8),
            len @ 126..=65_535 => {
                frame.push(0x80 | 126);
                frame.extend_from_slice(&(len as u16).to_be_bytes());
            }
            len => {
                frame.push(0x80 | 127);
                frame.extend_from_slice(&(len as u64).to_be_bytes());
            }
        }
        let mask = [0x37, 0xfa, 0x21, 0x3d];
        frame.extend_from_slice(&mask);
        frame.extend(
            payload
                .iter()
                .enumerate()
                .map(|(index, byte)| byte ^ mask[index % 4]),
        );
        self.stream
            .write_all(&frame)
            .await
            .expect("the frame is sent");
    }

    /// The next text message the edge sends, parsed. Panics if the socket closes.
    pub async fn next_json(&mut self) -> Value {
        use tokio::io::AsyncReadExt;

        loop {
            let first = self.stream.read_u8().await.expect("a frame header");
            let second = self.stream.read_u8().await.expect("a frame header");
            let length = match second & 0x7f {
                126 => usize::from(self.stream.read_u16().await.expect("a frame length")),
                127 => self.stream.read_u64().await.expect("a frame length") as usize,
                short => usize::from(short),
            };
            let mut payload = vec![0u8; length];
            self.stream
                .read_exact(&mut payload)
                .await
                .expect("a frame payload");
            match first & 0x0f {
                0x1 => return serde_json::from_slice(&payload).expect("a JSON text frame"),
                0x8 => panic!("the edge closed the browser socket"),
                _ => {} // ping, pong or binary: none of these tests use them
            }
        }
    }

    /// The next message that `wanted` accepts, within five seconds. The frames the edge
    /// streams in the meantime are skipped.
    pub async fn next_matching(&mut self, wanted: impl Fn(&Value) -> bool) -> Value {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let message = self.next_json().await;
                if wanted(&message) {
                    return message;
                }
            }
        })
        .await
        .expect("the edge answered within five seconds")
    }
}

/// Set or clear the takeover-enforcement switch for the next requests. Tests that
/// call this must be `#[serial]`.
pub fn set_takeover_enforcement(value: Option<&str>) {
    match value {
        Some(value) => std::env::set_var(ENV_TAKEOVER_ENFORCEMENT, value),
        None => std::env::remove_var(ENV_TAKEOVER_ENFORCEMENT),
    }
}

/// `{"type": "wait", "ms": 0}`: a valid action that needs no page and no network.
pub fn wait_action() -> Value {
    json!({ "type": "wait", "ms": 0 })
}

/// The run id as a typed run kind, for the store.
pub fn kind(run_id: &str) -> RunKind {
    run_id.parse().expect("a run id this edge issued")
}

/// A clock reading `ms` after now, for the sweeper's injected `now`.
pub fn later(ms: u64) -> u64 {
    now_ms() + ms
}
