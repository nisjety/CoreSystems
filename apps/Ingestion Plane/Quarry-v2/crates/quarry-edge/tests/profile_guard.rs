//! WS17.P0 route tests: the principal guard, the ZDR gate, the import scope, the redacted
//! reads and the 4 MiB import cap on `/v1/profiles*`.
//!
//! The handlers are called directly with hand-built claims, as `tests/profile_owner.rs`
//! does, so a service principal and a ZDR posture can be set exactly. The import-cap test
//! goes through the router with the dev bypass stub (which carries the import scope).
//! The decision tables are unit-tested in `profile_guard` and `profile_routes`.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::{Extension, Json};
use quarry_browser::session::{
    Cookie, IndexedDbEntry, ProfileMetadata, ProfileScope, ProfileStore, SaveMeta,
    SessionSnapshot, Viewport,
};
use quarry_core::envelope::Envelope;
use quarry_core::ids::kinds::ProfileKind;
use quarry_edge::auth::Claims;
use quarry_edge::profile_guard::IMPORT_SCOPE;
use quarry_edge::profile_routes::{
    self, CreateProfileRequest, RestoreProbeRequest, SaveProfileRequest, UpdateProfileRequest,
};
use quarry_edge::state::AppState;
use quarry_runtime::{fetch::StaticDriver, EventSink};
use serde_json::{json, Value};
use tokio::sync::mpsc;

mod support;

type Refusal = (StatusCode, Json<Envelope<()>>);

const ORG: &str = "org_p0_guard";
const ALICE: &str = "user_alice_p0";
const CANARY: &str = "CANARY-d16-p0";

fn user_claims(scopes: &[&str], zdr: Option<bool>) -> Claims {
    Claims {
        web_policy: None,
        sub: ALICE.to_owned(),
        iss: "test".to_owned(),
        exp: i64::MAX,
        org_id: ORG.to_owned(),
        user_id: ALICE.to_owned(),
        principal_type: Some("user".to_owned()),
        service_id: None,
        nbf: None,
        aud: None,
        scopes: scopes.iter().map(|s| (*s).to_owned()).collect(),
        zdr,
    }
}

/// A service token that holds every scope, the import scope included: the principal
/// refusal must not depend on what the token says it may do.
fn service_claims() -> Claims {
    Claims {
        web_policy: None,
        sub: "execution-core".to_owned(),
        iss: "test".to_owned(),
        exp: i64::MAX,
        org_id: ORG.to_owned(),
        user_id: String::new(),
        principal_type: Some("service".to_owned()),
        service_id: Some("execution-core".to_owned()),
        nbf: None,
        aud: None,
        scopes: vec![IMPORT_SCOPE.to_owned(), "quarry:profiles:read".to_owned()],
        zdr: Some(true),
    }
}

/// An edge state with an in-memory profile store and the ZDR refusal flag as given.
fn state(zdr_refuse: bool) -> AppState {
    let (tx, mut rx) = mpsc::channel(64);
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let driver = Arc::new(StaticDriver::new(Duration::from_secs(10), "quarry-test").unwrap());
    let mut state = support::base_state(driver, EventSink::new(tx));
    state.zdr_refuse_durable_resources = zdr_refuse;
    state
}

/// A snapshot whose every secret field carries the canary.
fn canary_snapshot() -> SessionSnapshot {
    SessionSnapshot {
        cookies: vec![Cookie {
            name: "sid".to_owned(),
            value: CANARY.to_owned(),
            domain: ".example.com".to_owned(),
            path: "/".to_owned(),
            secure: true,
            http_only: true,
            expires: None,
            ..Default::default()
        }],
        local_storage: vec![("token".to_owned(), CANARY.to_owned())],
        session_storage: vec![("nav".to_owned(), CANARY.to_owned())],
        indexed_db: vec![IndexedDbEntry {
            origin: "https://app.example".to_owned(),
            database: "records".to_owned(),
            store: "rows".to_owned(),
            key: CANARY.to_owned(),
            value: json!({ "secret": CANARY }),
        }],
        user_agent: Some("Quarry/1.0 (test)".to_owned()),
        viewport: Some(Viewport {
            width: 1280,
            height: 800,
            device_scale_factor: 1.0,
            is_mobile: false,
        }),
        locale: Some("en-US".to_owned()),
        timezone: Some("UTC".to_owned()),
        ..Default::default()
    }
}

