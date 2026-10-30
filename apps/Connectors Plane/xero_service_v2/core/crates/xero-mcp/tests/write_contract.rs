use async_trait::async_trait;
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
    backend::{Backend, ConnectionInfo, Failure, Query, Response},
    config::AccessScope,
    jobs::{Destination, ExtractionPlan},
    server::XeroMcp,
    writes::{official_write_actions, WriteCommand, WriteRequest, WriteStore},
};

const TENANT: &str = "5f3c9a2e-0000-0000-0000-000000000000";

struct Fixture(AtomicUsize);

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
    async fn fetch(&self, _: &Query) -> Result<Response, Failure> {
        unreachable!()
    }
    async fn extract(
        &self,
        _: Uuid,
        _: &str,
        _: &ExtractionPlan,
        _: Destination,
        _: &Path,
    ) -> Result<Value, Failure> {
        unreachable!()
    }
    async fn write(&self, request: &WriteCommand) -> Result<Value, Failure> {
        tokio::time::sleep(std::time::Duration::from_millis(75)).await;
        self.0.fetch_add(1, Ordering::SeqCst);
        if request.action == "create_payment" {
            return Err(Failure::new(
                "xero_write_rejected",
                "Fixture rejection",
                false,
            ));
        }
        Ok(json!({"action":request.action,"ContactID":"created"}))
    }
}

fn contact() -> Value {
    json!({"tenant_id":TENANT,"action":"create_contact","input":{"name":"Example Customer","email":"customer@example.com"}})
}

