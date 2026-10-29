//! ZDR (Zero Data Retention) behavior tests for Quarry-edge's own HTTP
//! surface, independent of the Verevon gateway's `effective_profile_scope`
//! guard (`fix(gateway): close ZDR bypass via explicit ephemeral scope
//! claim`, `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/browser.rs`).
//!
//! That gateway fix is a *proxy-side* guard: it rejects a client-supplied
//! `{zdr: true, profileId: "<real>", scope: "ephemeral"}` combination before
//! ever forwarding the request to Quarry-edge. But Quarry-edge is the layer
//! that actually launches the browser and calls `ProfileStore::save` on
//! `close_run` → `agent_driver.release()` — any *other* direct caller of
//! `POST /v1/agent/runs` (a different internal consumer, a future service, a
//! gateway bug) must not be able to make a ZDR run's cookies/storage durable
//! just by supplying a `profile_id` or `persist_profile: true`. Before this
//! fix, `persist_profile = body.persist_profile || body.profile_id.is_some()`
//! ignored `zdr` entirely — a real, previously-undiscovered defense-in-depth
//! gap in `agent_routes.rs::start_run`, confirmed by reading
//! `quarry_core::lease::BrowserLease` (has no `zdr` field at all) and
//! `ChromiumoxideDriver::persist_current_page` (gated only on
//! `lease.persist_profile`, never on ZDR).
//!
//! These tests exercise only the rejection path — the guard runs and returns
//! before `state.agent_driver.acquire()` is ever called, so no real Chromium
//! binary is required to run them (unlike the `CHROMIUMOXIDE_TEST=1`-gated
//! tests in `quarry-browser/src/chromiumoxide.rs`).

use std::{net::SocketAddr, sync::Arc, time::Duration};

use quarry_edge::state::AppState;
use quarry_runtime::{fetch::StaticDriver, EventSink};
use serde_json::Value;
use tokio::sync::mpsc;

mod support;

const TEST_BEARER: &str = "zdr-behavior-dev-token";

async fn spawn_app() -> SocketAddr {
    std::env::set_var("QUARRY_EDGE_AUTH_DEV_BYPASS", "1");

    let (tx, mut rx) = mpsc::channel(64);
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let event_sink = EventSink::new(tx);

    let static_driver =
        Arc::new(StaticDriver::new(Duration::from_secs(10), "quarry-test").unwrap());
    let state = AppState {
        // These tests are about `start_run`'s body-level ZDR guard, which sits
        // behind the driver-admission gate. See `support::governed_agent_driver`.
        #[cfg(feature = "browser-agent")]
        agent_driver: support::governed_agent_driver(),
        ..support::base_state(static_driver, event_sink)
    };
    let app = quarry_edge::routes::router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    addr
}

fn authed(request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
    request.bearer_auth(TEST_BEARER)
}

#[tokio::test]
async fn zdr_run_with_a_profile_id_is_rejected_before_touching_the_browser() {
    let addr = spawn_app().await;
    let client = reqwest::Client::new();

    let resp = authed(client.post(format!("http://{addr}/v1/agent/runs")).json(
        &serde_json::json!({
            "zdr": true,
            "profile_id": "01HZDRLEAKTESTPROFILEID0000",
        }),
    ))
    .send()
    .await
    .unwrap();

    assert_eq!(
        resp.status().as_u16(),
        400,
        "a ZDR run + profile_id must be rejected"
    );
    let body: Value = resp.json().await.unwrap();
    let message = body["error"]["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("zdr_persistent_profile_forbidden"),
        "unexpected error body: {body}"
    );
}

#[tokio::test]
async fn zdr_run_with_persist_profile_flag_is_rejected_even_without_a_profile_id() {
    let addr = spawn_app().await;
    let client = reqwest::Client::new();

    let resp = authed(client.post(format!("http://{addr}/v1/agent/runs")).json(
        &serde_json::json!({
            "zdr": true,
            "persist_profile": true,
        }),
    ))
    .send()
    .await
    .unwrap();

    assert_eq!(
        resp.status().as_u16(),
        400,
        "a ZDR run + persist_profile:true must be rejected even with no profile_id"
    );
    let body: Value = resp.json().await.unwrap();
    let message = body["error"]["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("zdr_persistent_profile_forbidden"),
        "unexpected error body: {body}"
    );
}

/// Regression test for the Phase 3 ZDR-bypass class of bug, at the
/// Quarry-edge layer: a real, previously-created, genuinely-persistent
/// profile must still be refused for a ZDR run — the guard must not be
/// satisfied merely because the profile "looks" ephemeral or the caller
/// only supplies one of the two conflicting signals. This mirrors
/// `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/browser.rs`'s
/// `zdr_session_is_rejected_despite_explicit_ephemeral_scope_claim_over_real_profile_id`
/// but proves the same class of conflicting-signal payload is refused one
/// layer deeper, independent of the gateway ever running at all.
#[tokio::test]
async fn zdr_run_against_a_real_existing_profile_is_still_rejected() {
    let addr = spawn_app().await;
    let client = reqwest::Client::new();

    let created: Value = authed(
        client
            .post(format!("http://{addr}/v1/profiles/create"))
            .json(
                &serde_json::json!({ "name": "Real persistent profile", "scope": "user_private" }),
            ),
    )
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    let real_profile_id = created["data"]["profile_id"].as_str().unwrap().to_string();

    let resp = authed(client.post(format!("http://{addr}/v1/agent/runs")).json(
        &serde_json::json!({
            "zdr": true,
            "profile_id": real_profile_id,
            // The exact Phase-3-class conflicting signal: claiming
            // persist_profile is false does not help — has_profile_id
            // alone is enough to imply persistence, and zdr must win.
            "persist_profile": false,
        }),
    ))
    .send()
    .await
    .unwrap();

    assert_eq!(
        resp.status().as_u16(),
        400,
        "a ZDR run must not be allowed to attach to any real, existing profile"
    );
    let body: Value = resp.json().await.unwrap();
    let message = body["error"]["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("zdr_persistent_profile_forbidden"),
        "unexpected error body: {body}"
    );
}

#[tokio::test]
async fn non_zdr_run_with_persist_profile_signals_is_not_rejected_by_the_zdr_guard() {
    let addr = spawn_app().await;
    let client = reqwest::Client::new();

    let resp = authed(client.post(format!("http://{addr}/v1/agent/runs")).json(
        &serde_json::json!({
            "zdr": false,
            "profile_id": "01HNOTZDRPROFILEID00000000",
        }),
    ))
    .send()
    .await
    .unwrap();

    // Must NOT be rejected by the ZDR guard specifically. It may still fail
    // downstream for unrelated reasons in a test environment with no real
    // Chromium binary (a 502 DriverFailed from `agent_driver.acquire()`),
    // which is fine and expected here — this test only proves the guard
    // itself does not fire for a non-ZDR request.
    if resp.status().as_u16() == 400 {
        let body: Value = resp.json().await.unwrap();
        let message = body["error"]["message"].as_str().unwrap_or_default();
        assert!(
            !message.contains("zdr_persistent_profile_forbidden"),
            "non-ZDR request must never be rejected by the ZDR guard: {body}"
        );
    }
}