/// Seeds a named, user-private profile owned by `owner` straight into the store.
async fn seed(state: &AppState, owner: &str) -> ProfileKind {
    let id: ProfileKind = quarry_core::ids::Id::new();
    state
        .profiles
        .save(ORG, &id, &canary_snapshot(), &SaveMeta::now(Some(owner.to_owned())))
        .await
        .expect("seed a snapshot");
    state
        .profiles
        .save_metadata(
            ORG,
            &id,
            &ProfileMetadata {
                name: Some("Work".to_owned()),
                scope: ProfileScope::UserPrivate,
                owner: Some(owner.to_owned()),
            },
        )
        .await
        .expect("seed the metadata");
    id
}

fn status_of<T>(result: Result<T, Refusal>) -> StatusCode {
    match result {
        Ok(_) => StatusCode::OK,
        Err((status, _)) => status,
    }
}

/// The `error.details.zdr_refusal.action` of a refusal, or null.
fn zdr_action_of(result: Result<Value, Refusal>) -> Value {
    match result {
        Ok(_) => Value::Null,
        Err((_, body)) => {
            let value = serde_json::to_value(&body.0).unwrap();
            value["error"]["details"]["zdr_refusal"]["action"].clone()
        }
    }
}

async fn load(state: &AppState, claims: Claims, id: &ProfileKind) -> Result<Value, Refusal> {
    profile_routes::load_profile(
        State(state.clone()),
        Extension(claims),
        Path(id.to_string()),
    )
    .await
    .map(|ok| serde_json::to_value(&ok.0.data).unwrap())
}

async fn probe(state: &AppState, claims: Claims, id: &ProfileKind) -> Result<Value, Refusal> {
    profile_routes::restore_probe(
        State(state.clone()),
        Extension(claims),
        Path(id.to_string()),
        Json(RestoreProbeRequest {
            url: "https://example.com/".to_owned(),
        }),
    )
    .await
    .map(|ok| serde_json::to_value(&ok.0.data).unwrap())
}

async fn create(state: &AppState, claims: Claims) -> Result<Value, Refusal> {
    profile_routes::create_profile(
        State(state.clone()),
        Extension(claims),
        Json(CreateProfileRequest {
            name: Some("New".to_owned()),
            scope: ProfileScope::UserPrivate,
        }),
    )
    .await
    .map(|ok| serde_json::to_value(&ok.0.data).unwrap())
}

async fn save(state: &AppState, claims: Claims) -> Result<Value, Refusal> {
    profile_routes::save_profile(
        State(state.clone()),
        Extension(claims),
        Json(SaveProfileRequest {
            profile_id: None,
            snapshot: canary_snapshot(),
        }),
    )
    .await
    .map(|ok| serde_json::to_value(&ok.0.data).unwrap())
}

#[tokio::test]
async fn get_profile_returns_value_null_and_value_len() {
    let state = state(false);
    let id = seed(&state, ALICE).await;

    let body = load(&state, user_claims(&[], None), &id)
        .await
        .expect("the owner loads");
    let cookie = &body["cookies"][0];
    assert_eq!(cookie["name"], "sid");
    assert!(cookie["value"].is_null(), "{body}");
    assert_eq!(cookie["value_len"], CANARY.len());
    assert_eq!(cookie["session"], true);
    assert_eq!(body["local_storage_count"], 1);
    assert_eq!(body["session_storage_count"], 1);
    assert_eq!(body["origins"][0]["idb_rows"], 1);
    assert_eq!(body["name"], "Work");
    assert_eq!(body["scope"], "user_private");
    assert_eq!(body["owner_is_caller"], true);
    assert_eq!(body["truncated"], false);
    assert!(body.get("snapshot").is_none(), "{body}");
}

#[tokio::test]
async fn canary_cookie_value_never_appears_in_any_profile_response() {
    let state = state(false);
    let id = seed(&state, ALICE).await;
    let claims = user_claims(&[], None);

    let mut seen = Vec::new();
    seen.push(load(&state, claims.clone(), &id).await.expect("load"));
    seen.push(probe(&state, claims.clone(), &id).await.expect("probe"));
    let listed = profile_routes::list_profiles(State(state.clone()), Extension(claims))
        .await
        .expect("list");
    seen.push(serde_json::to_value(&listed.0.data).unwrap());

    for body in seen {
        let text = body.to_string();
        assert!(!text.contains(CANARY), "a secret leaked: {text}");
    }
}

