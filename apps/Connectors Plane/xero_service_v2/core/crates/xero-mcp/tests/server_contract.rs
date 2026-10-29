use async_trait::async_trait;
use chrono::Utc;
use serde_json::{json, Value};
use std::{
    path::Path,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};
use uuid::Uuid;
use xero_mcp::{
    backend::{
        Backend, ConnectionInfo, Failure, Query, Response, WarehouseMetricRequest,
        WarehouseMetricResult, WarehouseOverview, WarehouseTableCount,
    },
    catalog,
    config::AccessScope,
    jobs::{Destination, ExtractionPlan},
    server::XeroMcp,
};

const TENANT: &str = "5f3c9a2e-0000-0000-0000-000000000000";
struct Fixture {
    calls: AtomicUsize,
}
#[async_trait]
impl Backend for Fixture {
    async fn connections(&self) -> Result<Vec<ConnectionInfo>, Failure> {
        Ok(vec![ConnectionInfo {
            tenant_id: TENANT.into(),
            tenant_name: "Fixture".into(),
            scopes: vec![],
            auth_mode: "fixture".into(),
        }])
    }
    async fn fetch(&self, request: &Query) -> Result<Response, Failure> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let second_page = request.query.get("page") == Some(&json!(2));
        let body = if request.family == "accounting" && request.operation_id == "getOrganisations" {
            json!({"Organisations":[{"ShortCode":"ABCD1234"}]})
        } else if request.family == "bankfeeds" {
            json!({"items":[{"id":"feed-one"},{"id":"feed-two"}],"pagination":{"page":1,"pageCount":2}})
        } else if second_page {
            json!({"Invoices":[{"InvoiceID":"page-two-one"},{"InvoiceID":"page-two-two"}]})
        } else {
            json!({"Invoices":[{"InvoiceID":"one"},{"InvoiceID":"two"}]})
        };
        Ok(Response {
            body: serde_json::to_vec(&body).unwrap().into(),
            content_type: "application/json".into(),
            http_status: if request.family == "bankfeeds" {
                202
            } else {
                200
            },
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
        Ok(json!({"complete":true}))
    }
    async fn warehouse_overview(
        &self,
        tenant_ids: &[String],
    ) -> Result<WarehouseOverview, Failure> {
        assert_eq!(tenant_ids, &[TENANT.to_owned()]);
        Ok(WarehouseOverview {
            project_id: "warehouse-project".into(),
            dataset_id: "dw_staging_xero".into(),
            location: "europe-north2".into(),
            tables: vec![WarehouseTableCount {
                table_name: "invoices".into(),
                row_count: 3,
            }],
        })
    }
    async fn warehouse_metric(
        &self,
        tenant_ids: &[String],
        request: &WarehouseMetricRequest,
    ) -> Result<WarehouseMetricResult, Failure> {
        assert_eq!(tenant_ids, &[TENANT.to_owned()]);
        assert_eq!(request.metric.as_str(), "product_volume_by_month");
        Ok(WarehouseMetricResult {
            project_id: "warehouse-project".into(),
            location: "europe-north2".into(),
            metric: request.metric.as_str().into(),
            rows: vec![json!({
                "item_code": "ITEM-1",
                "net_quantity": "12",
                "net_amount_in_currency": "1200"
            })],
        })
    }
}

#[tokio::test]
async fn provider_pages_and_cached_slices_keep_separate_coverage() {
    let directory = tempfile::tempdir().unwrap();
    let (server, backend) = server(directory.path());
    let first = server.execute(&scope(false),"xero_query",json!({"tenant_id":TENANT,"family":"accounting","operation_id":"getInvoices","query":{"page":1,"pageSize":2},"limit":1})).await.unwrap();
    let second = server.execute(&scope(false),"xero_query",json!({"tenant_id":TENANT,"family":"accounting","operation_id":"getInvoices","query":{"page":2,"pageSize":2},"limit":1})).await.unwrap();
    assert_eq!(first["data"]["Invoices"][0]["InvoiceID"], "one");
    assert_eq!(second["data"]["Invoices"][0]["InvoiceID"], "page-two-one");
    assert_eq!(second["meta"]["page"], 2);
    let slice = server.execute(&scope(false),"xero_read_result",json!({"result_id":first["meta"]["result_id"],"pointer":"/Invoices","offset":1,"limit":1})).await.unwrap();
    assert_eq!(slice["data"][0]["InvoiceID"], "two");
    assert_eq!(backend.calls.load(Ordering::SeqCst), 2);
    for result in [&first, &second, &slice] {
        assert_eq!(result["meta"]["complete_collection"], false);
    }
    let sensitive = AccessScope {
        allow_sensitive: true,
        ..scope(false)
    };
    let feed = server.execute(&sensitive,"xero_query",json!({"tenant_id":TENANT,"family":"bankfeeds","operation_id":"getFeedConnections","query":{"page":1},"limit":1})).await.unwrap();
    assert_eq!(feed["meta"]["http_status"], 202);
    assert_eq!(feed["data"]["pagination"]["pageCount"], 2);
    assert_eq!(feed["data"]["items"].as_array().unwrap().len(), 1);
    assert_eq!(feed["meta"]["complete_collection"], false);
}
fn scope(extract: bool) -> AccessScope {
    AccessScope::new("fixture", vec![TENANT.into()], extract, false, false).unwrap()
}
fn server(root: &Path) -> (XeroMcp, Arc<Fixture>) {
    let backend = Arc::new(Fixture {
        calls: AtomicUsize::new(0),
    });
    (
        XeroMcp::new(backend.clone(), scope(false), root.into()),
        backend,
    )
}

#[tokio::test]
async fn warehouse_overview_requires_explicit_grant_and_returns_counts_only() {
    let directory = tempfile::tempdir().unwrap();
    let (server, _) = server(directory.path());

    let denied = server
        .execute(&scope(false), "xero_warehouse_overview", json!({}))
        .await
        .unwrap_err();
    assert_eq!(denied.code, "warehouse_access_required");

    let allowed = AccessScope {
        allow_warehouse: true,
        ..scope(false)
    };
    let overview = server
        .execute(&allowed, "xero_warehouse_overview", json!({}))
        .await
        .unwrap();

    assert_eq!(overview["data"]["total_rows"], 3);
    assert_eq!(overview["data"]["table_count"], 1);
    assert_eq!(overview["data"]["tables"][0]["name"], "invoices");
    assert!(overview["data"].get("tenant_ids").is_none());
    assert_eq!(overview["meta"]["coverage"], "tenant_scoped_row_counts");
    assert!(server
        .tools()
        .iter()
        .any(|tool| tool.name == "xero_warehouse_overview"));
}

#[tokio::test]
async fn warehouse_metric_uses_only_approved_aggregate_and_tenant_scope() {
    let directory = tempfile::tempdir().unwrap();
    let (server, _) = server(directory.path());
    let args = json!({
        "metric": "product_volume_by_month",
        "from": "2026-01-01",
        "to": "2026-01-31",
    });

    let denied = server
        .execute(&scope(false), "xero_warehouse_metric", args.clone())
        .await
        .unwrap_err();
    assert_eq!(denied.code, "warehouse_access_required");

    let allowed = AccessScope {
        allow_warehouse: true,
        ..scope(false)
    };
    let result = server
        .execute(&allowed, "xero_warehouse_metric", args)
        .await
        .unwrap();
    assert_eq!(result["data"]["metric"], "product_volume_by_month");
    assert_eq!(result["data"]["row_count"], 1);
    assert_eq!(result["data"]["rows"][0]["item_code"], "ITEM-1");
    assert_eq!(result["meta"]["coverage"], "tenant_scoped_gold_aggregate");
    assert_eq!(result["meta"]["record_data_included"], false);

    for bad_args in [
        json!({
            "metric": "product_volume_by_month",
            "from": "2026-01-01",
            "to": "2027-01-03",
        }),
        json!({
            "metric": "product_volume_by_month",
            "from": "2026-01-01",
            "to": "2026-01-31",
            "sql": "SELECT * FROM anything",
        }),
        json!({
            "metric": "product_volume_by_month",
            "from": "2026-01-01",
            "to": "2026-01-31",
            "tenant_id": "5f3c9a2e-0000-0000-0000-000000000001",
        }),
    ] {
        assert!(server
            .execute(&allowed, "xero_warehouse_metric", bad_args)
            .await
            .is_err());
    }
}

#[tokio::test]
async fn compact_discovery_filters_pages_and_preserves_schemas() {
    let directory = tempfile::tempdir().unwrap();
    let (mut server, _) = server(directory.path());
    server.compact = true;
    assert_eq!(server.tools().len(), 14);
    let manifest = server.manifest();
    let capability_tools = manifest["operational_tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|tool| tool.as_str())
        .collect::<Vec<_>>();
    for tool in [
        "xero_business_read",
        "xero_financial_report",
        "xero_payroll_nz_read",
        "xero_build_ui_link",
    ] {
        assert!(capability_tools.contains(&tool));
    }
    let first = server
        .execute(
            &scope(false),
            "xero_list_operations",
            json!({"family":"accounting","limit":2,"include_schema":true}),
        )
        .await
        .unwrap();
    assert_eq!(first["meta"]["total"], 124);
    assert_eq!(first["meta"]["next_offset"], 2);
    assert!(first["data"][0]["input_schema"]["properties"]["tenant_id"].is_object());
    let last = server
        .execute(
            &scope(false),
            "xero_list_operations",
            json!({"family":"identity","search":"Connections","offset":1}),
        )
        .await
        .unwrap();
    assert_eq!(last["meta"]["total"], 1);
    assert_eq!(last["meta"]["next_offset"], Value::Null);
    let sensitive = server
        .execute(
            &scope(false),
            "xero_list_operations",
            json!({"family":"identity"}),
        )
        .await
        .unwrap();
    assert_eq!(sensitive["data"][0]["permission_available"], false);
    for args in [
        json!({"family":"other"}),
        json!({"limit":21}),
        json!({"offset":187}),
        json!({"search":"\n"}),
        json!({"unknown":1}),
    ] {
        assert!(server
            .execute(&scope(false), "xero_list_operations", args)
            .await
            .is_err());
    }
    assert_eq!(catalog::manifest()["catalog_operation_count"], 186);
    assert_eq!(catalog::manifest()["operation_count"], 184);
    let payment_services = server
        .execute(
            &scope(false),
            "xero_list_operations",
            json!({"family":"accounting","search":"PaymentServices"}),
        )
        .await
        .unwrap();
    assert_eq!(payment_services["meta"]["total"], 0);
}

#[tokio::test]
async fn endpoint_tools_do_not_accept_authority_overrides_and_cache_slices_are_scoped() {
    let directory = tempfile::tempdir().unwrap();
    let (server, backend) = server(directory.path());
    for (tool, args) in [
        ("unknown-secret-input", json!({})),
        ("xero_list_connections", json!({"tenant_id":TENANT})),
        (
            "xero_accounting_getInvoices",
            json!({"tenant_id":TENANT,"allow_sensitive":true}),
        ),
        (
            "xero_query",
            json!({"tenant_id":"bad","family":"accounting","operation_id":"getInvoices"}),
        ),
        (
            "xero_query",
            json!({"tenant_id":TENANT,"family":"identity","operation_id":"getConnections"}),
        ),
        (
            "xero_query",
            json!({"tenant_id":TENANT,"family":"accounting","operation_id":"getPaymentServices"}),
        ),
        (
            "xero_query",
            json!({"tenant_id":TENANT,"family":"accounting","operation_id":"getInvoices","limit":0}),
        ),
    ] {
        assert!(server.execute(&scope(false), tool, args).await.is_err());
    }
    assert_eq!(backend.calls.load(Ordering::SeqCst), 0);
    let fetched = server
        .execute(
            &scope(false),
            "xero_accounting_getInvoices",
            json!({"tenant_id":TENANT,"limit":1}),
        )
        .await
        .unwrap();
    assert_eq!(fetched["meta"]["truncated"], true);
    assert!(fetched["meta"]["request_id"].is_string());
    let read = server.execute(&scope(false), "xero_read_result", json!({"result_id":fetched["meta"]["result_id"],"pointer":"/Invoices","offset":1,"limit":1,"fields":["InvoiceID"]})).await.unwrap();
    assert_eq!(read["data"][0]["InvoiceID"], "two");
}

#[tokio::test]
async fn curated_business_report_and_payroll_tools_preserve_catalog_and_tenant_controls() {
    let directory = tempfile::tempdir().unwrap();
    let (server, backend) = server(directory.path());

    for tool in [
        "xero_business_read",
        "xero_financial_report",
        "xero_payroll_nz_read",
        "xero_build_ui_link",
    ] {
        assert!(server
            .tools()
            .iter()
            .any(|candidate| candidate.name == tool));
    }

    let invoices = server
        .execute(
            &scope(false),
            "xero_business_read",
            json!({
                "tenant_id": TENANT,
                "resource": "invoices",
                "query": {"page": 1, "pageSize": 2},
                "limit": 1,
            }),
        )
        .await
        .unwrap();
    assert_eq!(invoices["data"]["Invoices"][0]["InvoiceID"], "one");
    assert_eq!(invoices["meta"]["operation_id"], "getInvoices");
    assert_eq!(invoices["meta"]["workflow"], "business_read");
    assert_eq!(invoices["meta"]["curated_resource"], "invoices");

    let report = server
        .execute(
            &scope(false),
            "xero_financial_report",
            json!({
                "tenant_id": TENANT,
                "report": "profit_and_loss",
                "query": {"fromDate": "2026-01-01", "toDate": "2026-01-31"},
            }),
        )
        .await
        .unwrap();
    assert_eq!(report["meta"]["operation_id"], "getReportProfitAndLoss");
    assert_eq!(report["meta"]["workflow"], "financial_report");

    let denied_payroll = server
        .execute(
            &scope(false),
            "xero_payroll_nz_read",
            json!({"tenant_id": TENANT, "resource": "employees"}),
        )
        .await
        .unwrap_err();
    assert_eq!(denied_payroll.code, "sensitive_access_required");

    let sensitive = AccessScope {
        allow_sensitive: true,
        ..scope(false)
    };
    let payroll = server
        .execute(
            &sensitive,
            "xero_payroll_nz_read",
            json!({"tenant_id": TENANT, "resource": "employees"}),
        )
        .await
        .unwrap();
    assert_eq!(payroll["meta"]["operation_id"], "getEmployees");
    assert_eq!(payroll["meta"]["workflow"], "payroll_nz_read");

    for args in [
        json!({"tenant_id": TENANT, "resource": "not_a_real_resource"}),
        json!({"tenant_id": "5f3c9a2e-0000-0000-0000-000000000001", "resource": "invoices"}),
        json!({"tenant_id": TENANT, "resource": "invoices", "header": {"Authorization": "secret"}}),
    ] {
        assert!(server
            .execute(&scope(false), "xero_business_read", args)
            .await
            .is_err());
    }
    assert_eq!(backend.calls.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn xero_ui_links_are_tenant_scoped_and_never_accept_unsafe_link_components() {
    let directory = tempfile::tempdir().unwrap();
    let (server, backend) = server(directory.path());
    let invoice_id = "8d02390c-8d05-4a5e-9e78-099d49c07b0d";

    let link = server
        .execute(
            &scope(false),
            "xero_build_ui_link",
            json!({
                "tenant_id": TENANT,
                "resource": "invoice",
                "resource_id": invoice_id,
            }),
        )
        .await
        .unwrap();
    assert_eq!(link["data"]["tenant_id"], TENANT);
    assert_eq!(link["data"]["resource"], "invoice");
    assert_eq!(
        link["data"]["url"],
        format!("https://go.xero.com/app/ABCD1234/invoicing/view/{invoice_id}")
    );
    assert_eq!(link["meta"]["coverage"], "xero_ui_navigation");
    assert_eq!(backend.calls.load(Ordering::SeqCst), 1);

    for args in [
        json!({"tenant_id": TENANT, "resource": "invoice", "resource_id": "not-a-uuid"}),
        json!({"tenant_id": TENANT, "resource": "invoice", "resource_id": invoice_id, "organization_short_code": "ABCD1234"}),
        json!({"tenant_id": "5f3c9a2e-0000-0000-0000-000000000001", "resource": "manual_journal", "resource_id": invoice_id}),
        json!({"tenant_id": TENANT, "resource": "bank_transaction", "resource_id": invoice_id}),
    ] {
        assert!(server
            .execute(&scope(false), "xero_build_ui_link", args)
            .await
            .is_err());
    }
}

#[tokio::test]
async fn extraction_denies_before_connection_lookup_then_tracks_owned_jobs() {
    let directory = tempfile::tempdir().unwrap();
    let (server, _) = server(directory.path());
    let args = json!({"tenant_id":TENANT,"destination":"local","idempotency_key":"fixture","plan":{"kind":"operation","family":"accounting","operation_id":"getInvoices"}});
    assert_eq!(
        server
            .execute(&scope(false), "xero_extract", args.clone())
            .await
            .unwrap_err()
            .code,
        "extraction_forbidden"
    );
    assert!(!directory.path().join("jobs").exists());
    for operation_id in ["getPaymentServices", "getBrandingThemePaymentServices"] {
        let rejected = json!({
            "tenant_id": TENANT,
            "destination": "local",
            "idempotency_key": format!("excluded-{operation_id}"),
            "plan": {"kind": "operation", "family": "accounting", "operation_id": operation_id},
        });
        assert!(server
            .execute(&scope(true), "xero_extract", rejected)
            .await
            .is_err());
    }
    assert!(!directory.path().join("jobs").exists());
    let started = server
        .execute(&scope(true), "xero_extract", args)
        .await
        .unwrap();
    let id = started["data"]["job_id"].clone();
    let polled = server
        .execute(&scope(true), "xero_get_job", json!({"job_id":id}))
        .await
        .unwrap();
    assert_eq!(polled["data"]["tenant_id"], TENANT);
    server
        .execute(&scope(true), "xero_cancel_job", json!({"job_id":id}))
        .await
        .unwrap();
    assert!(server
        .execute(&scope(true), "xero_get_job", json!({"job_id":"bad"}))
        .await
        .is_err());
    assert!(server
        .execute(
            &scope(true),
            "xero_get_job",
            json!({"job_id":Uuid::new_v4()})
        )
        .await
        .is_err());
}
