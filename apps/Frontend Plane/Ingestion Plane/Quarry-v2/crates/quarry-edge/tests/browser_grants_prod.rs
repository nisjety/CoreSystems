//! The production browser-grant path (WS1.A2), end to end inside the edge's own
//! process: the real router and token verification (`support/agent_harness.rs`),
//! the real `GrpcGrantValidator` talking gRPC to a fake BrowserBroker, and the real
//! `AuthCoreServiceTokenProvider` minting against a fake Auth Core, with grants
//! required (`require_browser_grants`, which `ENVIRONMENT=production` implies).
//!
//! What each test pins:
//!
//! - `prod_start_run_without_grant_is_403`: production never starts a browser
//!   without a grant, and does not ask the broker about a grant that is not there.
//! - `prod_start_run_with_active_grant_overrides_client_domains`: the broker's
//!   hosts, not the caller's, are the run's hosts.
//! - `prod_step_with_revoked_grant_is_403_not_5xx`: a grant revoked or expired
//!   after the start ends the next step with 403 (not a 502), an unreachable broker
//!   fails closed, and another organization's grant is not found.
//! - `prod_validator_mints_org_scoped_browser_broker_token`: every broker call
//!   carries a token minted for the run's own organization, `browser:read` only.
//! - `prod_boot_refuses_http_only_validator` and
//!   `prod_boot_refuses_grpc_validator_without_service_tokens`: the real binary
//!   refuses to start (the pure rules are unit-tested in `src/grant_boot.rs`).
//!
//! The fake broker is the generated server half of `browser.proto`; the Go broker's
//! own org resolution (`GetScoped(grant, org)`) is modelled by keying its grants
//! on the organization of the bearer.

#![cfg(feature = "browser-agent")]

#[path = "support/agent_harness.rs"]
mod agent_harness;
mod support;

use agent_harness::*;
use serde_json::{json, Value};

const EXAMPLE_ORG: &str = "org-standard-golden";
const STRANGER_ORG: &str = "org-stranger";
/// The credential of the `quarry-edge` service principal: a dummy test value.
const SERVICE_KEY: &str = "test-credential-of-quarry-edge";

// ── the boot refusals, against the real binary ─────────────────────────────

mod boot {
    use std::io::Read;
    use std::process::{Child, Command, ExitStatus, Stdio};
    use std::time::{Duration, Instant};

    const BIN: &str = env!("CARGO_BIN_EXE_quarry-edge-rs");

    /// The edge started with exactly `settings` and no other `QUARRY*` /
    /// `ENVIRONMENT` / `APP_ENV` variable from the ambient environment.
    fn spawn(settings: &[(&str, &str)]) -> Child {
        let mut command = Command::new(BIN);
        for (name, _) in std::env::vars() {
            if name.starts_with("QUARRY") || name == "ENVIRONMENT" || name == "APP_ENV" {
                command.env_remove(name);
            }
        }
        for (name, value) in settings {
            command.env(name, value);
        }
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the edge binary starts")
    }

    /// Waits up to `limit` for the process to exit; kills it when it does not.
    fn finish(mut child: Child, limit: Duration) -> (Option<ExitStatus>, String) {
        let deadline = Instant::now() + limit;
        let status = loop {
            match child.try_wait().expect("poll the edge") {
                Some(status) => break Some(status),
                None if Instant::now() >= deadline => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                None => std::thread::sleep(Duration::from_millis(50)),
            }
        };
        let mut output = String::new();
        for stream in [
            child.stdout.take().map(|s| Box::new(s) as Box<dyn Read>),
            child.stderr.take().map(|s| Box::new(s) as Box<dyn Read>),
        ]
        .into_iter()
        .flatten()
        {
            let mut stream = stream;
            let mut text = String::new();
            let _ = stream.read_to_string(&mut text);
            output.push_str(&text);
        }
        (status, output)
    }

    /// A production environment that passes every gate before the grant gate:
    /// the internal-auth secret production demands (a dummy of 32+ characters).
    const PRODUCTION: [(&str, &str); 2] = [
        ("ENVIRONMENT", "production"),
        (
            "QUARRY_EDGE__INTERNAL_SECRET",
            "0123456789abcdef0123456789abcdef-test-only",
        ),
    ];

