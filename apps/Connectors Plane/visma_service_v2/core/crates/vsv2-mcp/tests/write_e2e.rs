//! Full path through the real binary: MCP stdio -> write tools -> human approval (MCP elicitation) ->
//! Layer 3 executor -> Postgres audit -> HTTP transport -> (mock) Visma. Needs a SCRATCH database:
//!   VSV2_TEST_DATABASE_URL=postgres://.../scratch cargo test -p vsv2-mcp --test write_e2e -- --ignored --test-threads=1
//! The tests that need the database are `#[ignore]`d, so a plain `cargo test` does not claim to have run them: add
//! `-- --ignored`. They create the `vsv2` schema and apply migration 0012.

mod common;

use common::{Elicit, McpClient};
use mockito::Matcher;
use serde_json::{json, Value};
use sqlx::postgres::PgPoolOptions;

const TENANT: &str = "11111111-2222-4333-8444-555555555555";

async fn scratch_pool() -> Option<(sqlx::PgPool, String)> {
    let url = std::env::var("VSV2_TEST_DATABASE_URL")
        .expect("VSV2_TEST_DATABASE_URL must point at a scratch Postgres for these tests");
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .unwrap();
    sqlx::raw_sql("CREATE SCHEMA IF NOT EXISTS vsv2")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::raw_sql(include_str!("../../../../migrations/0012_write_audit.sql"))
        .execute(&pool)
        .await
        .unwrap();
    Some((pool, url))
}

fn tenant_dir() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("e2e.json"),
        json!({"tenant_id": TENANT, "tenant_name": "E2E Tenant"}).to_string(),
    )
    .unwrap();
    d
}

fn env_for(
    server_url: &str,
    db: &str,
    tenants: &str,
    allow_tenants: &str,
    allow_ops: &str,
) -> Vec<(String, String)> {
    let kv = |k: &str, v: &str| (k.to_string(), v.to_string());
    vec![
        kv("VISMA_CLIENT_ID", "cid"),
        kv("VISMA_CLIENT_SECRET", "secret"),
        kv("VISMA_SCOPES", "erp:read"),
        kv("VISMA_OAUTH_SCOPE_SALESORDER", "so:read"),
        kv("VISMA_OAUTH_SCOPE_SALESORDER_WRITE", "so:write"),
        kv("VISMA_TOKEN_URL", &format!("{server_url}/token")),
        kv("VISMA_ERP_REST_BASE_URL", server_url),
        kv("VSV2_MCP_SALESORDER_BASE_URL", server_url),
        kv("VSV2_DATABASE_URL", db),
        kv("VSV2_REDIS_URL", "redis://127.0.0.1:1"),
        kv("VSV2_PILOT_TENANT_ID", TENANT),
        kv("VSV2_TENANT_CONFIG_DIR", tenants),
        kv("VSV2_MCP_ENABLE_WRITES", "1"),
        kv("VSV2_WRITE_CONFIRM_SECRET", "e2e-confirm-secret-0123456789"),
        kv("VSV2_WRITE_ENABLED_TENANTS", allow_tenants),
        kv("VSV2_WRITE_ENABLED_OPERATIONS", allow_ops),
        kv("VSV2_MCP_CALLER", "e2e-caller"),
    ]
}

async fn spawn(env: &[(String, String)], elicit: Elicit) -> McpClient {
    let pairs: Vec<(&str, &str)> = env.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    McpClient::spawn_with(&pairs, elicit).await
}

fn ok(r: &Value) -> &Value {
    assert_ne!(r["result"]["isError"], true, "tool failed: {r}");
    &r["result"]["structuredContent"]
}

