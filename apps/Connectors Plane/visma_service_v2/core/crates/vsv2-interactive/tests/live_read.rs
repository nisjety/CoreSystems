//! Live, READ-ONLY check against the real Visma SalesOrder API. Ignored by default:
//!
//!   cargo test -p vsv2-interactive --test live_read -- --ignored --nocapture
//!
//! Uses only the read scope (list + single-order GET). It builds a `salesorder.patch` plan but
//! never calls `execute`, so nothing is written. Tokens are never printed.

use std::sync::Arc;

use serde_json::{json, Value};
use vsv2_auth::{OAuthClient, Scope};
use vsv2_common::config::AppConfig;
use vsv2_common::{Source, TenantId};
use vsv2_interactive::*;
use vsv2_ratelimit::Limiter;

#[tokio::test]
#[ignore = "hits the live Visma API (read-only)"]
async fn dry_run_against_a_real_order_reads_a_real_etag_and_status() {
    let cfg = AppConfig::from_env().expect("env (.env) must be loadable");
    let tenant = TenantId::new(cfg.aquatiq_tenant_id.clone());

    let oauth = OAuthClient::new(
        &cfg.oauth_token_url,
        &cfg.oauth_client_id,
        &cfg.oauth_client_secret,
    )
    .with_scope(Scope::SalesOrder, cfg.oauth_scope_salesorder.clone());
    let http = Arc::new(HttpTransport::new(
        oauth,
        Limiter::new(Source::ErpRest, 30, 1000),
        &cfg.erp_base_url,
        "https://salesorder.visma.net",
    ));

    // Pick one real order from the list endpoint.
    let list = http
        .get(
            &tenant,
            WriteTarget::SalesOrder,
            "/api/v3/SalesOrders?pageSize=1&pageIndex=0",
        )
        .await
        .expect("list call");
    assert_eq!(list.status, 200, "list must succeed");
    let first: Value = serde_json::from_str(&list.body).unwrap();
    let order = &first["value"][0];
    let (otype, oid) = (
        order["type"].as_str().unwrap(),
        order["orderId"].as_str().unwrap(),
    );

    let exec = Executor::new(
        default_registry(),
        WritePolicy::new([tenant.to_string()], ["salesorder.patch"]),
        ConfirmSigner::new(b"live-read-test-secret-0123456".to_vec()).unwrap(),
        Arc::new(MemoryAuditSink::default()),
        http.clone(), // never used: execute is not called
        http,
    );
    let input = json!({"type": otype, "orderId": oid, "changes": {"description": "LIVE-READ-TEST (never sent)"}});
    let res = exec
        .dry_run("live-read-test", &tenant, "salesorder.patch", &input)
        .await;

    match res {
        Ok(d) => {
            let etag = d.plan.request.if_match.as_deref().unwrap_or("");
            assert!(!etag.is_empty() && etag != "*****");
            assert!(
                !d.plan.summary.contains("status unknown"),
                "status must parse: {}",
                d.plan.summary
            );
            println!("LIVE OK: {}", d.plan.summary);
            println!(
                "  PATCH {} | If-Match len {} | sent: nothing",
                d.plan.request.path,
                etag.len()
            );
        }
        // A genuinely closed order is a valid refusal, not a parsing failure.
        Err(InteractiveError::InvalidInput(m)) if m.contains("closed orders") => {
            println!("LIVE OK (order is closed, correctly refused): {m}");
        }
        Err(e) => panic!("unexpected live failure: {e}"),
    }
}
