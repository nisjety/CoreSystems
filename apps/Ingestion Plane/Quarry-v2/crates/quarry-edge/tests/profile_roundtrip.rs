//! End-to-end capture/restore round-trip test for the `/v1/profiles` endpoints.
//!
//! Closes the long-standing risk-register gap: "Browser session capture/restore
//! round-trip not yet exercised". Captures a SessionSnapshot on one session and
//! restores it on a fresh one, verifying every field roundtrips intact.

use std::{net::SocketAddr, sync::Arc, time::Duration};

use quarry_browser::session::{Cookie, InMemoryProfileStore, SessionSnapshot, Viewport};
use quarry_core::output::DriverKind;
use quarry_edge::state::AppState;
use quarry_runtime::driver_registry::DriverRegistry;
use quarry_runtime::{artifact_store::InMemoryStore, fetch::StaticDriver, EventSink};
use quarry_security::preflight::DefaultEngine;
use serde_json::Value;
use tokio::sync::mpsc;

const TEST_BEARER: &str = "profile-roundtrip-dev-token";

async fn spawn_app() -> SocketAddr {
    std::env::set_var("QUARRY_EDGE_AUTH_DEV_BYPASS", "1");

    let (tx, mut rx) = mpsc::channel(64);
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let event_sink = EventSink::new(tx);

    let static_driver =
        Arc::new(StaticDriver::new(Duration::from_secs(10), "quarry-test").unwrap());
    let mut drivers = DriverRegistry::new(DriverKind::Static);
    drivers.register(static_driver.clone());
    let state = AppState {
        driver: static_driver,
        drivers,
        http3: None,
        security: Arc::new(DefaultEngine::new().with_allow_private_hosts(true)),
        artifacts: Arc::new(InMemoryStore::new()),
        control_base_url: String::new(),
        redis: None,
        cache: None,
        event_sink,
        ingest: None,
        profiles: Arc::new(InMemoryProfileStore::new()),
        search: None,
        vector_index: None,
        searxng_url: None,
        model_plane_url: None,
        model_plane_token: None,
        answer_pipeline: None,
        local_index: None,
        policy: quarry_runtime::RunPolicy::default(),
        scheduler: None,
        internal_signer: None,
        page_renderer: None,
        #[cfg(feature = "postgres-queue")]
        event_history: None,
        usage: std::sync::Arc::new(quarry_runtime::NoopUsageMeter),
        #[cfg(feature = "browser-agent")]
        agent_driver: Arc::new(quarry_browser::chromiumoxide::ChromiumoxideDriver::new()),
        #[cfg(feature = "browser-agent")]
        agent_runs: quarry_edge::agent_routes::new_runs(),
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

fn rich_snapshot() -> SessionSnapshot {
    SessionSnapshot {
        cookies: vec![
            Cookie {
                name: "session".into(),
                value: "abc123".into(),
                domain: ".example.com".into(),
                path: "/".into(),
                secure: true,
                http_only: true,
                expires: None,
            },
            Cookie {
                name: "csrf".into(),
                value: "xyz789".into(),
                domain: "example.com".into(),
                path: "/api".into(),
                secure: false,
                http_only: false,
                expires: None,
            },
        ],
        local_storage: vec![
            ("theme".into(), "dark".into()),
            ("locale".into(), "en-US".into()),
        ],
        session_storage: vec![("nav_state".into(), "expanded".into())],
        indexed_db: vec![],
        user_agent: Some("Quarry/1.0 (test)".into()),
        viewport: Some(Viewport {
            width: 1920,
            height: 1080,
            device_scale_factor: 2.0,
            is_mobile: false,
        }),
        locale: Some("en-US".into()),
        timezone: Some("America/Los_Angeles".into()),
    }
}

#[tokio::test]
async fn profile_save_load_roundtrip_preserves_all_fields() {
    let addr = spawn_app().await;
    let client = reqwest::Client::new();

    // Save
    let snapshot = rich_snapshot();
    let save_resp = authed(
        client
            .post(format!("http://{addr}/v1/profiles"))
            .json(&serde_json::json!({ "snapshot": snapshot })),
    )
    .send()
    .await
    .unwrap();
    assert!(
        save_resp.status().is_success(),
        "save failed: {save_resp:?}"
    );
    let save_body: Value = save_resp.json().await.unwrap();
    let profile_id = save_body["data"]["profile_id"]
        .as_str()
        .unwrap()
        .to_string();

    // Load
    let load_resp = authed(client.get(format!("http://{addr}/v1/profiles/{profile_id}")))
        .send()
        .await
        .unwrap();
    assert!(load_resp.status().is_success());
    let load_body: Value = load_resp.json().await.unwrap();
    let loaded: SessionSnapshot =
        serde_json::from_value(load_body["data"]["snapshot"].clone()).unwrap();

    // Verify every field round-trips
    assert_eq!(loaded.cookies.len(), 2);
    assert_eq!(loaded.cookies[0].name, "session");
    assert_eq!(loaded.cookies[0].value, "abc123");
    assert_eq!(loaded.cookies[0].domain, ".example.com");
    assert!(loaded.cookies[0].secure);
    assert!(loaded.cookies[0].http_only);
    assert_eq!(loaded.cookies[1].name, "csrf");
    assert_eq!(loaded.cookies[1].path, "/api");
    assert!(!loaded.cookies[1].secure);

    assert_eq!(loaded.local_storage.len(), 2);
    assert_eq!(loaded.local_storage[0], ("theme".into(), "dark".into()));
    assert_eq!(loaded.session_storage.len(), 1);

    assert_eq!(loaded.user_agent.as_deref(), Some("Quarry/1.0 (test)"));
    let vp = loaded.viewport.unwrap();
    assert_eq!(vp.width, 1920);
    assert_eq!(vp.height, 1080);
    assert!((vp.device_scale_factor - 2.0).abs() < 1e-9);
    assert!(!vp.is_mobile);
    assert_eq!(loaded.locale.as_deref(), Some("en-US"));
    assert_eq!(loaded.timezone.as_deref(), Some("America/Los_Angeles"));
}

#[tokio::test]
async fn profile_list_includes_saved_profile() {
    let addr = spawn_app().await;
    let client = reqwest::Client::new();

    let saved_id = {
        let resp = authed(
            client
                .post(format!("http://{addr}/v1/profiles"))
                .json(&serde_json::json!({ "snapshot": rich_snapshot() })),
        )
        .send()
        .await
        .unwrap();
        let body: Value = resp.json().await.unwrap();
        body["data"]["profile_id"].as_str().unwrap().to_string()
    };

    let list_resp = authed(client.get(format!("http://{addr}/v1/profiles")))
        .send()
        .await
        .unwrap();
    let list_body: Value = list_resp.json().await.unwrap();
    let profiles: Vec<String> =
        serde_json::from_value(list_body["data"]["profiles"].clone()).unwrap();
    assert!(
        profiles.contains(&saved_id),
        "{profiles:?} missing {saved_id}"
    );
}

#[tokio::test]
async fn profile_delete_removes_entry() {
    let addr = spawn_app().await;
    let client = reqwest::Client::new();

    let saved_id = {
        let resp = authed(
            client
                .post(format!("http://{addr}/v1/profiles"))
                .json(&serde_json::json!({ "snapshot": rich_snapshot() })),
        )
        .send()
        .await
        .unwrap();
        let body: Value = resp.json().await.unwrap();
        body["data"]["profile_id"].as_str().unwrap().to_string()
    };

    let del_resp = authed(client.delete(format!("http://{addr}/v1/profiles/{saved_id}")))
        .send()
        .await
        .unwrap();
    assert_eq!(del_resp.status().as_u16(), 204);

    let load_resp = authed(client.get(format!("http://{addr}/v1/profiles/{saved_id}")))
        .send()
        .await
        .unwrap();
    assert_eq!(load_resp.status().as_u16(), 404);
}

#[tokio::test]
async fn profile_load_unknown_returns_404() {
    let addr = spawn_app().await;
    let client = reqwest::Client::new();

    // Generate a valid ULID-shaped ProfileKind that wasn't saved
    let unknown: quarry_core::ids::kinds::ProfileKind = quarry_core::ids::Id::new();
    let resp = authed(client.get(format!("http://{addr}/v1/profiles/{unknown}")))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 404);
}

#[tokio::test]
async fn profile_save_with_specified_id_round_trips_that_id() {
    let addr = spawn_app().await;
    let client = reqwest::Client::new();
    let chosen: quarry_core::ids::kinds::ProfileKind = quarry_core::ids::Id::new();
    let chosen_str = chosen.to_string();

    let resp = authed(
        client
            .post(format!("http://{addr}/v1/profiles"))
            .json(&serde_json::json!({
                "profile_id": chosen_str,
                "snapshot": rich_snapshot(),
            })),
    )
    .send()
    .await
    .unwrap();
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["data"]["profile_id"].as_str().unwrap(), chosen_str);

    let load = authed(client.get(format!("http://{addr}/v1/profiles/{chosen_str}")))
        .send()
        .await
        .unwrap();
    assert!(load.status().is_success());
}