    fn refused(extra: &[(&str, &str)]) -> String {
        let settings: Vec<(&str, &str)> = PRODUCTION.iter().chain(extra).copied().collect();
        let (status, output) = finish(spawn(&settings), Duration::from_secs(60));
        let status = status.unwrap_or_else(|| panic!("the edge kept running; output:\n{output}"));
        assert!(!status.success(), "the edge exited cleanly; output:\n{output}");
        output
    }

    #[test]
    fn prod_boot_refuses_http_only_validator() {
        let output = refused(&[(
            "QUARRY_EDGE__BROWSER_GRANT_VALIDATOR_URL",
            "http://broker.invalid:9090",
        )]);
        assert!(
            output.contains("production refuses the HTTP-only browser grant validator"),
            "the refusal names the rule; output:\n{output}"
        );
        assert!(
            output.contains("QUARRY_EDGE__BROWSER_GRANT_VALIDATOR_GRPC_URL"),
            "and the variable that fixes it; output:\n{output}"
        );
    }

    #[test]
    fn prod_boot_refuses_no_validator_at_all() {
        let output = refused(&[]);
        assert!(
            output.contains("no BrowserBroker validation endpoint is configured"),
            "output:\n{output}"
        );
    }

    #[cfg(feature = "grpc")]
    #[test]
    fn prod_boot_refuses_grpc_validator_without_service_tokens() {
        let output = refused(&[(
            "QUARRY_EDGE__BROWSER_GRANT_VALIDATOR_GRPC_URL",
            "http://broker.invalid:9090",
        )]);
        assert!(
            output.contains("QUARRY_EDGE__QUARRY_SERVICE_API_KEY"),
            "the refusal names the missing credential; output:\n{output}"
        );
        assert!(
            output.contains("service token minted for the run's organization"),
            "output:\n{output}"
        );
    }

    /// The control for the two refusals above: with the gRPC endpoint and the
    /// service credential the grant gate lets the boot through, and the edge then
    /// stops at the next production gate (a durable artifact backend), so the
    /// refusals are about the grant configuration and not a blanket failure.
    #[cfg(feature = "grpc")]
    #[test]
    fn prod_boot_gets_past_the_grant_gate_with_grpc_and_service_tokens() {
        let output = refused(&[
            (
                "QUARRY_EDGE__BROWSER_GRANT_VALIDATOR_GRPC_URL",
                "http://broker.invalid:9090",
            ),
            ("QUARRY_EDGE__QUARRY_SERVICE_API_KEY", super::SERVICE_KEY),
        ]);
        assert!(
            output.contains("durable runtime is required"),
            "the boot reached the durability gate; output:\n{output}"
        );
        for text in [
            "refuses the HTTP-only browser grant validator",
            "QUARRY_EDGE__QUARRY_SERVICE_API_KEY:",
            "no BrowserBroker validation endpoint",
        ] {
            assert!(!output.contains(text), "{text:?} in output:\n{output}");
        }
    }
}

// ── the production path through the real validator ─────────────────────────

#[cfg(feature = "grpc")]
mod through_the_validator {
    use std::collections::{HashMap, VecDeque};
    use std::sync::{Arc, Mutex};

