//! The sales-order operations (patch, set_status, create) through the real HttpTransport against a mock
//! Visma: tokens, GET with ETag, PATCH and POST.

use std::sync::Arc;

use mockito::Matcher;
use serde_json::json;
use vsv2_auth::{OAuthClient, Scope};
use vsv2_common::{Source, TenantId};
use vsv2_interactive::*;
use vsv2_ratelimit::Limiter;

const TENANT: &str = "t-e2e";

struct Rig {
    exec: Executor,
    sink: Arc<MemoryAuditSink>,
}

fn rig(server: &mockito::ServerGuard) -> Rig {
    let oauth = OAuthClient::new(format!("{}/token", server.url()), "cid", "secret")
        .with_scope(Scope::SalesOrder, "visma.net.erp.salesorder:read")
        .with_scope(Scope::SalesOrderWrite, "visma.net.erp.salesorder:write");
    let http = Arc::new(HttpTransport::new(
        oauth,
        Limiter::new(Source::ErpRest, 6000, 100_000),
        server.url(),
        server.url(),
    ));
    let sink = Arc::new(MemoryAuditSink::default());
    let exec = Executor::new(
        default_registry(),
        WritePolicy::new(
            [TENANT],
            [
                "salesorder.patch",
                "salesorder.create",
                "salesorder.set_status",
            ],
        ),
        ConfirmSigner::new(b"e2e-test-confirm-secret-0123".to_vec()).unwrap(),
        sink.clone(),
        http.clone(),
        http,
    );
    Rig { exec, sink }
}

fn input() -> serde_json::Value {
    json!({"type":"SO","orderId":"000100","changes":{
        "description":"Updated by Layer 3",
        "orderLines":[{"lineId":2,"quantity":7}]}})
}

