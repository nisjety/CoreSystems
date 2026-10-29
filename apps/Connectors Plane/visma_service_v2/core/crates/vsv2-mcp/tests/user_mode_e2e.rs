//! The server acting as a signed-in person, through the real binary: a sign-in prepared on disk, a mock
//! Visma (token endpoint and Finance API) and the MCP over stdio. No service credentials anywhere.

mod common;

use chrono::{Duration, Utc};
use common::McpClient;
use mockito::Matcher;
use serde_json::{json, Value};
use vsv2_signin::{Connection, Session, Store};

const READ: &str = "vismanet_erp_interactive_api:read";

fn data(r: &Value) -> &Value {
    assert_ne!(r["result"]["isError"], true, "tool failed: {r}");
    &r["result"]["structuredContent"]
}

/// The message of a refused call: a protocol error for bad arguments, or a tool result marked as an error.
fn failure_text(r: &Value) -> String {
    if let Some(message) = r["error"]["message"].as_str() {
        return message.to_string();
    }
    assert_eq!(r["result"]["isError"], true, "expected a failure: {r}");
    r["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

struct World {
    _dir: tempfile::TempDir,
    server: mockito::ServerGuard,
    store: Store,
    env: Vec<(String, String)>,
}

/// A sign-in for two companies on disk, the first with an access token that has run out, and the
/// environment that points the server at the mock.
async fn world(client_id: &str) -> World {
    let dir = tempfile::tempdir().unwrap();
    let server = mockito::Server::new_async().await;
    let store_dir = dir.path().join("user");
    let store = Store::new(store_dir.clone());
    store
        .save_session(&Session::new(
            "user-1".into(),
            Some("ima@example.com".into()),
            client_id.into(),
        ))
        .unwrap();
    let mut first = Connection::new(
        "t-1".into(),
        "user-1".into(),
        vec![READ.into()],
        "r1".into(),
        "old-access".into(),
        Utc::now() - Duration::seconds(10),
    );
    first.name = Some("Aquatiq AS".into());
    let mut second = Connection::new(
        "t-2".into(),
        "user-1".into(),
        vec![READ.into()],
        "r2".into(),
        "fresh-access-2".into(),
        Utc::now() + Duration::hours(1),
    );
    second.name = Some("Aqua Pharma Inc".into());
    store.save_connection(&first).unwrap();
    store.save_connection(&second).unwrap();
    let env = vec![
        (
            "VSV2_USER_STORE_DIR".to_string(),
            store_dir.display().to_string(),
        ),
        ("VISMA_USER_CLIENT_ID".to_string(), "web-app".to_string()),
        ("VISMA_USER_CLIENT_SECRET".to_string(), "s3cret".to_string()),
        (
            "VISMA_USER_TOKEN_URL".to_string(),
            format!("{}/token", server.url()),
        ),
        ("VISMA_ERP_REST_BASE_URL".to_string(), server.url()),
    ];
    World {
        _dir: dir,
        server,
        store,
        env,
    }
}

impl World {
    fn pairs(&self) -> Vec<(&str, &str)> {
        self.env
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect()
    }
}

#[tokio::test]
async fn the_server_acts_as_the_person_and_reads_each_company_with_that_companys_own_token() {
    let mut w = world("web-app").await;
    let refresh = w
        .server
        .mock("POST", "/token")
        .match_body(Matcher::AllOf(vec![
            Matcher::UrlEncoded("grant_type".into(), "refresh_token".into()),
            Matcher::UrlEncoded("refresh_token".into(), "r1".into()),
            Matcher::UrlEncoded("client_id".into(), "web-app".into()),
        ]))
        .with_body(
            r#"{"access_token":"new-access","refresh_token":"r1-rotated","expires_in":3600}"#,
        )
        .expect(1)
        .create_async()
        .await;
    let first = w
        .server
        .mock("GET", "/v1/customer")
        .match_query(Matcher::Any)
        .match_header("authorization", "Bearer new-access")
        .match_header("ipp-company-id", "t-1")
        .with_body(r#"[{"customerNumber":"C-1","name":"First","metadata":{"totalCount":1,"maxPageSize":1000}}]"#)
        .expect(1)
        .create_async()
        .await;
    let second = w
        .server
        .mock("GET", "/v1/customer")
        .match_query(Matcher::Any)
        .match_header("authorization", "Bearer fresh-access-2")
        .match_header("ipp-company-id", "t-2")
        .with_body(r#"[{"customerNumber":"C-2","name":"Second","metadata":{"totalCount":1,"maxPageSize":1000}}]"#)
        .expect(1)
        .create_async()
        .await;

    let mut c = McpClient::spawn(&w.pairs()).await;

    let init = c.call("visma_init", json!({})).await;
    let init = data(&init);
    assert!(
        init["acts_as"]
            .as_str()
            .unwrap()
            .contains("ima@example.com"),
        "{init}"
    );
    assert_eq!(init["capabilities"]["identity"]["mode"], "user");
    assert_eq!(
        init["capabilities"]["identity"]["companies"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(init["capabilities"]["live_visma"]["identity"], "user");
    assert_eq!(init["capabilities"]["datamart"]["enabled"], false, "{init}");
    assert!(
        !init.to_string().contains("s3cret") && !init.to_string().contains("fresh-access-2"),
        "no secret reaches the model"
    );

    let names: Vec<String> = c
        .tools()
        .await
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect();
    assert!(names.contains(&"visma_erp_query".to_string()), "{names:?}");
    assert!(
        !names.iter().any(|n| n.starts_with("visma_datamart")),
        "DataMart has no per-person sign-in: {names:?}"
    );

    let tenants = c.call("visma_list_tenants", json!({})).await;
    let listed: Vec<&str> = data(&tenants)["tenants"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        listed,
        ["Aqua Pharma Inc", "Aquatiq AS"],
        "only the connected companies, by the names Visma gave"
    );

    // No default company in a sign-in: the model must say which one.
    let nameless = c
        .call("visma_erp_query", json!({"endpoint": "customers"}))
        .await;
    assert!(
        failure_text(&nameless).contains("no default company"),
        "{nameless}"
    );
    let unknown = c
        .call(
            "visma_erp_query",
            json!({"endpoint": "customers", "tenant": "Aquatiq AB"}),
        )
        .await;
    assert!(
        failure_text(&unknown).contains("unknown tenant"),
        "{unknown}"
    );

    let one = c
        .call(
            "visma_erp_query",
            json!({"endpoint": "customers", "tenant": "Aquatiq AS", "page_size": 5}),
        )
        .await;
    assert!(data(&one).to_string().contains("C-1"), "{one}");
    let two = c
        .call(
            "visma_erp_query",
            json!({"endpoint": "customers", "tenant": "t-2", "page_size": 5}),
        )
        .await;
    assert!(data(&two).to_string().contains("C-2"), "{two}");

    refresh.assert_async().await;
    first.assert_async().await;
    second.assert_async().await;
    let saved = w.store.connection("t-1").unwrap().unwrap();
    assert_eq!(
        (saved.access_token.as_str(), saved.refresh_token.as_str()),
        ("new-access", "r1-rotated")
    );
}

#[tokio::test]
async fn a_sign_in_made_with_another_application_turns_the_live_tools_off() {
    let w = world("another-app").await;
    let mut c = McpClient::spawn(&w.pairs()).await;
    let init = c.call("visma_init", json!({})).await;
    let init = data(&init);
    assert_eq!(init["capabilities"]["identity"]["enabled"], false);
    assert!(
        init["capabilities"]["identity"]["reason"]
            .as_str()
            .unwrap()
            .contains("another application"),
        "{init}"
    );
    assert_eq!(init["capabilities"]["live_visma"]["enabled"], false);
    let names: Vec<String> = c
        .tools()
        .await
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect();
    assert!(!names.contains(&"visma_erp_query".to_string()), "{names:?}");
}

#[tokio::test]
async fn user_mode_without_a_sign_in_is_off_and_never_falls_back_to_the_service_credentials() {
    let empty = tempfile::tempdir().unwrap();
    let store = empty.path().join("nobody").display().to_string();
    let mut c = McpClient::spawn(&[
        ("VSV2_USER_STORE_DIR", store.as_str()),
        ("VSV2_MCP_IDENTITY", "user"),
        // Complete service credentials are on hand and must not be used.
        ("VISMA_CLIENT_ID", "service-id"),
        ("VISMA_CLIENT_SECRET", "service-secret"),
        ("VISMA_OAUTH_SCOPE", "vismanet_erp_service_api:read"),
        ("VSV2_DATABASE_URL", "postgres://u:p@127.0.0.1:1/none"),
        ("VSV2_REDIS_URL", "redis://127.0.0.1:1"),
    ])
    .await;
    let init = c.call("visma_init", json!({})).await;
    let init = data(&init);
    assert_eq!(init["capabilities"]["identity"]["enabled"], false);
    assert!(
        init["capabilities"]["identity"]["reason"]
            .as_str()
            .unwrap()
            .contains("vsv2-mcp login"),
        "{init}"
    );
    assert_eq!(init["capabilities"]["live_visma"]["enabled"], false);
    let names: Vec<String> = c
        .tools()
        .await
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect();
    assert!(!names.contains(&"visma_erp_query".to_string()), "{names:?}");
}

#[tokio::test]
async fn the_service_identity_ignores_a_sign_in_on_this_computer() {
    let w = world("web-app").await;
    let mut pairs = w.pairs();
    pairs.push(("VSV2_MCP_IDENTITY", "service"));
    let mut c = McpClient::spawn(&pairs).await;
    let init = c.call("visma_init", json!({})).await;
    let init = data(&init);
    assert!(
        init["acts_as"]
            .as_str()
            .unwrap()
            .contains("service credentials"),
        "{init}"
    );
    // There are no service credentials in this environment, so the live tools are off. Had the sign-in been used
    // they would be on: that is what proves it was ignored.
    assert_eq!(
        init["capabilities"]["live_visma"]["enabled"], false,
        "{init}"
    );
    assert!(
        init["capabilities"]["live_visma"]["reason"]
            .as_str()
            .unwrap()
            .contains("credentials"),
        "{init}"
    );
    assert!(init["capabilities"].get("identity").is_none(), "{init}");
    let names: Vec<String> = c
        .tools()
        .await
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect();
    assert!(!names.contains(&"visma_erp_query".to_string()), "{names:?}");
}