    use quarry_edge::run_limits::RunLimits;
    use quarry_runtime::grpc::model_plane::v1::browser_broker_server::{
        BrowserBroker, BrowserBrokerServer,
    };
    use quarry_runtime::grpc::model_plane::v1::{
        AcquireGrantRequest, AcquireGrantResponse, BrowserHealthRequest, BrowserHealthResponse,
        RevokeGrantRequest, RevokeGrantResponse, ValidateGrantRequest, ValidateGrantResponse,
    };
    use quarry_runtime::service_tokens::AuthCoreServiceTokenProvider;
    use tonic::{Request, Response, Status};
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, Request as MockRequest, ResponseTemplate};

    use super::*;

    /// One `ValidateGrant` the broker received: the organization its bearer was
    /// minted for (`None` when the bearer is not one of the fake Auth Core's) and
    /// the grant asked about.
    pub type Seen = (Option<String>, String);

    /// The Go broker's `ValidateGrant`, from memory: the organization comes from the
    /// bearer, a grant is looked up inside that organization only (so another
    /// organization's grant is `NotFound`), and each grant has a script of answers
    /// whose last entry repeats.
    #[derive(Clone, Default)]
    pub struct FakeBroker {
        grants: Arc<Mutex<HashMap<(String, String), VecDeque<Result<ValidateGrantResponse, Status>>>>>,
        seen: Arc<Mutex<Vec<Seen>>>,
    }

    impl FakeBroker {
        /// Replace the script for `grant_id` inside `org`.
        pub fn set(&self, org: &str, grant_id: &str, answers: Vec<Result<ValidateGrantResponse, Status>>) {
            self.grants
                .lock()
                .unwrap()
                .insert((org.to_owned(), grant_id.to_owned()), answers.into());
        }

        pub fn seen(&self) -> Vec<Seen> {
            self.seen.lock().unwrap().clone()
        }
    }

    #[tonic::async_trait]
    impl BrowserBroker for FakeBroker {
        async fn acquire_grant(
            &self,
            _request: Request<AcquireGrantRequest>,
        ) -> Result<Response<AcquireGrantResponse>, Status> {
            Err(Status::unimplemented("Quarry never mints a grant"))
        }

        async fn revoke_grant(
            &self,
            _request: Request<RevokeGrantRequest>,
        ) -> Result<Response<RevokeGrantResponse>, Status> {
            Err(Status::unimplemented("Quarry never revokes a grant"))
        }

        async fn validate_grant(
            &self,
            request: Request<ValidateGrantRequest>,
        ) -> Result<Response<ValidateGrantResponse>, Status> {
            let org = request
                .metadata()
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.strip_prefix("Bearer broker-token-for-"))
                .map(str::to_owned);
            let grant_id = request.into_inner().grant_id;
            self.seen.lock().unwrap().push((org.clone(), grant_id.clone()));
            let Some(org) = org else {
                return Err(Status::unauthenticated("not a token Auth Core minted"));
            };
            let mut grants = self.grants.lock().unwrap();
            let script = grants
                .get_mut(&(org, grant_id))
                .ok_or_else(|| Status::not_found("grant not found"))?;
            let answer = if script.len() > 1 {
                script.pop_front().expect("a non-empty script")
            } else {
                script.front().cloned().expect("a script has an answer")
            };
            answer.map(Response::new)
        }

        async fn health(
            &self,
            _request: Request<BrowserHealthRequest>,
        ) -> Result<Response<BrowserHealthResponse>, Status> {
            Err(Status::unimplemented("not part of this test"))
        }
    }

    /// Auth Core's `browser-broker` mint: a token that names the organization it
    /// was minted for, and only for the registered `quarry-edge` credential.
    struct MintForOrg;

    impl wiremock::Respond for MintForOrg {
        fn respond(&self, request: &MockRequest) -> ResponseTemplate {
            let body: Value = serde_json::from_slice(&request.body).expect("a JSON mint body");
            let org = body["orgId"].as_str().expect("an orgId");
            ResponseTemplate::new(200).set_body_json(json!({
                "token": format!("broker-token-for-{org}"),
                "expiresInSeconds": 300,
            }))
        }
    }

    pub struct Production {
        pub edge: Edge,
        pub broker: FakeBroker,
        pub auth_core: MockServer,
    }

    /// An edge wired as production wires it: the gRPC validator with service
    /// tokens, grants required.
    pub async fn production_edge() -> Production {
        let auth_core = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/browser-broker/internal-token"))
            .and(header("x-service-id", "quarry-edge"))
            .and(header("x-service-api-key", SERVICE_KEY))
            .respond_with(MintForOrg)
            .mount(&auth_core)
            .await;

        let broker = FakeBroker::default();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(
            tonic::transport::Server::builder()
                .add_service(BrowserBrokerServer::new(broker.clone()))
                .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener)),
        );

        // The function `main` calls, with the tokens `main` passes in production.
        let validator = quarry_edge::grant_boot::build_grpc_validator(
            &format!("http://{address}"),
            Some(Arc::new(
                AuthCoreServiceTokenProvider::new(auth_core.uri(), SERVICE_KEY).unwrap(),
            )),
            None,
            false,
        )
        .await
        .expect("the validator connects to the fake broker");
        let edge = Edge::start_with(RunLimits::default(), move |state| {
            let mut state = state;
            state.grant_validator = Arc::new(validator);
            state.require_browser_grants = true;
            state
        })
        .await;
        Production {
            edge,
            broker,
            auth_core,
        }
    }

    /// The broker's answer for a usable run grant owned by `owner`.
    pub fn active_grant(grant_id: &str, owner: &str, domains: &[&str]) -> ValidateGrantResponse {
        ValidateGrantResponse {
            grant_id: grant_id.into(),
            active: true,
            kind: "run".into(),
            owner_id: owner.into(),
            allowed_domains: domains.iter().map(|d| (*d).to_owned()).collect(),
            ..Default::default()
        }
    }

    pub async fn mint_bodies(auth_core: &MockServer) -> Vec<Value> {
        auth_core
            .received_requests()
            .await
            .unwrap()
            .iter()
            .map(|request| serde_json::from_slice(&request.body).unwrap())
            .collect()
    }

    pub fn start_body(grant_id: &str, client_domains: Option<&[&str]>) -> Value {
        let mut body = json!({ "grant_id": grant_id });
        if let Some(domains) = client_domains {
            body["constraints"] = json!({ "max_steps": 0, "allowed_domains": domains });
        }
        body
    }
}

