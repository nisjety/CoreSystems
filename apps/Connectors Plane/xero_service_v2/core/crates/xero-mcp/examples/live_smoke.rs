//! Read-only MCP probe. Prints counts and protocol outcomes, never Xero records.
use clap::Parser;
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
use uuid::Uuid;

#[derive(Parser)]
struct Args {
    #[arg(long, default_value = "http://127.0.0.1:5003/mcp")]
    url: String,
    #[arg(long)]
    tenant: Uuid,
    #[arg(long)]
    token_file: PathBuf,
    /// Add bounded Accounting master-data, Assets and Projects probes.
    #[arg(long)]
    extended: bool,
}
struct Client {
    http: reqwest::Client,
    url: String,
    token: String,
}
impl Client {
    async fn rpc(&self, id: u32, method: &str, params: Value) -> anyhow::Result<Value> {
        let response = self
            .http
            .post(&self.url)
            .bearer_auth(&self.token)
            .header("Accept", "application/json, text/event-stream")
            .header("MCP-Protocol-Version", "2024-11-05")
            .json(&json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
            .send()
            .await?;
        anyhow::ensure!(
            response.status().is_success(),
            "MCP HTTP request failed ({})",
            response.status()
        );
        let value: Value = response.json().await?;
        anyhow::ensure!(value.get("error").is_none(), "MCP protocol request failed");
        Ok(value["result"].clone())
    }
    async fn tool(&self, id: u32, name: &str, arguments: Value) -> anyhow::Result<Value> {
        let result = self
            .rpc(id, "tools/call", json!({"name":name,"arguments":arguments}))
            .await?;
        anyhow::ensure!(
            result["isError"] != true,
            "MCP tool failed: {}",
            result["structuredContent"]["error"]["code"]
                .as_str()
                .unwrap_or("unknown")
        );
        Ok(result["structuredContent"].clone())
    }
}
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let parsed = reqwest::Url::parse(&args.url)?;
    anyhow::ensure!(
        parsed.scheme() == "https"
            || (parsed.scheme() == "http"
                && matches!(parsed.host_str(), Some("127.0.0.1" | "localhost" | "::1"))),
        "Use HTTPS or a loopback URL for credentials"
    );
    let token = std::fs::read_to_string(args.token_file)
        .map_err(|_| anyhow::anyhow!("Cannot read private MCP credential file"))?;
    let client = Client {
        http: reqwest::Client::builder()
            .timeout(Duration::from_secs(70))
            .build()?,
        url: args.url,
        token: token.trim().into(),
    };
    let init = client.rpc(1,"initialize",json!({"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"xero-read-only-smoke","version":"1"}})).await?;
    let response = client
        .http
        .post(&client.url)
        .bearer_auth(&client.token)
        .header("Accept", "application/json, text/event-stream")
        .header("MCP-Protocol-Version", "2024-11-05")
        .json(&json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
        .send()
        .await?;
    anyhow::ensure!(
        response.status().is_success(),
        "MCP initialization notification failed"
    );
    let mut tools = 0;
    let mut cursor: Option<String> = None;
    loop {
        let page = client
            .rpc(
                2,
                "tools/list",
                cursor
                    .as_ref()
                    .map(|c| json!({"cursor":c}))
                    .unwrap_or(json!({})),
            )
            .await?;
        tools += page["tools"].as_array().map(Vec::len).unwrap_or(0);
        cursor = page["nextCursor"].as_str().map(String::from);
        if cursor.is_none() {
            break;
        }
    }
    let connections = client.tool(3, "xero_list_connections", json!({})).await?;
    let mut checks = Vec::new();
    let mut invoice_id: Option<Value> = None;
    for (id, operation, collection, query) in [
        (4, "getInvoices", "Invoices", json!({"page":1,"pageSize":2})),
        (5, "getAccounts", "Accounts", json!({})),
        (6, "getOrganisations", "Organisations", json!({})),
    ] {
        let started = Instant::now();
        let result=client.tool(id,"xero_query",json!({"tenant_id":args.tenant,"family":"accounting","operation_id":operation,"query":query,"limit":1})).await?;
        let records = result["data"][collection]
            .as_array()
            .map(Vec::len)
            .unwrap_or(0);
        anyhow::ensure!(
            result["meta"]["complete_collection"] == false,
            "A single response claimed full coverage"
        );
        checks.push(json!({"operation":operation,"http_status":result["meta"]["http_status"],"preview_records":records,"elapsed_ms":started.elapsed().as_millis(),"truncated":result["meta"]["truncated"],"complete_collection":result["meta"]["complete_collection"]}));
        if operation == "getInvoices" && result["meta"]["result_id"].is_string() {
            invoice_id = result["data"]["Invoices"][0].get("InvoiceID").cloned();
            let started = Instant::now();
            let slice=client.tool(7,"xero_read_result",json!({"result_id":result["meta"]["result_id"],"pointer":"/Invoices","offset":1,"limit":1,"fields":["InvoiceID"]})).await?;
            checks.push(json!({"check":"cached_invoice_page","records":slice["data"].as_array().map(Vec::len).unwrap_or(0),"elapsed_ms":started.elapsed().as_millis()}));
        }
    }
    let denied = client.rpc(8,"tools/call",json!({"name":"xero_query","arguments":{"tenant_id":Uuid::nil(),"family":"accounting","operation_id":"getInvoices"}})).await?;
    anyhow::ensure!(denied["isError"] == true, "Unlisted tenant was not denied");
    let prompt_list = client.rpc(9, "prompts/list", json!({})).await?;
    let prompts = prompt_list["prompts"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("Prompt catalog missing"))?;
    anyhow::ensure!(prompts.len() == 6, "Expected six workflows");
    for prompt in prompts {
        let result = client
            .rpc(
                10,
                "prompts/get",
                json!({"name":prompt["name"],"arguments":{"tenant_id":args.tenant.to_string()}}),
            )
            .await?;
        anyhow::ensure!(result["messages"].is_array(), "Prompt response missing");
    }
    for (family, total) in [
        ("accounting", 126),
        ("assets", 4),
        ("projects", 7),
        ("bankfeeds", 4),
        ("finance", 8),
        ("identity", 1),
    ] {
        let result = client
            .tool(
                11,
                "xero_list_operations",
                json!({"family":family,"limit":1}),
            )
            .await?;
        anyhow::ensure!(
            result["meta"]["total"] == total,
            "Endpoint discovery count mismatch"
        );
    }
    for query in [
        json!({"page":1,"pageSize":101}),
        json!({"unknown_filter":"not-allowed"}),
    ] {
        let invalid = client.rpc(12,"tools/call",json!({"name":"xero_query","arguments":{"tenant_id":args.tenant,"family":"accounting","operation_id":"getInvoices","query":query}})).await?;
        anyhow::ensure!(invalid["isError"] == true, "Invalid request was accepted");
    }
    let mut policies = json!({});
    if connections["meta"]["allow_extract"] == false {
        let denied = client.rpc(13,"tools/call",json!({"name":"xero_extract","arguments":{"tenant_id":args.tenant,"destination":"local","idempotency_key":"read-only-probe-must-not-execute","plan":{"kind":"accounting","entities":["invoices"]}}})).await?;
        anyhow::ensure!(
            denied["structuredContent"]["error"]["code"] == "extraction_forbidden",
            "Extraction policy did not deny"
        );
        policies["extraction_denied"] = json!(true);
    }
    if connections["meta"]["allow_sensitive"] == false {
        let denied = client.rpc(14,"tools/call",json!({"name":"xero_query","arguments":{"tenant_id":args.tenant,"family":"identity","operation_id":"getConnections"}})).await?;
        anyhow::ensure!(
            denied["structuredContent"]["error"]["code"] == "sensitive_access_required",
            "Sensitive policy did not deny"
        );
        policies["sensitive_denied"] = json!(true);
    }
    if args.extended {
        let probes = [
            (
                "accounting",
                "getInvoices",
                "Invoices",
                json!({"page":2,"pageSize":2}),
            ),
            (
                "accounting",
                "getContacts",
                "Contacts",
                json!({"page":1,"pageSize":2}),
            ),
            ("accounting", "getCurrencies", "Currencies", json!({})),
            ("accounting", "getItems", "Items", json!({})),
            ("accounting", "getTaxRates", "TaxRates", json!({})),
            (
                "accounting",
                "getTrackingCategories",
                "TrackingCategories",
                json!({}),
            ),
            (
                "assets",
                "getAssets",
                "items",
                json!({"status":"DRAFT","page":1,"pageSize":2}),
            ),
            (
                "projects",
                "getProjects",
                "items",
                json!({"page":1,"pageSize":2}),
            ),
        ];
        for (family, operation, collection, query) in probes {
            let started = Instant::now();
            let result = client.rpc(15,"tools/call",json!({"name":"xero_query","arguments":{"tenant_id":args.tenant,"family":family,"operation_id":operation,"query":query,"limit":1}})).await?;
            if result["isError"] == true {
                checks.push(json!({"family":family,"operation":operation,"available":false,"error_code":result["structuredContent"]["error"]["code"],"elapsed_ms":started.elapsed().as_millis()}));
                continue;
            }
            let value = &result["structuredContent"];
            anyhow::ensure!(
                value["meta"]["complete_collection"] == false,
                "A page claimed complete coverage"
            );
            if operation == "getInvoices" {
                if let (Some(first), Some(second)) = (
                    invoice_id.as_ref(),
                    value["data"]["Invoices"][0].get("InvoiceID"),
                ) {
                    anyhow::ensure!(
                        first != second,
                        "Provider page 2 repeated the first invoice"
                    );
                }
            }
            checks.push(json!({"family":family,"operation":operation,"http_status":value["meta"]["http_status"],"preview_records":value["data"][collection].as_array().map(Vec::len).unwrap_or(0),"truncated":value["meta"]["truncated"],"elapsed_ms":started.elapsed().as_millis()}));
        }
    }
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"protocol":init["protocolVersion"],"tools":tools,"visible_connections":connections["data"].as_array().map(Vec::len).unwrap_or(0),"cross_tenant_denied":true,"prompts_validated":prompts.len(),"catalog_operations":186,"invalid_parameters_denied":true,"source_writes":false,"policy_checks":policies,"checks":checks})
        )?
    );
    Ok(())
}