fn token_mocks(s: &mut mockito::ServerGuard) -> (mockito::Mock, mockito::Mock) {
    let read = s
        .mock("POST", "/token")
        .match_body(Matcher::Regex(
            "scope=visma.net.erp.salesorder%3Aread".into(),
        ))
        .with_body(r#"{"access_token":"R","expires_in":3600}"#)
        .create();
    let write = s
        .mock("POST", "/token")
        .match_body(Matcher::Regex(
            "scope=visma.net.erp.salesorder%3Awrite".into(),
        ))
        .with_body(r#"{"access_token":"W","expires_in":3600}"#)
        .create();
    (read, write)
}

#[tokio::test]
async fn dry_run_reads_with_the_read_token_then_execute_patches_with_the_write_token() {
    let mut s = mockito::Server::new_async().await;
    let _tokens = token_mocks(&mut s);
    let get = s
        .mock("GET", "/api/v3/SalesOrders/SO/000100")
        .match_header("authorization", "Bearer R")
        .with_header("etag", "\"v7\"")
        .with_body(r#"{"orderId":"000100","status":{"description":"Open","cancelled":false,"hold":false}}"#)
        .expect(1)
        .create_async()
        .await;
    let patch = s
        .mock("PATCH", "/api/v3/SalesOrders/SO/000100")
        .match_header("authorization", "Bearer W")
        .match_header("if-match", "\"v7\"")
        .match_body(Matcher::Json(json!({
            "description":"Updated by Layer 3",
            "orderLines":[{"lineId":2,"quantity":7}]})))
        .with_status(202)
        .expect(1)
        .create_async()
        .await;
    let r = rig(&s);
    let t = TenantId::new(TENANT);

    let d = r
        .exec
        .dry_run("ima", &t, "salesorder.patch", &input())
        .await
        .unwrap();
    get.assert_async().await;
    assert!(!patch.matched_async().await, "dry run must not PATCH");
    assert_eq!(d.plan.request.if_match.as_deref(), Some("\"v7\""));
    assert!(d.plan.summary.contains("SO/000100"));

    let out = r
        .exec
        .execute("ima", &d.plan, &d.confirm_token)
        .await
        .unwrap();
    patch.assert_async().await;
    assert_eq!(out.status, 202);
    assert_eq!(
        r.sink.stages(),
        vec![
            AuditStage::DryRun,
            AuditStage::Executing,
            AuditStage::Succeeded
        ]
    );
}

#[tokio::test]
async fn a_concurrent_edit_gives_412_which_is_surfaced_and_audited_not_overwritten() {
    let mut s = mockito::Server::new_async().await;
    let _tokens = token_mocks(&mut s);
    let _get = s
        .mock("GET", "/api/v3/SalesOrders/SO/000100")
        .with_header("etag", "\"v7\"")
        .with_body(r#"{"orderId":"000100","status":{"description":"Open","cancelled":false,"hold":false}}"#)
        .create_async()
        .await;
    let _patch = s
        .mock("PATCH", "/api/v3/SalesOrders/SO/000100")
        .with_status(412)
        .with_body("Order version does not match with If-Match header")
        .expect(1)
        .create_async()
        .await;
    let r = rig(&s);
    let t = TenantId::new(TENANT);

    let d = r
        .exec
        .dry_run("ima", &t, "salesorder.patch", &input())
        .await
        .unwrap();
    let res = r.exec.execute("ima", &d.plan, &d.confirm_token).await;

    assert!(matches!(
        res,
        Err(InteractiveError::Upstream { status: 412, .. })
    ));
    let last = r.sink.entries().pop().unwrap();
    assert_eq!(
        (last.stage, last.http_status),
        (AuditStage::Failed, Some(412))
    );
}

const OPEN_ORDER: &str =
    r#"{"orderId":"000100","status":{"description":"Open","cancelled":false,"hold":false}}"#;

#[tokio::test]
async fn a_hold_reads_the_order_then_patches_only_the_status_against_the_version_it_read() {
    let mut s = mockito::Server::new_async().await;
    let _tokens = token_mocks(&mut s);
    let get = s
        .mock("GET", "/api/v3/SalesOrders/SO/000100")
        .match_header("authorization", "Bearer R")
        .with_header("etag", "\"v7\"")
        .with_body(OPEN_ORDER)
        .expect(1)
        .create_async()
        .await;
    let patch = s
        .mock("PATCH", "/api/v3/SalesOrders/SO/000100")
        .match_header("authorization", "Bearer W")
        .match_header("if-match", "\"v7\"")
        .match_body(Matcher::Json(json!({"status": "Hold"})))
        .with_status(200)
        .expect(1)
        .create_async()
        .await;
    let r = rig(&s);
    let input = json!({"type": "SO", "orderId": "000100", "status": "Hold"});

    let d = r
        .exec
        .dry_run(
            "ima",
            &TenantId::new(TENANT),
            "salesorder.set_status",
            &input,
        )
        .await
        .unwrap();
    get.assert_async().await;
    assert!(!patch.matched_async().await, "a plan must not PATCH");
    assert!(d.plan.summary.contains("on hold"), "{}", d.plan.summary);

    let out = r
        .exec
        .execute("ima", &d.plan, &d.confirm_token)
        .await
        .unwrap();
    patch.assert_async().await;
    assert_eq!(out.status, 200);
}

#[tokio::test]
async fn a_release_of_an_order_that_is_not_on_hold_never_reaches_the_patch_step() {
    let mut s = mockito::Server::new_async().await;
    let _tokens = token_mocks(&mut s);
    let _get = s
        .mock("GET", "/api/v3/SalesOrders/SO/000100")
        .with_header("etag", "\"v7\"")
        .with_body(OPEN_ORDER)
        .create_async()
        .await;
    let patch = s
        .mock("PATCH", "/api/v3/SalesOrders/SO/000100")
        .expect(0)
        .create_async()
        .await;
    let r = rig(&s);
    let input = json!({"type": "SO", "orderId": "000100", "status": "Open"});

    let res = r
        .exec
        .dry_run(
            "ima",
            &TenantId::new(TENANT),
            "salesorder.set_status",
            &input,
        )
        .await;
    assert!(matches!(res, Err(InteractiveError::InvalidInput(m)) if m.contains("On Hold")));
    patch.assert_async().await;
}

#[tokio::test]
async fn a_created_order_is_posted_on_hold_with_the_write_token_and_without_reading_anything() {
    let mut s = mockito::Server::new_async().await;
    let _tokens = token_mocks(&mut s);
    let get = s.mock("GET", Matcher::Any).expect(0).create_async().await;
    let post = s
        .mock("POST", "/api/v3/SalesOrders")
        .match_header("authorization", "Bearer W")
        .match_header("if-match", Matcher::Missing)
        .match_body(Matcher::Json(json!({
            "type": "SO",
            "status": "Hold",
            "customer": {"id": "C-1001"},
            "orderLines": [{"inventoryId": "ITEM-1", "quantity": 2}]})))
        .with_status(201)
        .with_header(
            "location",
            "https://salesorder.visma.net/api/v3/SalesOrders/SO/000321",
        )
        .with_body(r#"{"warnings":[]}"#)
        .expect(1)
        .create_async()
        .await;
    let r = rig(&s);
    let order = json!({
        "type": "SO",
        "customer": {"id": "C-1001"},
        "orderLines": [{"inventoryId": "ITEM-1", "quantity": 2}]});

    let d = r
        .exec
        .dry_run("ima", &TenantId::new(TENANT), "salesorder.create", &order)
        .await
        .unwrap();
    assert!(d.plan.summary.contains("(on hold)"), "{}", d.plan.summary);
    let out = r
        .exec
        .execute("ima", &d.plan, &d.confirm_token)
        .await
        .unwrap();
    post.assert_async().await;
    get.assert_async().await;
    assert_eq!(out.status, 201);
    assert_eq!(
        out.location.as_deref(),
        Some("https://salesorder.visma.net/api/v3/SalesOrders/SO/000321"),
        "the address of the new order is what Visma returns it in"
    );
}

#[tokio::test]
async fn nothing_reaches_visma_when_the_operation_is_not_allow_listed() {
    let s = mockito::Server::new_async().await; // no mocks: any request would 501
    let mut r = rig(&s);
    r.exec = Executor::new(
        default_registry(),
        WritePolicy::deny_all(),
        ConfirmSigner::new(b"e2e-test-confirm-secret-0123".to_vec()).unwrap(),
        r.sink.clone(),
        Arc::new(NeverTransport),
        Arc::new(NeverTransport),
    );
    let res = r
        .exec
        .dry_run("ima", &TenantId::new(TENANT), "salesorder.patch", &input())
        .await;
    assert!(matches!(res, Err(InteractiveError::Denied(_))));
}

struct NeverTransport;
#[async_trait::async_trait]
impl Transport for NeverTransport {
    async fn send(&self, _: &TenantId, _: &WriteRequest) -> Result<WriteResponse> {
        panic!("transport must not be called")
    }
}
#[async_trait::async_trait]
impl Reader for NeverTransport {
    async fn get(&self, _: &TenantId, _: WriteTarget, _: &str) -> Result<ReadResponse> {
        panic!("reader must not be called")
    }
}

#[tokio::test]
async fn a_write_that_is_answered_with_a_redirect_is_a_failure_and_the_redirect_is_not_followed() {
    let mut s = mockito::Server::new_async().await;
    let _tokens = token_mocks(&mut s);
    let _get = s
        .mock("GET", "/api/v3/SalesOrders/SO/000100")
        .with_header("etag", "\"v7\"")
        .with_body(OPEN_ORDER)
        .create_async()
        .await;
    let _moved = s
        .mock("PATCH", "/api/v3/SalesOrders/SO/000100")
        .with_status(307)
        .with_header("location", &format!("{}/elsewhere", s.url()))
        .create_async()
        .await;
    let elsewhere = s.mock("PATCH", "/elsewhere").expect(0).create_async().await;
    let r = rig(&s);
    let input = json!({"type": "SO", "orderId": "000100", "status": "Hold"});
    let d = r
        .exec
        .dry_run(
            "ima",
            &TenantId::new(TENANT),
            "salesorder.set_status",
            &input,
        )
        .await
        .unwrap();
    let res = r.exec.execute("ima", &d.plan, &d.confirm_token).await;
    assert!(
        matches!(res, Err(InteractiveError::Upstream { status: 307, .. })),
        "{res:?}"
    );
    elsewhere.assert_async().await;
    assert_eq!(r.sink.stages().last(), Some(&AuditStage::Failed));
}

#[tokio::test]
async fn a_write_scope_visma_refuses_stops_the_plan_before_anything_is_asked_or_sent() {
    let mut s = mockito::Server::new_async().await;
    let _read = s
        .mock("POST", "/token")
        .match_body(Matcher::Regex(
            "scope=visma.net.erp.salesorder%3Aread".into(),
        ))
        .with_body(r#"{"access_token":"R","expires_in":3600}"#)
        .create_async()
        .await;
    let _write = s
        .mock("POST", "/token")
        .match_body(Matcher::Regex(
            "scope=visma.net.erp.salesorder%3Awrite".into(),
        ))
        .with_status(400)
        .with_body(r#"{"error":"invalid_scope"}"#)
        .create_async()
        .await;
    let post = s
        .mock("POST", "/api/v3/SalesOrders")
        .expect(0)
        .create_async()
        .await;
    let r = rig(&s);
    let order = json!({"type": "SO", "customer": {"id": "C-1"}, "orderLines": [{"inventoryId": "I-1", "quantity": 1}]});
    let res = r
        .exec
        .dry_run("ima", &TenantId::new(TENANT), "salesorder.create", &order)
        .await;
    assert!(matches!(res, Err(InteractiveError::Common(_))), "{res:?}");
    assert_eq!(r.sink.stages(), vec![AuditStage::Denied]);
    post.assert_async().await;
}