#[cfg(feature = "grpc")]
use through_the_validator::*;

/// Production requires a grant: a start that names none is 403 `browser grant_id
/// is required` before any browser is acquired and before the broker or Auth Core
/// is asked anything. A start that names a grant the broker does not know is 403
/// too, never an upstream error.
#[cfg(feature = "grpc")]
#[tokio::test]
async fn prod_start_run_without_grant_is_403() {
    let Production {
        edge,
        broker,
        auth_core,
    } = production_edge().await;

    let none = edge
        .post(&member_token(), "/v1/agent/runs", json!({}))
        .await;
    assert_eq!(none.status, 403, "{}", none.body);
    assert_eq!(none.message(), "browser grant_id is required");
    assert!(edge.driver.acquired().is_empty());
    assert!(broker.seen().is_empty(), "no grant, no broker call");
    assert!(mint_bodies(&auth_core).await.is_empty(), "no grant, no mint");

    let forged = edge
        .post(
            &member_token(),
            "/v1/agent/runs",
            start_body("grant-forged", None),
        )
        .await;
    assert_eq!(forged.status, 403, "{}", forged.body);
    assert_eq!(forged.message(), "browser grant is inactive or expired");
    assert!(edge.driver.acquired().is_empty());
}

/// With a usable grant the run starts, and its hosts are the broker's whatever the
/// caller asked for: a wider list, a list that does not include the broker's, and no
/// list at all.
#[cfg(feature = "grpc")]
#[tokio::test]
async fn prod_start_run_with_active_grant_overrides_client_domains() {
    let Production { edge, broker, .. } = production_edge().await;
    broker.set(
        EXAMPLE_ORG,
        "grant-hosts",
        vec![Ok(active_grant("grant-hosts", "user-golden", &["example.com"]))],
    );

    for client in [
        Some(&["example.com", "evil.test"][..]),
        Some(&["evil.test"][..]),
        None,
    ] {
        let started = edge
            .post(
                &member_token(),
                "/v1/agent/runs",
                start_body("grant-hosts", client),
            )
            .await;
        assert_eq!(started.status, 200, "client {client:?}: {}", started.body);
        let run = started.data()["run_id"].as_str().expect("a run id");
        let handle = edge.live(run).expect("the run is live");
        let entry = handle.entry.lock().await;
        assert_eq!(
            entry.constraints.allowed_domains,
            ["example.com"],
            "client {client:?}: the broker's hosts are the run's hosts"
        );
    }
}