fn err_text(r: &Value) -> String {
    assert_eq!(r["result"]["isError"], true, "expected tool error: {r}");
    r["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

fn mock_tokens(s: &mut mockito::ServerGuard) -> Vec<mockito::Mock> {
    vec![
        s.mock("POST", "/token")
            .match_body(Matcher::Regex("scope=so%3Aread".into()))
            .with_body(r#"{"access_token":"R","expires_in":3600}"#)
            .create(),
        s.mock("POST", "/token")
            .match_body(Matcher::Regex("scope=so%3Awrite".into()))
            .with_body(r#"{"access_token":"W","expires_in":3600}"#)
            .create(),
    ]
}

const ORDER: &str =
    r#"{"orderId":"000100","status":{"description":"Open","cancelled":false,"hold":false}}"#;

fn change() -> Value {
    json!({"type": "SO", "orderId": "000100", "changes": {"description": "edited via MCP"}})
}

fn exec_args(p: &Value) -> Value {
    json!({"plan": p["plan"], "confirm_token": p["confirm_token"]})
}

#[tokio::test]
#[ignore = "needs a scratch Postgres: set VSV2_TEST_DATABASE_URL and run with --ignored"]
async fn user_approves_in_the_client_then_the_write_is_applied_once_and_audited() {
    let Some((pool, db)) = scratch_pool().await else {
        eprintln!("skipped: VSV2_TEST_DATABASE_URL not set");
        return;
    };
    let mut s = mockito::Server::new_async().await;
    let _tokens = mock_tokens(&mut s);
    let _get = s
        .mock("GET", "/api/v3/SalesOrders/SO/000100")
        .match_header("authorization", "Bearer R")
        .with_header("etag", "\"v7\"")
        .with_body(ORDER)
        .create_async()
        .await;
    let patch = s
        .mock("PATCH", "/api/v3/SalesOrders/SO/000100")
        .match_header("authorization", "Bearer W")
        .match_header("if-match", "\"v7\"")
        .match_body(Matcher::Json(json!({"description": "edited via MCP"})))
        .with_status(202)
        .expect(1)
        .create_async()
        .await;
    let td = tenant_dir();
    let env = env_for(
        &s.url(),
        &db,
        td.path().to_str().unwrap(),
        TENANT,
        "salesorder.patch",
    );
    let mut c = spawn(&env, Elicit::Accept).await;

    // Registered with the right safety annotations, and the execute schema has no model-settable approval flag.
    let tools = c.tools().await;
    let by = |n: &str| {
        tools
            .iter()
            .find(|t| t["name"] == n)
            .unwrap_or_else(|| panic!("{n} missing"))
            .clone()
    };
    assert_eq!(
        by("visma_salesorder_patch_plan")["annotations"]["readOnlyHint"],
        false
    );
    assert_eq!(
        by("visma_salesorder_patch_plan")["annotations"]["destructiveHint"],
        false
    );
    assert_eq!(
        by("visma_salesorder_patch_execute")["annotations"]["destructiveHint"],
        true
    );
    assert_eq!(
        by("visma_salesorder_patch_execute")["inputSchema"]["required"],
        json!(["plan", "confirm_token"])
    );

    let plan = c.call("visma_salesorder_patch_plan", change()).await;
    let p = ok(&plan).clone();
    println!("plan summary: {}", p["summary"]);
    assert!(!patch.matched_async().await, "planning must not PATCH");
    assert!(c.elicitations.is_empty(), "planning never prompts the user");

    let done = c
        .call("visma_salesorder_patch_execute", exec_args(&p))
        .await;
    assert_eq!(ok(&done)["http_status"], 202);
    patch.assert_async().await;
    assert_eq!(c.elicitations.len(), 1, "the user was asked exactly once");
    let shown = &c.elicitations[0];
    println!("user was shown:\n{shown}");
    assert!(
        shown.contains("SO/000100")
            && shown.contains("edited via MCP")
            && shown.contains("PATCH /api/v3/SalesOrders/SO/000100")
    );

    // Replaying the same plan is refused without asking again and sends nothing more.
    assert!(err_text(
        &c.call("visma_salesorder_patch_execute", exec_args(&p))
            .await
    )
    .contains("already executed"));
    assert_eq!(c.elicitations.len(), 1);
    patch.assert_async().await;

    // Audit trail in Postgres: stages, caller, and no secret/token material.
    let plan_id = p["plan"]["plan_id"].as_str().unwrap().to_string();
    let rows: Vec<(String, String, Option<i32>)> = sqlx::query_as(
        "SELECT stage, caller, http_status FROM vsv2.write_audit WHERE plan_id = $1::uuid AND stage <> 'denied' ORDER BY created_at, audit_id",
    )
    .bind(&plan_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    println!("audit: {rows:?}");
    let stages: Vec<&str> = rows.iter().map(|r| r.0.as_str()).collect();
    assert_eq!(stages, ["dry_run", "executing", "succeeded"]);
    assert!(rows.iter().all(|r| r.1 == "e2e-caller"));
    let leaked: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM vsv2.write_audit WHERE detail ILIKE '%Bearer%' OR payload::text ILIKE '%secret%'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(leaked, 0);
}

#[tokio::test]
#[ignore = "needs a scratch Postgres: set VSV2_TEST_DATABASE_URL and run with --ignored"]
async fn nothing_is_written_when_the_user_declines_or_the_client_cannot_ask() {
    let Some((pool, db)) = scratch_pool().await else {
        eprintln!("skipped: VSV2_TEST_DATABASE_URL not set");
        return;
    };
    let mut s = mockito::Server::new_async().await;
    let _tokens = mock_tokens(&mut s);
    let _get = s
        .mock("GET", "/api/v3/SalesOrders/SO/000100")
        .with_header("etag", "\"v7\"")
        .with_body(ORDER)
        .create_async()
        .await;
    let patch = s.mock("PATCH", Matcher::Any).expect(0).create_async().await;
    let td = tenant_dir();
    let env = env_for(
        &s.url(),
        &db,
        td.path().to_str().unwrap(),
        TENANT,
        "salesorder.patch",
    );

    // The user says no.
    let mut c = spawn(&env, Elicit::Decline).await;
    let p = ok(&c.call("visma_salesorder_patch_plan", change()).await).clone();
    // The model "approves" on the user's behalf with the old flag: it changes nothing.
    let mut claimed = exec_args(&p);
    claimed["user_approved"] = json!(true);
    let r = c.call("visma_salesorder_patch_execute", claimed).await;
    assert!(err_text(&r).contains("did not approve"), "{r}");
    assert_eq!(
        c.elicitations.len(),
        1,
        "the user was still asked, and said no"
    );

    // A client that cannot ask the user: fail closed, with advice for the operator.
    let mut c2 = spawn(&env, Elicit::Unsupported).await;
    let p2 = ok(&c2.call("visma_salesorder_patch_plan", change()).await).clone();
    let r2 = c2
        .call("visma_salesorder_patch_execute", exec_args(&p2))
        .await;
    let msg = err_text(&r2);
    assert!(
        msg.contains("cannot ask the user") || msg.contains("does not support elicitation"),
        "{msg}"
    );
    assert!(
        msg.contains("permission_prompt"),
        "tells the operator how to opt in: {msg}"
    );

    patch.assert_async().await; // zero PATCHes, ever
    let executing: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM vsv2.write_audit WHERE stage = 'executing' AND plan_id IN ($1::uuid, $2::uuid)")
        .bind(p["plan"]["plan_id"].as_str().unwrap())
        .bind(p2["plan"]["plan_id"].as_str().unwrap())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(executing, 0, "no execution was ever claimed");
}

#[tokio::test]
#[ignore = "needs a scratch Postgres: set VSV2_TEST_DATABASE_URL and run with --ignored"]
async fn permission_prompt_mode_is_an_explicit_opt_in_that_needs_the_flag() {
    let Some((_pool, db)) = scratch_pool().await else {
        eprintln!("skipped: VSV2_TEST_DATABASE_URL not set");
        return;
    };
    let mut s = mockito::Server::new_async().await;
    let _tokens = mock_tokens(&mut s);
    let _get = s
        .mock("GET", "/api/v3/SalesOrders/SO/000100")
        .with_header("etag", "\"v7\"")
        .with_body(ORDER)
        .create_async()
        .await;
    let patch = s
        .mock("PATCH", "/api/v3/SalesOrders/SO/000100")
        .with_status(202)
        .expect(1)
        .create_async()
        .await;
    let td = tenant_dir();
    let mut env = env_for(
        &s.url(),
        &db,
        td.path().to_str().unwrap(),
        TENANT,
        "salesorder.patch",
    );
    env.push(("VSV2_MCP_WRITE_APPROVAL".into(), "permission_prompt".into()));
    let mut c = spawn(&env, Elicit::Unsupported).await;

    let tools = c.tools().await;
    let exec = tools
        .iter()
        .find(|t| t["name"] == "visma_salesorder_patch_execute")
        .unwrap();
    assert_eq!(
        exec["inputSchema"]["required"],
        json!(["plan", "confirm_token", "user_approved"])
    );

    let p = ok(&c.call("visma_salesorder_patch_plan", change()).await).clone();
    let mut args = exec_args(&p);
    args["user_approved"] = json!(false);
    assert!(
        err_text(&c.call("visma_salesorder_patch_execute", args).await).contains("user_approved")
    );
    let mut args = exec_args(&p);
    args["user_approved"] = json!(true);
    assert_eq!(
        ok(&c.call("visma_salesorder_patch_execute", args).await)["http_status"],
        202
    );
    patch.assert_async().await;
}

#[tokio::test]
#[ignore = "needs a scratch Postgres: set VSV2_TEST_DATABASE_URL and run with --ignored"]
async fn writes_are_refused_when_the_tenant_or_operation_is_not_allow_listed() {
    let Some((pool, db)) = scratch_pool().await else {
        eprintln!("skipped: VSV2_TEST_DATABASE_URL not set");
        return;
    };
    let mut s = mockito::Server::new_async().await;
    let _tokens = mock_tokens(&mut s);
    let get = s.mock("GET", Matcher::Any).expect(0).create_async().await; // not even a read happens
    let td = tenant_dir();
    let env = env_for(&s.url(), &db, td.path().to_str().unwrap(), "", "");
    let mut c = spawn(&env, Elicit::Accept).await;

    let r = c.call("visma_salesorder_patch_plan", change()).await;
    assert!(err_text(&r).contains("write denied"), "{r}");
    get.assert_async().await;
    assert!(c.elicitations.is_empty());
    let denied: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM vsv2.write_audit WHERE stage = 'denied' AND caller = 'e2e-caller'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(denied >= 1, "the denial itself is audited");
}

#[tokio::test]
#[ignore = "needs a scratch Postgres: set VSV2_TEST_DATABASE_URL and run with --ignored"]
async fn write_tools_are_absent_unless_fully_and_correctly_configured() {
    let Some((pool, db)) = scratch_pool().await else {
        eprintln!("skipped: VSV2_TEST_DATABASE_URL not set");
        return;
    };
    let mut s = mockito::Server::new_async().await;
    let _tokens = mock_tokens(&mut s);
    let td = tenant_dir();
    let url = s.url();
    let dir = td.path().to_str().unwrap();
    let has_write = |tools: &[Value]| {
        tools
            .iter()
            .any(|t| t["name"].as_str().unwrap().contains("patch"))
    };

    // Flag off.
    let mut env = env_for(&url, &db, dir, TENANT, "salesorder.patch");
    env.retain(|(k, _)| k != "VSV2_MCP_ENABLE_WRITES");
    let mut c = spawn(&env, Elicit::Accept).await;
    let tools = c.tools().await;
    assert!(
        tools.iter().any(|t| t["name"] == "visma_erp_query"),
        "read tools are there"
    );
    assert!(!has_write(&tools), "no write tool without the flag");

    // Missing confirm secret.
    let mut env = env_for(&url, &db, dir, TENANT, "salesorder.patch");
    env.retain(|(k, _)| k != "VSV2_WRITE_CONFIRM_SECRET");
    assert!(!has_write(&spawn(&env, Elicit::Accept).await.tools().await));

    // Audit database unreachable: nothing half-enabled (the old lazy connect would have registered them).
    let env = env_for(
        &url,
        "postgres://u:p@127.0.0.1:1/none",
        dir,
        TENANT,
        "salesorder.patch",
    );
    assert!(!has_write(&spawn(&env, Elicit::Accept).await.tools().await));

    // Audit table missing (migration not applied): same.
    sqlx::raw_sql("ALTER TABLE vsv2.write_audit RENAME TO write_audit_moved")
        .execute(&pool)
        .await
        .unwrap();
    let env = env_for(&url, &db, dir, TENANT, "salesorder.patch");
    let hidden = !has_write(&spawn(&env, Elicit::Accept).await.tools().await);
    sqlx::raw_sql("ALTER TABLE vsv2.write_audit_moved RENAME TO write_audit")
        .execute(&pool)
        .await
        .unwrap();
    assert!(hidden, "no write tool while vsv2.write_audit is missing");
}

#[tokio::test]
async fn an_unsafe_salesorder_host_stops_the_server_from_starting() {
    // Bearer tokens must never be sent to a non-Visma host because of a stray environment variable.
    let td = tenant_dir();
    let env = env_for(
        "https://evil.example",
        "postgres://u:p@127.0.0.1:1/none",
        td.path().to_str().unwrap(),
        TENANT,
        "salesorder.patch",
    );
    let pairs: Vec<(&str, &str)> = env.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let mut cmd = tokio::process::Command::new(env!("CARGO_BIN_EXE_vsv2-mcp"));
    cmd.env_clear()
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped());
    for (k, v) in pairs {
        cmd.env(k, v);
    }
    cmd.env(
        "VSV2_ENDPOINT_CONFIG_DIR",
        format!("{}/config/endpoints", common::repo_root()),
    );
    let out = cmd.output().await.unwrap();
    assert!(!out.status.success(), "must refuse to start");
    assert!(String::from_utf8_lossy(&out.stderr).contains("must be https on a *.visma.net host"));
}

const NEW_ORDER_LOCATION: &str = "/api/v3/SalesOrders/SO/000321";

fn new_order() -> Value {
    json!({
        "type": "SO",
        "customer": {"id": "C-1001"},
        "orderLines": [{"inventoryId": "ITEM-1", "quantity": 2}]
    })
}

async fn stages(pool: &sqlx::PgPool, plan_id: &str) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT stage FROM vsv2.write_audit WHERE plan_id = $1::uuid AND stage <> 'denied' ORDER BY created_at, audit_id",
    )
    .bind(plan_id)
    .fetch_all(pool)
    .await
    .unwrap()
}

#[ignore = "needs a scratch Postgres: set VSV2_TEST_DATABASE_URL and run with --ignored"]
#[tokio::test]
async fn an_order_is_created_on_hold_after_approval_and_the_new_order_is_reported() {
    let Some((pool, db)) = scratch_pool().await else {
        return;
    };
    let mut s = mockito::Server::new_async().await;
    let _tokens = mock_tokens(&mut s);
    let post = s
        .mock("POST", "/api/v3/SalesOrders")
        .match_header("authorization", "Bearer W")
        .match_header("if-match", Matcher::Missing)
        .match_body(Matcher::Json(json!({
            "type": "SO", "status": "Hold",
            "customer": {"id": "C-1001"},
            "orderLines": [{"inventoryId": "ITEM-1", "quantity": 2}]
        })))
        .with_status(201)
        .with_header("location", &format!("{}{NEW_ORDER_LOCATION}", s.url()))
        .with_body(r#"{"warnings":[]}"#)
        .expect(1)
        .create_async()
        .await;
    let td = tenant_dir();
    let env = env_for(
        &s.url(),
        &db,
        td.path().to_str().unwrap(),
        TENANT,
        "salesorder.create",
    );
    let mut c = spawn(&env, Elicit::Accept).await;

    let planned = c
        .call(
            "visma_salesorder_create_plan",
            json!({"order": new_order()}),
        )
        .await;
    let p = ok(&planned).clone();
    assert!(p["summary"].as_str().unwrap().contains("(on hold)"), "{p}");
    assert!(!post.matched_async().await, "planning must not POST");
    assert!(c.elicitations.is_empty());

    let done = c
        .call("visma_salesorder_create_execute", exec_args(&p))
        .await;
    let done = ok(&done);
    assert_eq!(done["http_status"], 201);
    assert_eq!(
        done["created_order"],
        json!({"type": "SO", "orderId": "000321"}),
        "{done}"
    );
    assert!(
        done["location"]
            .as_str()
            .unwrap()
            .ends_with(NEW_ORDER_LOCATION),
        "{done}"
    );
    post.assert_async().await;
    assert_eq!(c.elicitations.len(), 1);
    let shown = &c.elicitations[0];
    assert!(
        shown.contains(&format!("Company: E2E Tenant ({TENANT})"))
            && shown.contains("ITEM-1")
            && shown.contains("POST /api/v3/SalesOrders"),
        "{shown}"
    );

    // Replay: refused, nothing more sent. The audit trail names the order that was made.
    assert!(err_text(
        &c.call("visma_salesorder_create_execute", exec_args(&p))
            .await
    )
    .contains("already executed"));
    let plan_id = p["plan"]["plan_id"].as_str().unwrap().to_string();
    assert_eq!(
        stages(&pool, &plan_id).await,
        ["dry_run", "executing", "succeeded"]
    );
    let detail: String = sqlx::query_scalar(
        "SELECT detail FROM vsv2.write_audit WHERE plan_id = $1::uuid AND stage = 'succeeded'",
    )
    .bind(&plan_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(detail.contains(NEW_ORDER_LOCATION), "{detail}");
}

#[ignore = "needs a scratch Postgres: set VSV2_TEST_DATABASE_URL and run with --ignored"]
#[tokio::test]
async fn an_open_order_is_put_on_hold_and_released_each_step_approved_and_against_the_version_read()
{
    let Some((pool, db)) = scratch_pool().await else {
        return;
    };
    let held =
        r#"{"orderId":"000100","status":{"description":"On Hold","cancelled":false,"hold":true}}"#;
    let mut s = mockito::Server::new_async().await;
    let _tokens = mock_tokens(&mut s);
    let _open = s
        .mock("GET", "/api/v3/SalesOrders/SO/000100")
        .with_header("etag", "\"v1\"")
        .with_body(ORDER)
        .expect(1)
        .create_async()
        .await;
    let hold = s
        .mock("PATCH", "/api/v3/SalesOrders/SO/000100")
        .match_header("authorization", "Bearer W")
        .match_header("if-match", "\"v1\"")
        .match_body(Matcher::Json(json!({"status": "Hold"})))
        .with_status(202)
        .expect(1)
        .create_async()
        .await;
    let _held = s
        .mock("GET", "/api/v3/SalesOrders/SO/000100")
        .with_header("etag", "\"v2\"")
        .with_body(held)
        .expect(1)
        .create_async()
        .await;
    let release = s
        .mock("PATCH", "/api/v3/SalesOrders/SO/000100")
        .match_header("authorization", "Bearer W")
        .match_header("if-match", "\"v2\"")
        .match_body(Matcher::Json(json!({"status": "Open"})))
        .with_status(202)
        .expect(1)
        .create_async()
        .await;
    let td = tenant_dir();
    let env = env_for(
        &s.url(),
        &db,
        td.path().to_str().unwrap(),
        TENANT,
        "salesorder.set_status",
    );
    let mut c = spawn(&env, Elicit::Accept).await;

    let planned = c
        .call(
            "visma_salesorder_status_plan",
            json!({"type": "SO", "orderId": "000100", "status": "Hold"}),
        )
        .await;
    let p = ok(&planned).clone();
    assert!(p["summary"].as_str().unwrap().contains("on hold"), "{p}");
    assert!(!hold.matched_async().await);
    ok(&c
        .call("visma_salesorder_status_execute", exec_args(&p))
        .await);
    hold.assert_async().await;

    let planned = c
        .call(
            "visma_salesorder_status_plan",
            json!({"type": "SO", "orderId": "000100", "status": "Open"}),
        )
        .await;
    let p2 = ok(&planned).clone();
    assert!(p2["summary"].as_str().unwrap().contains("Release"), "{p2}");
    ok(&c
        .call("visma_salesorder_status_execute", exec_args(&p2))
        .await);
    release.assert_async().await;
    assert_eq!(c.elicitations.len(), 2, "each change was put to the person");
    assert_eq!(
        stages(&pool, p["plan"]["plan_id"].as_str().unwrap()).await,
        ["dry_run", "executing", "succeeded"]
    );
    assert_eq!(
        stages(&pool, p2["plan"]["plan_id"].as_str().unwrap()).await,
        ["dry_run", "executing", "succeeded"]
    );
}

#[ignore = "needs a scratch Postgres: set VSV2_TEST_DATABASE_URL and run with --ignored"]
#[tokio::test]
async fn a_write_scope_visma_refuses_stops_the_plan_before_the_person_is_asked() {
    let Some((pool, db)) = scratch_pool().await else {
        return;
    };
    let mut s = mockito::Server::new_async().await;
    let _read = s
        .mock("POST", "/token")
        .match_body(Matcher::Regex("scope=so%3Aread".into()))
        .with_body(r#"{"access_token":"R","expires_in":3600}"#)
        .create_async()
        .await;
    let _write = s
        .mock("POST", "/token")
        .match_body(Matcher::Regex("scope=so%3Awrite".into()))
        .with_status(400)
        .with_body(r#"{"error":"invalid_scope"}"#)
        .create_async()
        .await;
    let post = s
        .mock("POST", "/api/v3/SalesOrders")
        .expect(0)
        .create_async()
        .await;
    let td = tenant_dir();
    let env = env_for(
        &s.url(),
        &db,
        td.path().to_str().unwrap(),
        TENANT,
        "salesorder.create",
    );
    let mut c = spawn(&env, Elicit::Accept).await;
    let planned = c
        .call(
            "visma_salesorder_create_plan",
            json!({"order": new_order()}),
        )
        .await;
    let why = err_text(&planned);
    assert!(
        why.contains("invalid_scope") || why.contains("scope"),
        "{why}"
    );
    assert!(
        c.elicitations.is_empty(),
        "nobody is asked to approve what could not be sent"
    );
    post.assert_async().await;
    let sent: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM vsv2.write_audit WHERE operation = 'salesorder.create' AND stage IN ('executing','succeeded')")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(sent, 0);
}
