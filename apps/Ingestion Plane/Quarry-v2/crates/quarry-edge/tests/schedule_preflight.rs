//! `POST /v1/internal/schedules/preflight` (WS15.S2).
//!
//! The orchestrator asks this route before every scheduled fire. S2 answers allow
//! with the posture `unchecked` and the entitlement `floor`. Since WS15.S4a
//! (W2-Q-S4a) that answer is mode `off`; the default mode `observe` reads the
//! org-core posture (covered by the unit tests in `src/schedule_preflight.rs`).
//! The mode is read once per process, so every test here pins `off` before the
//! first request and none can see another mode. The route needs the
//! shared runtime bearer token and the `X-Quarry-Run-Sig` binding over
//! `org_id + "\n" + "schedule:<id>"`, so a token or a signature cannot be reused
//! for another schedule or another tenant.
//!
//! Written in the build wave and compile-checked only (`cargo check --tests`).
//! Not run yet: the final test phase runs it.

use std::{net::SocketAddr, sync::Arc};

use hmac::{Hmac, Mac};
use quarry_edge::{internal_auth::InternalSigner, state::AppState};
use quarry_runtime::EventSink;
use serde_json::{json, Value};
use sha2::Sha256;
use tokio::sync::mpsc;

mod support;

use support::fixture_site::FixtureSiteDriver;

const RUNTIME_TOKEN: &str = "schedule-preflight-runtime-token";
const INTERNAL_SECRET: &str = "schedule-preflight-internal-secret";
const ORG: &str = "org_schedule_preflight";
const SCHEDULE: &str = "sch_preflight_test";

type HmacSha256 = Hmac<Sha256>;

/// `X-Quarry-Run-Sig` for a target: hex(HMAC-SHA256(secret, org_id + "\n" + target)).
fn binding_sig(org: &str, target: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(INTERNAL_SECRET.as_bytes()).unwrap();
    mac.update(format!("{org}\n{target}").as_bytes());
    format!("{:x}", mac.finalize().into_bytes())
}

async fn spawn_edge() -> SocketAddr {
    std::env::set_var("QUARRY_EDGE__RUNTIME_AUTH_TOKEN", RUNTIME_TOKEN);
    // The S2 answer is mode `off` since S4a; same value in every test (read once).
    std::env::set_var(quarry_edge::schedule_preflight::ENV_SCHEDULE_POSTURE, "off");
    let (tx, mut rx) = mpsc::channel(64);
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let driver = Arc::new(FixtureSiteDriver::standard_site());
    let state = AppState {
        internal_signer: Some(Arc::new(InternalSigner::new(INTERNAL_SECRET).unwrap())),
        ..support::base_state(driver, EventSink::new(tx))
    };
    let app = quarry_edge::routes::router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    addr
}

async fn preflight(
    addr: SocketAddr,
    token: Option<&str>,
    sig: Option<String>,
    body: &Value,
) -> (u16, Value) {
    let mut req = reqwest::Client::new()
        .post(format!("http://{addr}/v1/internal/schedules/preflight"))
        .json(body);
    if let Some(t) = token {
        req = req.bearer_auth(t);
    }
    if let Some(s) = sig {
        req = req.header("x-quarry-run-sig", s);
    }
    let response = req.send().await.unwrap();
    let status = response.status().as_u16();
    let json = response.json().await.unwrap_or(Value::Null);
    (status, json)
}

#[tokio::test]
async fn preflight_allow_in_s2() {
    let addr = spawn_edge().await;
    let body = json!({ "schedule_id": SCHEDULE, "org_id": ORG });
    let sig = binding_sig(ORG, &format!("schedule:{SCHEDULE}"));

    let (status, answer) = preflight(addr, Some(RUNTIME_TOKEN), Some(sig), &body).await;
    assert_eq!(status, 200, "answer={answer}");
    assert_eq!(answer["allow"], json!(true));
    assert_eq!(answer["posture"], json!("unchecked"));
    assert_eq!(answer["entitlement"], json!("floor"));
}

#[tokio::test]
async fn preflight_requires_runtime_token_and_binding() {
    let addr = spawn_edge().await;
    let body = json!({ "schedule_id": SCHEDULE, "org_id": ORG });
    let good = binding_sig(ORG, &format!("schedule:{SCHEDULE}"));

    // No runtime bearer token: refused, even with a valid signature.
    let (no_token, _) = preflight(addr, None, Some(good.clone()), &body).await;
    assert_eq!(no_token, 401, "a missing runtime token must be refused");

    // Runtime token but no run binding: refused when the edge has a secret.
    let (no_sig, _) = preflight(addr, Some(RUNTIME_TOKEN), None, &body).await;
    assert_eq!(no_sig, 401, "a missing binding must be refused");

    // A signature for another schedule of the same tenant does not verify.
    let other_schedule = binding_sig(ORG, "schedule:sch_other");
    let (wrong_schedule, _) =
        preflight(addr, Some(RUNTIME_TOKEN), Some(other_schedule), &body).await;
    assert_eq!(wrong_schedule, 401, "a signature for another schedule must be refused");

    // A signature for another tenant does not verify either.
    let other_org = binding_sig("org_other", &format!("schedule:{SCHEDULE}"));
    let (wrong_org, _) = preflight(addr, Some(RUNTIME_TOKEN), Some(other_org), &body).await;
    assert_eq!(wrong_org, 401, "a signature for another tenant must be refused");

    // The right token and the right binding are allowed.
    let (ok, answer) = preflight(addr, Some(RUNTIME_TOKEN), Some(good), &body).await;
    assert_eq!(ok, 200, "answer={answer}");
}