/// A grant that is revoked (or expires) after the start ends the next step with
/// 403 `SECURITY_BLOCKED`, not a 5xx: the broker answers `FailedPrecondition` and
/// the validator reads it as an inactive grant. A broker that cannot be reached
/// fails closed with its own status (502), and the same grant presented from
/// another organization is not found (403).
#[cfg(feature = "grpc")]
#[tokio::test]
async fn prod_step_with_revoked_grant_is_403_not_5xx() {
    let Production { edge, broker, .. } = production_edge().await;
    broker.set(
        EXAMPLE_ORG,
        "grant-live",
        vec![Ok(active_grant("grant-live", "user-golden", &["example.com"]))],
    );
    let person = member_token();
    let run = edge
        .start_run(&person, start_body("grant-live", None))
        .await;

    // Still usable: the step is not stopped by the grant (the recording driver's
    // page is on no allowed host, so its own status is not asserted).
    let live = edge.step(&person, &run, wait_action(), None).await;
    assert_ne!(
        live.message(),
        "browser grant is inactive or expired",
        "{}",
        live.body
    );

    // Revoked at the broker.
    broker.set(
        EXAMPLE_ORG,
        "grant-live",
        vec![Err(tonic::Status::failed_precondition("grant revoked"))],
    );
    let revoked = edge.step(&person, &run, wait_action(), None).await;
    assert_eq!(revoked.status, 403, "{}", revoked.body);
    assert_eq!(revoked.code(), "SECURITY_BLOCKED", "{}", revoked.body);
    assert_eq!(revoked.message(), "browser grant is inactive or expired");

    // Expired at the broker answers the same.
    broker.set(
        EXAMPLE_ORG,
        "grant-live",
        vec![Err(tonic::Status::failed_precondition("grant expired"))],
    );
    let expired = edge.step(&person, &run, wait_action(), None).await;
    assert_eq!(expired.status, 403, "{}", expired.body);

    // A broker that is down is not an allow and not a 500.
    broker.set(
        EXAMPLE_ORG,
        "grant-live",
        vec![Err(tonic::Status::unavailable("broker restarting"))],
    );
    let down = edge.step(&person, &run, wait_action(), None).await;
    assert_eq!(down.status, 502, "{}", down.body);
    assert_eq!(down.code(), "DRIVER_FAILED", "{}", down.body);

    // A new run on the revoked grant is refused at the start.
    broker.set(
        EXAMPLE_ORG,
        "grant-live",
        vec![Err(tonic::Status::failed_precondition("grant revoked"))],
    );
    let again = edge
        .post(&person, "/v1/agent/runs", start_body("grant-live", None))
        .await;
    assert_eq!(again.status, 403, "{}", again.body);
    assert_eq!(again.message(), "browser grant is inactive or expired");
}

/// Every broker call carries a token minted for the organization of the run it
/// is for: the mint asks Auth Core for `browser:read` (the one scope of the
/// `quarry-edge` row), names the run's organization, and a grant of one
/// organization is not found by a user of another, because the broker looks the
/// grant up inside the token's organization.
#[cfg(feature = "grpc")]
#[tokio::test]
async fn prod_validator_mints_org_scoped_browser_broker_token() {
    let Production {
        edge,
        broker,
        auth_core,
    } = production_edge().await;
    broker.set(
        EXAMPLE_ORG,
        "grant-of-example-org",
        vec![Ok(active_grant(
            "grant-of-example-org",
            "user-golden",
            &["example.com"],
        ))],
    );

    // The owner's organization finds it; the stranger's organization does not.
    let own = edge
        .post(
            &member_token(),
            "/v1/agent/runs",
            start_body("grant-of-example-org", None),
        )
        .await;
    assert_eq!(own.status, 200, "{}", own.body);
    let foreign = edge
        .post(
            &stranger_token(),
            "/v1/agent/runs",
            start_body("grant-of-example-org", None),
        )
        .await;
    assert_eq!(foreign.status, 403, "{}", foreign.body);
    assert_eq!(foreign.message(), "browser grant is inactive or expired");
    assert!(
        edge.driver.acquired().len() == 1,
        "no browser is acquired for the other organization's call"
    );

    assert_eq!(
        broker.seen(),
        vec![
            (Some(EXAMPLE_ORG.to_owned()), "grant-of-example-org".to_owned()),
            (Some(STRANGER_ORG.to_owned()), "grant-of-example-org".to_owned()),
        ],
        "each call is made with the token minted for its own organization"
    );
    let mints = mint_bodies(&auth_core).await;
    assert_eq!(mints.len(), 2, "one mint per organization");
    for (body, org) in mints.iter().zip([EXAMPLE_ORG, STRANGER_ORG]) {
        assert_eq!(body["orgId"], org);
        assert_eq!(body["scopes"], json!(["browser:read"]), "never browser:write");
        assert_eq!(body["reason"], "validate verified browser grant");
        assert!(body.get("zdr").is_none(), "the request names no posture: {body}");
    }

    // A second check for the same organization reuses its token: the cache is
    // keyed by organization, so no third mint and no sharing between tenants.
    let run = own.data()["run_id"].as_str().expect("a run id").to_owned();
    let _ = edge.step(&member_token(), &run, wait_action(), None).await;
    assert_eq!(mint_bodies(&auth_core).await.len(), 2);
    assert_eq!(broker.seen().len(), 3);
    assert_eq!(broker.seen()[2].0.as_deref(), Some(EXAMPLE_ORG));
}
