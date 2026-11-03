//! The durable hook-rule proxy refuses a ZDR or non-admin caller before it sends anything, and an admin's
//! write reaches capability-core under the caller's own bearer (WS25-S8). Mirrors
//! `issuer_zdr_capability_mutations_do_not_forward_or_touch_mcp_registry` (WS05-S5).

use model_gateway::runtime_registries::{
    proxy_list_hooks, proxy_register_hook, HookRegistry, HookWriteCaller,
};
use mp_contracts::model_plane::v1::{Hook, ListHooksRequest, RegisterHookRequest};
use serde_json::json;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn register(org_id: &str) -> RegisterHookRequest {
    RegisterHookRequest {
        request_id: "req-1".to_owned(),
        org_id: org_id.to_owned(),
        hook: Some(Hook {
            event: "pre_tool".to_owned(),
            tool_scope: "shell".to_owned(),
            enabled: true,
            decision: "deny".to_owned(),
            reason: "no shell".to_owned(),
            priority: 10,
            ..Hook::default()
        }),
    }
}

async fn upstream_calls(server: &MockServer) -> usize {
    server.received_requests().await.unwrap_or_default().len()
}

#[tokio::test]
async fn a_zdr_principal_never_stores_a_hook_rule_upstream() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/org-hooks"))
        .respond_with(ResponseTemplate::new(201))
        .mount(&server)
        .await;
    let reg = HookRegistry::new();

    let refused = proxy_register_hook(
        &reg,
        &reqwest::Client::new(),
        &server.uri(),
        "admin-token",
        HookWriteCaller {
            zdr: true,
            admin: true,
        },
        register("org-a"),
    )
    .await
    .expect_err("a ZDR principal's rule write must fail locally");
    assert_eq!(refused.code(), tonic::Code::FailedPrecondition);
    assert_eq!(upstream_calls(&server).await, 0, "a ZDR rule write was forwarded");
}

#[tokio::test]
async fn a_non_admin_never_stores_a_hook_rule_upstream() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/org-hooks"))
        .respond_with(ResponseTemplate::new(201))
        .mount(&server)
        .await;
    let reg = HookRegistry::new();

    let refused = proxy_register_hook(
        &reg,
        &reqwest::Client::new(),
        &server.uri(),
        "member-token",
        HookWriteCaller {
            zdr: false,
            admin: false,
        },
        register("org-a"),
    )
    .await
    .expect_err("a member's rule write must fail locally");
    assert_eq!(refused.code(), tonic::Code::PermissionDenied);
    assert_eq!(upstream_calls(&server).await, 0, "a member's rule write was forwarded");
}

#[tokio::test]
async fn an_admin_write_forwards_the_callers_bearer_and_reads_the_rules_back() {
    let server = MockServer::start().await;
    let rule = json!({
        "id": "ohr_1", "event": "pre_tool", "tool_matcher": "shell", "decision": "deny",
        "reason": "no shell", "priority": 10, "enabled": true, "created_by": "user-admin",
        "updated_by": "user-admin", "created_at": "2026-11-01T00:00:00Z",
        "updated_at": "2026-11-01T00:00:00Z"
    });
    let mut stored = rule.clone();
    stored["revision"] = json!(4);
    Mock::given(method("POST"))
        .and(path("/api/v1/org-hooks"))
        .and(header("authorization", "Bearer admin-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(stored))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/org-hooks"))
        .and(header("authorization", "Bearer admin-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "org_id": "org-a",
            "revision": 4,
            "rules": [rule],
        })))
        .mount(&server)
        .await;
    let reg = HookRegistry::new();
    let client = reqwest::Client::new();

    let answer = proxy_register_hook(
        &reg,
        &client,
        &server.uri(),
        "admin-token",
        HookWriteCaller {
            zdr: false,
            admin: true,
        },
        register("org-a"),
    )
    .await
    .expect("an admin's write is forwarded");
    let written = answer.hook.expect("the stored rule");
    assert_eq!((written.hook_id.as_str(), written.revision), ("ohr_1", 4));

    let listed = proxy_list_hooks(
        &reg,
        &client,
        &server.uri(),
        "admin-token",
        ListHooksRequest {
            request_id: "req-2".to_owned(),
            org_id: "org-a".to_owned(),
            event_filter: String::new(),
        },
    )
    .await
    .expect("the rules are listed");
    assert_eq!(listed.hooks.len(), 1);
    assert_eq!(listed.hooks[0].hook_id, "ohr_1");
}
