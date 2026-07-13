use std::{
    sync::{Arc, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use quickwit_adapter_rs::{
    api::router_with_store,
    auth::{AdminClaims, AdminVerifier},
    jobs, quickwit,
    rebuild::RebuildContext,
};
use rsa::{
    pkcs8::{EncodePrivateKey, EncodePublicKey, LineEnding},
    rand_core::OsRng,
    RsaPrivateKey,
};
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;

fn app() -> (axum::Router, RsaPrivateKey) {
    static PRIVATE_KEY: OnceLock<RsaPrivateKey> = OnceLock::new();
    let private = PRIVATE_KEY
        .get_or_init(|| RsaPrivateKey::new(&mut OsRng, 2048).expect("test RSA key"))
        .clone();
    let public_pem = private
        .to_public_key()
        .to_public_key_pem(LineEnding::LF)
        .expect("public PEM");
    let verifier = AdminVerifier::from_pem(
        public_pem.as_bytes(),
        "data-plane",
        "https://control.example/api/convex-auth",
    )
    .expect("verifier");
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://unused:unused@127.0.0.1:1/unused")
        .expect("lazy pool");
    let config_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../infra/quickwit/dataplane-corpus-index.yaml"
    );
    let quickwit = quickwit::QuickwitClient::new("http://127.0.0.1:1", "test-index", config_path)
        .expect("test client");
    let ctx = Arc::new(RebuildContext {
        pool,
        quickwit,
        batch_size: 10,
    });
    let jobs = Arc::new(jobs::InMemoryAdminJobStore::new(
        std::time::Duration::ZERO,
        1,
    ));
    (router_with_store(ctx, Arc::new(verifier), jobs), private)
}

fn user_token(private: &RsaPrivateKey, org_id: &str, scopes: &[&str]) -> String {
    let private_pem = private.to_pkcs8_pem(LineEnding::LF).expect("private PEM");
    let claims = AdminClaims {
        user_id: Some("operator-1".into()),
        service_id: None,
        principal_type: "user".into(),
        org_id: org_id.into(),
        scopes: scopes.iter().map(|scope| (*scope).to_string()).collect(),
        issuer: "https://control.example/api/convex-auth".into(),
        audience: "data-plane".into(),
        subject: "operator-1".into(),
        expires_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_secs()
            + 300,
    };
    encode(
        &Header::new(Algorithm::RS256),
        &claims,
        &EncodingKey::from_rsa_pem(private_pem.as_bytes()).expect("encoding key"),
    )
    .expect("token")
}

fn service_token(private: &RsaPrivateKey, org_id: &str, scopes: &[&str]) -> String {
    let private_pem = private.to_pkcs8_pem(LineEnding::LF).expect("private PEM");
    let claims = AdminClaims {
        user_id: None,
        service_id: Some("service:search-operator".into()),
        principal_type: "service".into(),
        org_id: org_id.into(),
        scopes: scopes.iter().map(|scope| (*scope).to_string()).collect(),
        issuer: "https://control.example/api/convex-auth".into(),
        audience: "data-plane".into(),
        subject: "service:search-operator".into(),
        expires_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_secs()
            + 300,
    };
    encode(
        &Header::new(Algorithm::RS256),
        &claims,
        &EncodingKey::from_rsa_pem(private_pem.as_bytes()).expect("encoding key"),
    )
    .expect("token")
}

fn rebuild_request(authorization: Option<&str>, body: &str) -> Request<Body> {
    let mut builder = Request::builder()
        .method("POST")
        .uri("/admin/rebuild")
        .header("content-type", "application/json");
    if let Some(value) = authorization {
        builder = builder.header("authorization", value);
    }
    builder.body(Body::from(body.to_owned())).expect("request")
}

#[tokio::test]
async fn wired_route_rejects_missing_credentials() {
    let (app, _) = app();
    let response = app
        .oneshot(rebuild_request(None, r#"{"dry_run":true}"#))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn wired_route_authenticates_before_parsing_an_invalid_body() {
    let (app, _) = app();
    let response = app
        .oneshot(rebuild_request(None, r#"{"dry_run":"#))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn wired_route_rejects_spoofed_tenant() {
    let (app, private) = app();
    let bearer = format!(
        "Bearer {}",
        user_token(&private, "org-a", &["data:search:rebuild"])
    );
    let response = app
        .oneshot(rebuild_request(
            Some(&bearer),
            r#"{"org_id":"org-b","dry_run":true}"#,
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn wired_route_allows_scoped_tenant_preview_without_side_effects() {
    let (app, private) = app();
    let bearer = format!(
        "Bearer {}",
        user_token(&private, "org-a", &["data:search:rebuild"])
    );
    let response = app
        .oneshot(rebuild_request(Some(&bearer), r#"{"dry_run":true}"#))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn wired_route_rejects_unknown_admin_fields() {
    let (app, private) = app();
    let bearer = format!(
        "Bearer {}",
        user_token(&private, "org-a", &["data:search:rebuild"])
    );
    let response = app
        .oneshot(rebuild_request(
            Some(&bearer),
            r#"{"dry_run":true,"force_everything":true}"#,
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn wired_route_rejects_oversized_admin_bodies() {
    let (app, private) = app();
    let bearer = format!(
        "Bearer {}",
        user_token(&private, "org-a", &["data:search:rebuild"])
    );
    let body = format!(r#"{{"dry_run":true,"reason":"{}"}}"#, "x".repeat(20 * 1024));
    let response = app
        .oneshot(rebuild_request(Some(&bearer), &body))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn wired_route_accepts_canonical_scoped_service_preview() {
    let (app, private) = app();
    let bearer = format!(
        "Bearer {}",
        service_token(&private, "org-a", &["data:search:rebuild"])
    );
    let response = app
        .oneshot(rebuild_request(Some(&bearer), r#"{"dry_run":true}"#))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn wired_route_refuses_global_mutation_without_durable_orchestration() {
    let (app, private) = app();
    let bearer = format!(
        "Bearer {}",
        service_token(
            &private,
            "org-a",
            &["data:search:rebuild", "data:search:rebuild:global"],
        )
    );
    let response = app
        .oneshot(rebuild_request(
            Some(&bearer),
            r#"{
                "global": true,
                "break_glass": true,
                "dry_run": false,
                "clear": true,
                "approval_id": "approval-123",
                "idempotency_key": "idempotency-123",
                "reason": "approved recovery exercise"
            }"#,
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
}

#[tokio::test]
async fn wired_route_allows_only_a_global_break_glass_preview() {
    let (app, private) = app();
    let bearer = format!(
        "Bearer {}",
        service_token(
            &private,
            "org-a",
            &["data:search:rebuild", "data:search:rebuild:global"],
        )
    );
    let response = app
        .oneshot(rebuild_request(
            Some(&bearer),
            r#"{
                "global": true,
                "break_glass": true,
                "dry_run": true,
                "clear": true
            }"#,
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn wired_route_accepts_durable_tenant_job_without_clear() {
    let (app, private) = app();
    let bearer = format!(
        "Bearer {}",
        service_token(&private, "org-a", &["data:search:rebuild"])
    );
    let response = app
        .oneshot(rebuild_request(
            Some(&bearer),
            r#"{
                "dry_run": false,
                "clear": false,
                "approval_id": "approval-123",
                "idempotency_key": "idempotency-123",
                "reason": "approved tenant recovery exercise"
            }"#,
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::ACCEPTED);
}

#[tokio::test]
async fn wired_route_requires_separate_scoped_approver_and_matching_approval() {
    let (app, private) = app();
    let requester = format!(
        "Bearer {}",
        service_token(&private, "org-a", &["data:search:rebuild"])
    );
    let response = app
        .clone()
        .oneshot(rebuild_request(
            Some(&requester),
            r#"{
                "dry_run": false,
                "approval_id": "approval-route",
                "idempotency_key": "idempotency-route",
                "reason": "approved tenant recovery exercise"
            }"#,
        ))
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .expect("body");
    let job_id = serde_json::from_slice::<serde_json::Value>(&body).expect("json")["job_id"]
        .as_str()
        .expect("job id")
        .to_string();

    let approver = format!(
        "Bearer {}",
        user_token(&private, "org-a", &["data:search:rebuild:approve"])
    );
    let wrong_reference = Request::builder()
        .method("POST")
        .uri(format!("/admin/rebuild/{job_id}/approve"))
        .header("content-type", "application/json")
        .header("authorization", &approver)
        .body(Body::from(r#"{"approval_id":"wrong","break_glass":false}"#))
        .expect("request");
    assert_eq!(
        app.clone().oneshot(wrong_reference).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );

    let approval = Request::builder()
        .method("POST")
        .uri(format!("/admin/rebuild/{job_id}/approve"))
        .header("content-type", "application/json")
        .header("authorization", &approver)
        .body(Body::from(
            r#"{"approval_id":"approval-route","break_glass":false}"#,
        ))
        .expect("request");
    assert_eq!(
        app.oneshot(approval).await.unwrap().status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn health_readiness_and_scoped_job_status_are_wired() {
    let (app, private) = app();
    let health = Request::builder()
        .uri("/health")
        .body(Body::empty())
        .expect("health request");
    assert_eq!(
        app.clone().oneshot(health).await.unwrap().status(),
        StatusCode::OK
    );
    let readiness = Request::builder()
        .uri("/readyz")
        .body(Body::empty())
        .expect("readiness request");
    assert_eq!(
        app.clone().oneshot(readiness).await.unwrap().status(),
        StatusCode::OK
    );

    let requester = format!(
        "Bearer {}",
        service_token(&private, "org-a", &["data:search:rebuild"])
    );
    let created = app
        .clone()
        .oneshot(rebuild_request(
            Some(&requester),
            r#"{
                "dry_run": false,
                "approval_id": "approval-status",
                "idempotency_key": "idempotency-status",
                "reason": "status visibility exercise"
            }"#,
        ))
        .await
        .expect("created response");
    assert_eq!(created.status(), StatusCode::ACCEPTED);
    let body = axum::body::to_bytes(created.into_body(), 4096)
        .await
        .expect("body");
    let job_id = serde_json::from_slice::<serde_json::Value>(&body).expect("json")["job_id"]
        .as_str()
        .expect("job id")
        .to_string();

    let status_request = |bearer: &str, id: &str| {
        Request::builder()
            .uri(format!("/admin/rebuild/{id}"))
            .header("authorization", bearer)
            .body(Body::empty())
            .expect("status request")
    };
    assert_eq!(
        app.clone()
            .oneshot(status_request(&requester, &job_id))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    let other_tenant = format!(
        "Bearer {}",
        user_token(&private, "org-b", &["data:search:rebuild"])
    );
    assert_eq!(
        app.clone()
            .oneshot(status_request(&other_tenant, &job_id))
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        app.clone()
            .oneshot(status_request(&requester, "missing-job"))
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    let no_scope = format!("Bearer {}", user_token(&private, "org-a", &[]));
    assert_eq!(
        app.oneshot(status_request(&no_scope, &job_id))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn approval_fails_closed_for_missing_jobs_and_requester_self_approval() {
    let (app, private) = app();
    let requester = format!(
        "Bearer {}",
        service_token(
            &private,
            "org-a",
            &["data:search:rebuild", "data:search:rebuild:approve"],
        )
    );
    let approval_request = |job_id: &str, approval_id: &str| {
        Request::builder()
            .method("POST")
            .uri(format!("/admin/rebuild/{job_id}/approve"))
            .header("content-type", "application/json")
            .header("authorization", &requester)
            .body(Body::from(format!(
                r#"{{"approval_id":"{approval_id}","break_glass":false}}"#
            )))
            .expect("approval request")
    };
    assert_eq!(
        app.clone()
            .oneshot(approval_request("missing-job", "missing"))
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );

    let created = app
        .clone()
        .oneshot(rebuild_request(
            Some(&requester),
            r#"{
                "dry_run": false,
                "approval_id": "approval-self",
                "idempotency_key": "idempotency-self",
                "reason": "self approval rejection exercise"
            }"#,
        ))
        .await
        .expect("created response");
    let body = axum::body::to_bytes(created.into_body(), 4096)
        .await
        .expect("body");
    let job_id = serde_json::from_slice::<serde_json::Value>(&body).expect("json")["job_id"]
        .as_str()
        .expect("job id")
        .to_string();
    assert_eq!(
        app.oneshot(approval_request(&job_id, "approval-self"))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
}
