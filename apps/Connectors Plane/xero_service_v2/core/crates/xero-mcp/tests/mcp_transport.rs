use async_trait::async_trait;
use chrono::Utc;
use rmcp::{
    model::{CallToolRequestParams, ClientConfig, ReadResourceRequestParams},
    ServiceExt,
};
use serde_json::{json, Value};
use std::{
    path::Path,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use xero_mcp::{
    backend::{
        Backend, ConnectionInfo, Failure, Query, Response, WarehouseOverview, WarehouseTableCount,
    },
    config::{AccessScope, Settings},
    jobs::{Destination, ExtractionPlan},
    server::XeroMcp,
    transport,
};

const FIRST: &str = "5f3c9a2e-0000-0000-0000-000000000000";
const SECOND: &str = "aeb4f270-0000-0000-0000-000000000000";
struct Fixture {
    fetches: AtomicUsize,
}
impl Fixture {
    fn new() -> Self {
        Self {
            fetches: AtomicUsize::new(0),
        }
    }
}
#[async_trait]
impl Backend for Fixture {
    async fn connections(&self) -> Result<Vec<ConnectionInfo>, Failure> {
        Ok([FIRST, SECOND]
            .into_iter()
            .map(|id| ConnectionInfo {
                tenant_id: id.into(),
                tenant_name: "Fixture".into(),
                scopes: vec!["accounting.transactions.read".into()],
                auth_mode: "fixture".into(),
            })
            .collect())
    }
    async fn fetch(&self, _: &Query) -> Result<Response, Failure> {
        self.fetches.fetch_add(1, Ordering::SeqCst);
        Ok(Response {
            body: br#"{"Invoices":[{"InvoiceID":"fixture-one"},{"InvoiceID":"fixture-two"}]}"#
                .as_slice()
                .into(),
            content_type: "application/json".into(),
            http_status: 200,
            fetched_at: Utc::now(),
        })
    }
    async fn extract(
        &self,
        _: Uuid,
        _: &str,
        _: &ExtractionPlan,
        _: Destination,
        _: &Path,
    ) -> Result<Value, Failure> {
        Ok(json!({"complete":true,"outcomes":[]}))
    }
    async fn warehouse_overview(
        &self,
        tenant_ids: &[String],
    ) -> Result<WarehouseOverview, Failure> {
        Ok(WarehouseOverview {
            project_id: "warehouse-project".into(),
            dataset_id: "dw_staging_xero".into(),
            location: "europe-north2".into(),
            tables: vec![WarehouseTableCount {
                table_name: "invoices".into(),
                row_count: tenant_ids.len() as u64,
            }],
        })
    }
}

async fn rpc(
    client: &reqwest::Client,
    url: &str,
    token: &str,
    id: i64,
    method: &str,
    params: Value,
) -> Value {
    let response = client
        .post(url)
        .bearer_auth(token)
        .header("Accept", "application/json, text/event-stream")
        .header("MCP-Protocol-Version", "2024-11-05")
        .json(&json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success(), "HTTP {}", response.status());
    response.json().await.unwrap()
}

#[tokio::test]
async fn http_real_handshake_discovery_and_per_request_client_isolation() {
    let directory = tempfile::tempdir().unwrap();
    let first_token = "a".repeat(40);
    let second_token = "b".repeat(40);
    let settings = Settings::from_map(&std::collections::HashMap::from([
        ("XERO_MCP_TENANTS".into(), format!("{FIRST},{SECOND}")),
        (
            "XERO_MCP_CLIENTS_JSON".into(),
            json!([
                {"token":first_token,"client_id":"first","tenant_ids":[FIRST]},
                {"token":second_token,"client_id":"second","tenant_ids":[SECOND],"allow_warehouse":true}
            ])
            .to_string(),
        ),
    ]))
    .unwrap();
    let backend = Arc::new(Fixture::new());
    let server = XeroMcp::new(
        backend.clone(),
        settings.local_scope.clone(),
        directory.path().into(),
    );
    let cancellation = CancellationToken::new();
    let router = transport::router(server, &settings, cancellation.clone()).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/mcp", listener.local_addr().unwrap());
    let shutdown = cancellation.clone();
    let task = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(shutdown.cancelled_owned())
            .await
            .unwrap();
    });
    let client = reqwest::Client::new();
    assert_eq!(
        client
            .post(&url)
            .json(&json!({}))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    let initialized=rpc(&client,&url,&first_token,1,"initialize",json!({"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"model-plane","version":"test"}})).await;
    assert_eq!(initialized["result"]["protocolVersion"], "2024-11-05");
    let mut cursor: Option<String> = None;
    let mut tools = Vec::new();
    loop {
        let response = rpc(
            &client,
            &url,
            &first_token,
            2,
            "tools/list",
            cursor
                .as_ref()
                .map(|s| json!({"cursor":s}))
                .unwrap_or(json!({})),
        )
        .await;
        tools.extend(response["result"]["tools"].as_array().unwrap().clone());
        cursor = response["result"]["nextCursor"].as_str().map(String::from);
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(tools.len(), 198);
    let connections = rpc(
        &client,
        &url,
        &first_token,
        3,
        "tools/call",
        json!({"name":"xero_list_connections","arguments":{}}),
    )
    .await;
    assert_eq!(
        connections["result"]["structuredContent"]["data"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        connections["result"]["structuredContent"]["data"][0]["tenant_id"],
        FIRST
    );
    let warehouse_denied = rpc(
        &client,
        &url,
        &first_token,
        30,
        "tools/call",
        json!({"name":"xero_warehouse_overview","arguments":{}}),
    )
    .await;
    assert_eq!(warehouse_denied["result"]["isError"], true);
    let warehouse = rpc(
        &client,
        &url,
        &second_token,
        31,
        "tools/call",
        json!({"name":"xero_warehouse_overview","arguments":{}}),
    )
    .await;
    assert_eq!(warehouse["result"]["isError"], false);
    assert_eq!(
        warehouse["result"]["structuredContent"]["data"]["total_rows"],
        1
    );
    assert_eq!(
        warehouse["result"]["structuredContent"]["meta"]["record_data_included"],
        false
    );
    let args =
        json!({"tenant_id":SECOND,"family":"accounting","operation_id":"getInvoices","limit":1});
    let denied = rpc(
        &client,
        &url,
        &first_token,
        4,
        "tools/call",
        json!({"name":"xero_query","arguments":args}),
    )
    .await;
    assert_eq!(denied["result"]["isError"], true);
    assert_eq!(backend.fetches.load(Ordering::SeqCst), 0);
    let allowed = rpc(
        &client,
        &url,
        &second_token,
        5,
        "tools/call",
        json!({"name":"xero_query","arguments":args}),
    )
    .await;
    assert_eq!(allowed["result"]["isError"], false);
    assert_eq!(
        allowed["result"]["structuredContent"]["data"]["Invoices"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let uri = allowed["result"]["structuredContent"]["meta"]["resource_uri"]
        .as_str()
        .unwrap();
    let denied_resource = rpc(
        &client,
        &url,
        &first_token,
        6,
        "resources/read",
        json!({"uri":uri}),
    )
    .await;
    assert!(denied_resource.get("error").is_some());
    let own_resource = rpc(
        &client,
        &url,
        &second_token,
        7,
        "resources/read",
        json!({"uri":uri}),
    )
    .await;
    assert!(own_resource["result"]["contents"].is_array());
    let (first_parallel, second_parallel) = tokio::join!(
        rpc(
            &client,
            &url,
            &first_token,
            20,
            "tools/call",
            json!({"name":"xero_query","arguments":{"tenant_id":FIRST,"family":"accounting","operation_id":"getInvoices"}})
        ),
        rpc(
            &client,
            &url,
            &second_token,
            21,
            "tools/call",
            json!({"name":"xero_query","arguments":{"tenant_id":SECOND,"family":"accounting","operation_id":"getInvoices"}})
        )
    );
    assert_eq!(
        first_parallel["result"]["structuredContent"]["meta"]["tenant_id"],
        FIRST
    );
    assert_eq!(
        second_parallel["result"]["structuredContent"]["meta"]["tenant_id"],
        SECOND
    );
    let other_handle = &second_parallel["result"]["structuredContent"]["meta"]["result_id"];
    let denied_slice = rpc(&client,&url,&first_token,22,"tools/call",json!({"name":"xero_read_result","arguments":{"result_id":other_handle,"pointer":"/Invoices"}})).await;
    assert_eq!(denied_slice["result"]["isError"], true);
    let prompt = rpc(
        &client,
        &url,
        &first_token,
        9,
        "prompts/get",
        json!({"name":"financial_overview","arguments":{"tenant_id":FIRST}}),
    )
    .await;
    assert!(prompt["result"]["messages"].is_array());
    let denied_prompt = rpc(
        &client,
        &url,
        &first_token,
        14,
        "prompts/get",
        json!({"name":"financial_overview","arguments":{"tenant_id":SECOND}}),
    )
    .await;
    assert!(denied_prompt.get("error").is_some());
    let invalid_prompt = rpc(
        &client,
        &url,
        &first_token,
        11,
        "prompts/get",
        json!({"name":"unknown","arguments":{"tenant_id":FIRST}}),
    )
    .await;
    assert!(invalid_prompt.get("error").is_some());
    let resources = rpc(&client, &url, &first_token, 12, "resources/list", json!({})).await;
    assert_eq!(
        resources["result"]["resources"].as_array().unwrap().len(),
        3
    );
    let templates = rpc(
        &client,
        &url,
        &first_token,
        13,
        "resources/templates/list",
        json!({}),
    )
    .await;
    assert_eq!(
        templates["result"]["resourceTemplates"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    for uri in ["xero://capabilities", "xero://connections"] {
        assert!(rpc(
            &client,
            &url,
            &first_token,
            14,
            "resources/read",
            json!({"uri":uri})
        )
        .await["result"]["contents"]
            .is_array());
    }
    for uri in ["xero://unknown", "xero://results/bad", "xero://jobs/bad"] {
        assert!(rpc(
            &client,
            &url,
            &first_token,
            15,
            "resources/read",
            json!({"uri":uri})
        )
        .await
        .get("error")
        .is_some());
    }
    for cursor in ["bad", "199"] {
        assert!(rpc(
            &client,
            &url,
            &first_token,
            16,
            "tools/list",
            json!({"cursor":cursor})
        )
        .await
        .get("error")
        .is_some());
    }
    assert_eq!(
        client
            .post(&url)
            .bearer_auth(&first_token)
            .header("Origin", "https://untrusted.invalid")
            .header("Accept", "application/json, text/event-stream")
            .json(&json!({"jsonrpc":"2.0","id":8,"method":"tools/list","params":{}}))
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    assert_eq!(
        client
            .post(&url)
            .bearer_auth(&first_token)
            .header("Host", "untrusted.invalid")
            .header("Accept", "application/json, text/event-stream")
            .json(&json!({"jsonrpc":"2.0","id":17,"method":"tools/list","params":{}}))
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    let capabilities_url = url.replace("/mcp", "/capabilities");
    let mut limited = false;
    for _ in 0..121 {
        let response = client
            .get(&capabilities_url)
            .bearer_auth(&first_token)
            .send()
            .await
            .unwrap();
        if response.status() == 429 {
            assert_eq!(response.headers()["retry-after"], "60");
            limited = true;
            break;
        }
        assert!(response.status().is_success());
    }
    assert!(limited);
    assert!(client
        .get(&capabilities_url)
        .bearer_auth(&second_token)
        .send()
        .await
        .unwrap()
        .status()
        .is_success());
    cancellation.cancel();
    task.await.unwrap();
}

#[tokio::test]
async fn stdio_sdk_roundtrip_lists_prompts_reads_resources_and_invokes_tools() {
    let directory = tempfile::tempdir().unwrap();
    let scope = AccessScope::new("local", vec![FIRST.into()], false, false, false).unwrap();
    let server = XeroMcp::new(Arc::new(Fixture::new()), scope, directory.path().into());
    let (server_io, client_io) = tokio::io::duplex(128 * 1024);
    let task = tokio::spawn(async move {
        let running = server.serve(server_io).await.unwrap();
        running.waiting().await.unwrap();
    });
    let client = ClientConfig::default().serve(client_io).await.unwrap();
    assert_eq!(client.list_all_tools().await.unwrap().len(), 198);
    assert_eq!(client.list_all_prompts().await.unwrap().len(), 9);
    let resource = client
        .read_resource(ReadResourceRequestParams::new("xero://usage"))
        .await
        .unwrap();
    assert!(!serde_json::to_value(resource).unwrap()["contents"]
        .as_array()
        .unwrap()
        .is_empty());
    let mut call = CallToolRequestParams::new("xero_query");
    call.arguments = Some(
        json!({"tenant_id":FIRST,"family":"accounting","operation_id":"getInvoices","limit":1})
            .as_object()
            .unwrap()
            .clone(),
    );
    let result = serde_json::to_value(client.call_tool(call).await.unwrap()).unwrap();
    assert_eq!(result["isError"], false);
    assert_eq!(
        result["structuredContent"]["meta"]["complete_collection"],
        false
    );
    client.cancel().await.unwrap();
    task.await.unwrap();
}