#[tokio::test]
async fn service_token_gets_403_on_all_profile_routes() {
    let state = state(false);
    let id = seed(&state, ALICE).await;
    let service = service_claims();

    assert_eq!(
        status_of(
            profile_routes::list_profiles(State(state.clone()), Extension(service.clone())).await
        ),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        status_of(create(&state, service.clone()).await),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        status_of(load(&state, service.clone(), &id).await),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        status_of(
            profile_routes::update_profile(
                State(state.clone()),
                Extension(service.clone()),
                Path(id.to_string()),
                Json(UpdateProfileRequest {
                    name: Some("Renamed".to_owned()),
                    scope: None,
                }),
            )
            .await
        ),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        status_of(
            profile_routes::delete_profile(
                State(state.clone()),
                Extension(service.clone()),
                Path(id.to_string()),
            )
            .await
        ),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        status_of(probe(&state, service.clone(), &id).await),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        status_of(save(&state, service).await),
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn zdr_flag_off_only_logs() {
    // Flag off (the default): a ZDR user is served on every profile route as before.
    let state = state(false);
    let id = seed(&state, ALICE).await;
    let zdr = user_claims(&[], Some(true));

    assert!(create(&state, zdr.clone()).await.is_ok());
    assert!(load(&state, zdr.clone(), &id).await.is_ok());
    assert!(probe(&state, zdr, &id).await.is_ok());
}

#[tokio::test]
async fn zdr_user_is_refused_except_list_and_delete_when_flag_on() {
    let state = state(true);
    let id = seed(&state, ALICE).await;
    let zdr = user_claims(&[IMPORT_SCOPE], Some(true));

    // Refused: every op except list and delete.
    assert_eq!(zdr_action_of(create(&state, zdr.clone()).await), "create");
    assert_eq!(zdr_action_of(save(&state, zdr.clone()).await), "save");
    assert_eq!(zdr_action_of(load(&state, zdr.clone(), &id).await), "read");
    assert_eq!(zdr_action_of(probe(&state, zdr.clone(), &id).await), "probe");
    let updated = profile_routes::update_profile(
        State(state.clone()),
        Extension(zdr.clone()),
        Path(id.to_string()),
        Json(UpdateProfileRequest {
            name: Some("Renamed".to_owned()),
            scope: None,
        }),
    )
    .await
    .map(|ok| serde_json::to_value(&ok.0.data).unwrap());
    assert_eq!(zdr_action_of(updated), "update");
    // Kept: list.
    assert!(profile_routes::list_profiles(State(state.clone()), Extension(zdr.clone()))
        .await
        .is_ok());
}

#[tokio::test]
async fn zdr_org_user_cannot_create_or_probe_but_can_delete() {
    let state = state(true);
    let id = seed(&state, ALICE).await;
    let zdr = user_claims(&[], Some(true));

    assert_eq!(zdr_action_of(create(&state, zdr.clone()).await), "create");
    assert_eq!(zdr_action_of(probe(&state, zdr.clone(), &id).await), "probe");
    // Wind-down: an org erases what it already holds.
    let erased = profile_routes::delete_profile(
        State(state.clone()),
        Extension(zdr),
        Path(id.to_string()),
    )
    .await;
    assert_eq!(erased.expect("delete stays open"), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn import_requires_scope() {
    let state = state(false);

    // Without the import scope: 403 with the scope message.
    let (status, body) = match profile_routes::save_profile(
        State(state.clone()),
        Extension(user_claims(&[], None)),
        Json(SaveProfileRequest {
            profile_id: None,
            snapshot: canary_snapshot(),
        }),
    )
    .await
    {
        Err(refusal) => refusal,
        Ok(_) => panic!("an import without the scope must be refused"),
    };
    assert_eq!(status, StatusCode::FORBIDDEN);
    let text = serde_json::to_value(&body.0).unwrap().to_string();
    assert!(text.contains("quarry:profiles:import"), "{text}");

    // With the scope: the import is served.
    assert!(save(&state, user_claims(&[IMPORT_SCOPE], None)).await.is_ok());
}

#[tokio::test]
async fn import_over_cap_is_413() {
    // Through the router: the cap is a layer, not a handler check.
    std::env::set_var("QUARRY_EDGE_AUTH_DEV_BYPASS", "1");
    let (tx, mut rx) = mpsc::channel(64);
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let driver = Arc::new(StaticDriver::new(Duration::from_secs(10), "quarry-test").unwrap());
    let state = support::base_state(driver, EventSink::new(tx));
    let app = quarry_edge::routes::router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let client = reqwest::Client::new();

    let post = |size: usize| {
        let body = json!({
            "snapshot": { "cookies": [], "local_storage": [["k", "x".repeat(size)]] }
        });
        client
            .post(format!("http://{addr}/v1/profiles"))
            .bearer_auth("profile-guard-dev-token")
            .header("content-type", "application/json")
            .body(serde_json::to_vec(&body).unwrap())
            .send()
    };

    // Past the 1 MB surface cap, under the 4 MiB import cap: served.
    let inside = post(1_500_000).await.unwrap();
    assert!(inside.status().is_success(), "{}", inside.status());

    // Past 4 MiB: 413.
    let over = post(5 * 1024 * 1024).await.unwrap();
    assert_eq!(over.status().as_u16(), 413);
}