#[tokio::test]
async fn writes_are_opt_in_per_action_and_require_out_of_band_approval() {
    let root = tempfile::tempdir().unwrap();
    let backend = Arc::new(Fixture(AtomicUsize::new(0)));
    let mut server = XeroMcp::new(
        backend.clone(),
        AccessScope::new("fixture", vec![TENANT.into()], false, false, false).unwrap(),
        root.path().into(),
    );
    assert!(!server
        .tools()
        .iter()
        .any(|tool| tool.name == "xero_prepare_write"));
    server.enable_writes(root.path()).unwrap();
    assert_eq!(
        server.manifest()["source_writes"],
        "operator_approved_opt_in"
    );
    assert!(server
        .tools()
        .iter()
        .any(|tool| tool.name == "xero_prepare_write"));
    let denied = server
        .execute(
            &AccessScope::new("reader", vec![TENANT.into()], false, false, false).unwrap(),
            "xero_prepare_write",
            contact(),
        )
        .await
        .unwrap_err();
    assert_eq!(denied.code, "write_forbidden");
    let writer = AccessScope::new("writer", vec![TENANT.into()], false, false, false)
        .unwrap()
        .with_write_actions(vec!["create_contact".into()])
        .unwrap();
    let prepared = server
        .execute(&writer, "xero_prepare_write", contact())
        .await
        .unwrap();
    let approval_id = Uuid::parse_str(prepared["data"]["approval_id"].as_str().unwrap()).unwrap();
    assert_eq!(backend.0.load(Ordering::SeqCst), 0);
    let unapproved = server
        .execute(
            &writer,
            "xero_confirm_write",
            json!({"approval_id":approval_id}),
        )
        .await
        .unwrap_err();
    assert_eq!(unapproved.code, "write_not_approved");
    let other = AccessScope::new("other", vec![TENANT.into()], false, false, false)
        .unwrap()
        .with_write_actions(vec!["create_contact".into()])
        .unwrap();
    let store = WriteStore::open(root.path().join("writes.sqlite")).unwrap();
    let digest = store.inspect(approval_id).unwrap()["body_sha256"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(store
        .approve(approval_id, "other", TENANT, "create_contact", &digest)
        .is_err());
    assert!(store
        .approve(
            approval_id,
            "writer",
            TENANT,
            "create_contact",
            &"0".repeat(64)
        )
        .is_err());
    store
        .approve(approval_id, "writer", TENANT, "create_contact", &digest)
        .unwrap();
    assert_eq!(
        server
            .execute(
                &other,
                "xero_confirm_write",
                json!({"approval_id":approval_id})
            )
            .await
            .unwrap_err()
            .code,
        "write_forbidden"
    );
    let result = server
        .execute(
            &writer,
            "xero_confirm_write",
            json!({"approval_id":approval_id}),
        )
        .await
        .unwrap();
    assert_eq!(result["data"]["ContactID"], "created");
    assert_eq!(backend.0.load(Ordering::SeqCst), 1);
    assert_eq!(
        server
            .execute(
                &writer,
                "xero_confirm_write",
                json!({"approval_id":approval_id})
            )
            .await
            .unwrap_err()
            .code,
        "write_already_claimed"
    );
}

#[test]
fn write_inputs_are_typed_and_draft_invoice_cannot_authorise() {
    let invalid = json!({"tenant_id":TENANT,"action":"create_draft_invoice","input":{"contact_id":TENANT,"invoice_type":"ACCREC","status":"AUTHORISED","line_items":[{"description":"Work","quantity":1,"unit_amount":10,"account_code":"200"}]}});
    assert!(serde_json::from_value::<WriteRequest>(invalid).is_err());
    let request: WriteRequest = serde_json::from_value(json!({"tenant_id":TENANT,"action":"create_draft_invoice","input":{"contact_id":TENANT,"invoice_type":"ACCREC","line_items":[{"description":"Work","quantity":1,"unit_amount":10,"account_code":"200"}]}})).unwrap();
    request.validate().unwrap();
    assert_eq!(request.action.body()["Invoices"][0]["Status"], "DRAFT");
    assert_eq!(
        request.action.method_path().unwrap().0,
        reqwest::Method::PUT
    );
    assert!(serde_json::from_value::<WriteRequest>(json!({"tenant_id":TENANT,"action":"create_payment","input":{"invoice_id":TENANT,"account_id":TENANT,"amount":0}})).unwrap().validate().is_err());
    let payment: WriteRequest = serde_json::from_value(json!({"tenant_id":TENANT,"action":"create_payment","input":{"invoice_id":TENANT,"account_id":TENANT,"amount":12.50}})).unwrap();
    payment.validate().unwrap();
    assert_eq!(
        payment.action.method_path().unwrap().0,
        reqwest::Method::POST
    );
    assert_eq!(payment.action.body()["Amount"], 12.50);
    let update: WriteRequest = serde_json::from_value(json!({"tenant_id":TENANT,"action":"update_contact","input":{"contact_id":TENANT,"name":"Updated"}})).unwrap();
    update.validate().unwrap();
    assert_eq!(
        update.action.method_path().unwrap().0,
        reqwest::Method::POST
    );
    assert_eq!(update.action.body()["Contacts"][0]["ContactID"], TENANT);
    let payable: WriteRequest = serde_json::from_value(json!({"tenant_id":TENANT,"action":"create_draft_invoice","input":{"contact_id":TENANT,"invoice_type":"ACCPAY","reference":"BILL-42","line_items":[{"description":"Work","quantity":1,"unit_amount":10,"account_code":"200"}]}})).unwrap();
    payable.validate().unwrap();
    assert_eq!(
        payable.action.body()["Invoices"][0]["InvoiceNumber"],
        "BILL-42"
    );
    assert!(payable.action.body()["Invoices"][0]
        .get("Reference")
        .is_none());
    assert!(serde_json::from_value::<WriteRequest>(json!({"tenant_id":TENANT,"action":"create_contact","input":{"name":"Bad","email":"not-an-email"}})).unwrap().validate().is_err());
    assert!(serde_json::from_value::<WriteRequest>(json!({"tenant_id":TENANT,"action":"create_draft_invoice","input":{"contact_id":TENANT,"invoice_type":"ACCREC","date":"2026-10-08","due_date":"2026-10-07","line_items":[{"description":"Work","quantity":1,"unit_amount":10,"account_code":"200"}]}})).unwrap().validate().is_err());
}

#[test]
fn official_mcp_mutation_surface_is_allowlisted_and_tenant_scoped() {
    let actions = official_write_actions();
    assert_eq!(actions.len(), 25);
    for action in [
        "create_contact",
        "create_invoice",
        "create_payment",
        "create_bank_transaction",
        "create_credit_note",
        "create_item",
        "create_manual_journal",
        "create_quote",
        "create_payroll_timesheet",
        "create_tracking_category",
        "create_tracking_options",
        "update_contact",
        "update_invoice",
        "update_bank_transaction",
        "update_credit_note",
        "update_item",
        "update_manual_journal",
        "update_quote",
        "update_tracking_category",
        "update_tracking_options",
        "approve_payroll_timesheet",
        "add_payroll_timesheet_line",
        "update_payroll_timesheet_line",
        "revert_payroll_timesheet",
        "delete_payroll_timesheet",
    ] {
        assert!(actions.contains(&action), "missing {action}");
    }

    let update: WriteRequest = serde_json::from_value(json!({
        "tenant_id": TENANT,
        "action": "official_mcp",
        "input": {
            "operation": "update_invoice",
            "path": {"InvoiceID": TENANT},
            "body": {"Invoices": [{"InvoiceID": TENANT, "Status": "DRAFT"}]}
        }
    }))
    .unwrap();
    let prepared = update.prepare().unwrap();
    assert_eq!(prepared.action, "update_invoice");
    assert_eq!(prepared.method, reqwest::Method::POST);
    assert_eq!(prepared.path, format!("Invoices/{TENANT}"));
    assert_eq!(prepared.base_url, "https://api.xero.com/api.xro/2.0");

    let payroll: WriteRequest = serde_json::from_value(json!({
        "tenant_id": TENANT,
        "action": "official_mcp",
        "input": {"operation": "approve_payroll_timesheet", "path": {"TimesheetID": TENANT}}
    }))
    .unwrap();
    let prepared = payroll.prepare().unwrap();
    assert_eq!(prepared.method, reqwest::Method::POST);
    assert_eq!(prepared.path, format!("Timesheets/{TENANT}/Approve"));
    assert_eq!(prepared.base_url, "https://api.xero.com/payroll.xro/2.0");
    assert!(prepared.body.is_null());

    let unknown: WriteRequest = serde_json::from_value(json!({
        "tenant_id": TENANT,
        "action": "official_mcp",
        "input": {"operation": "delete_everything", "body": {}}
    }))
    .unwrap();
    assert_eq!(unknown.prepare().unwrap_err().code, "invalid_arguments");

    let control_character = format!("line{}break", '\0');
    let unsafe_payload: WriteRequest = serde_json::from_value(json!({
        "tenant_id": TENANT,
        "action": "official_mcp",
        "input": {
            "operation": "update_invoice",
            "path": {"InvoiceID": TENANT},
            "body": {"Invoices": [{"Reference": control_character}]}
        }
    }))
    .unwrap();
    assert_eq!(
        unsafe_payload.prepare().unwrap_err().code,
        "invalid_arguments"
    );
}

#[test]
fn operator_cli_approves_only_the_exact_client_tenant_and_action() {
    let root = tempfile::tempdir().unwrap();
    let store = WriteStore::open(root.path().join("writes.sqlite")).unwrap();
    let writer = AccessScope::new("writer", vec![TENANT.into()], false, false, false)
        .unwrap()
        .with_write_actions(vec!["create_contact".into()])
        .unwrap();
    let request = serde_json::from_value(contact()).unwrap();
    let prepared = store.prepare(&writer, request).unwrap();
    let id = prepared["approval_id"].as_str().unwrap();
    let digest = store.inspect(Uuid::parse_str(id).unwrap()).unwrap()["body_sha256"]
        .as_str()
        .unwrap()
        .to_owned();
    let inspected = std::process::Command::new(env!("CARGO_BIN_EXE_xero-mcp"))
        .env_clear()
        .env("XERO_MCP_STORAGE_ROOT", root.path())
        .args(["--inspect-write", id])
        .output()
        .unwrap();
    assert!(inspected.status.success());
    let details: Value = serde_json::from_slice(&inspected.stdout).unwrap();
    assert_eq!(details["body_sha256"], digest);
    assert_eq!(details["body"]["Contacts"][0]["Name"], "Example Customer");
    let run = |client: &str| {
        std::process::Command::new(env!("CARGO_BIN_EXE_xero-mcp"))
            .env_clear()
            .env("XERO_MCP_STORAGE_ROOT", root.path())
            .args([
                "--approve-write",
                id,
                "--confirm-client",
                client,
                "--confirm-tenant",
                TENANT,
                "--confirm-action",
                "create_contact",
                "--confirm-body-sha256",
                &digest,
            ])
            .output()
            .unwrap()
    };
    assert!(!run("other").status.success());
    assert!(run("writer").status.success());
    assert!(!run("writer").status.success());
    let connection = rusqlite::Connection::open(root.path().join("writes.sqlite")).unwrap();
    let row: (String, i64, Option<i64>) = connection
        .query_row(
            "SELECT status, created_at, approved_at FROM writes WHERE id = ?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(row.0, "prepared");
    assert!(row.2.unwrap() >= row.1);
}

#[tokio::test]
async fn write_finishes_after_caller_cancellation() {
    let root = tempfile::tempdir().unwrap();
    let backend = Arc::new(Fixture(AtomicUsize::new(0)));
    let writer = AccessScope::new("writer", vec![TENANT.into()], false, false, false)
        .unwrap()
        .with_write_actions(vec!["create_contact".into()])
        .unwrap();
    let mut server = XeroMcp::new(backend.clone(), writer.clone(), root.path().into());
    server.enable_writes(root.path()).unwrap();
    let server = Arc::new(server);
    let prepared = server
        .execute(&writer, "xero_prepare_write", contact())
        .await
        .unwrap();
    let id = Uuid::parse_str(prepared["data"]["approval_id"].as_str().unwrap()).unwrap();
    let store = WriteStore::open(root.path().join("writes.sqlite")).unwrap();
    let digest = store.inspect(id).unwrap()["body_sha256"]
        .as_str()
        .unwrap()
        .to_owned();
    store
        .approve(id, "writer", TENANT, "create_contact", &digest)
        .unwrap();
    let caller = tokio::spawn({
        let server = server.clone();
        async move {
            server
                .execute(&writer, "xero_confirm_write", json!({"approval_id":id}))
                .await
        }
    });
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            if store.inspect(id).unwrap()["status"] == "claimed" {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    caller.abort();
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    assert_eq!(store.inspect(id).unwrap()["status"], "committed");
    assert_eq!(backend.0.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn rejected_write_has_durable_rejected_receipt() {
    let root = tempfile::tempdir().unwrap();
    let backend = Arc::new(Fixture(AtomicUsize::new(0)));
    let writer = AccessScope::new("writer", vec![TENANT.into()], false, false, false)
        .unwrap()
        .with_write_actions(vec!["create_payment".into()])
        .unwrap();
    let mut server = XeroMcp::new(backend, writer.clone(), root.path().into());
    server.enable_writes(root.path()).unwrap();
    let request = json!({"tenant_id":TENANT,"action":"create_payment","input":{"invoice_id":TENANT,"account_id":TENANT,"amount":12.50}});
    let prepared = server
        .execute(&writer, "xero_prepare_write", request)
        .await
        .unwrap();
    let id = Uuid::parse_str(prepared["data"]["approval_id"].as_str().unwrap()).unwrap();
    let store = WriteStore::open(root.path().join("writes.sqlite")).unwrap();
    let digest = store.inspect(id).unwrap()["body_sha256"]
        .as_str()
        .unwrap()
        .to_owned();
    store
        .approve(id, "writer", TENANT, "create_payment", &digest)
        .unwrap();
    assert_eq!(
        server
            .execute(&writer, "xero_confirm_write", json!({"approval_id":id}))
            .await
            .unwrap_err()
            .code,
        "xero_write_rejected"
    );
    assert_eq!(store.inspect(id).unwrap()["status"], "rejected");
}
