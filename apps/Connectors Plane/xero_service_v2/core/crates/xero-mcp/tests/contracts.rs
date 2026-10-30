use serde_json::{json, Value};
use xero_mcp::{catalog, config::AccessScope};

#[test]
fn all_pinned_reads_have_tenant_scoped_schemas_and_fixed_hosts() {
    let operations = catalog::operations();
    assert_eq!(catalog::catalog().operations.len(), 186);
    assert_eq!(operations.len(), 184);
    assert_eq!(
        operations
            .iter()
            .filter(|op| op.family == "accounting")
            .count(),
        124
    );
    for op in operations {
        assert!(op.base_url.starts_with("https://api.xero.com"));
        let schema = op.tool_schema();
        assert_eq!(schema["additionalProperties"], false);
        assert!(schema["required"]
            .as_array()
            .unwrap()
            .contains(&json!("tenant_id")));
        assert!(schema["properties"].get("authorization").is_none());
    }
}

#[test]
fn official_mcp_read_operations_are_catalogued() {
    // Current upstream Xero MCP source: 25 list tools plus get-timesheet.
    // This server keeps the same functions within its tenant-scoped OpenAPI
    // catalog rather than duplicating their narrow per-tool implementations.
    let official_reads = [
        ("accounting", "getAccounts"),
        ("accounting", "getReportAgedPayablesByContact"),
        ("accounting", "getReportAgedReceivablesByContact"),
        ("accounting", "getBankTransactions"),
        ("accounting", "getContactGroups"),
        ("accounting", "getContacts"),
        ("accounting", "getCreditNotes"),
        ("accounting", "getInvoices"),
        ("accounting", "getItems"),
        ("accounting", "getManualJournals"),
        ("accounting", "getOrganisations"),
        ("accounting", "getPayments"),
        ("payroll_nz", "getEmployeeLeaves"),
        ("payroll_nz", "getEmployeeLeaveBalances"),
        ("payroll_nz", "getEmployeeLeaveTypes"),
        ("payroll_nz", "getEmployees"),
        ("payroll_nz", "getEmployeeLeavePeriods"),
        ("payroll_nz", "getLeaveTypes"),
        ("accounting", "getReportProfitAndLoss"),
        ("accounting", "getQuotes"),
        ("accounting", "getReportBalanceSheet"),
        ("accounting", "getTaxRates"),
        ("payroll_nz", "getTimesheets"),
        ("accounting", "getTrackingCategories"),
        ("accounting", "getReportTrialBalance"),
        ("payroll_nz", "getTimesheet"),
    ];
    assert_eq!(official_reads.len(), 26);
    for (family, operation_id) in official_reads {
        assert!(
            catalog::operation(family, operation_id).is_ok(),
            "missing upstream Xero MCP read: {family}/{operation_id}"
        );
    }
}

#[test]
fn missing_required_path_and_unknown_parameters_fail_before_fetch() {
    let op = catalog::operation("accounting", "getInvoice").unwrap();
    assert!(op
        .prepare(
            &Default::default(),
            &Default::default(),
            &Default::default()
        )
        .is_err());
    let unknown = serde_json::from_value(json!({"redirect":"https://other.invalid"})).unwrap();
    let list = catalog::operation("accounting", "getInvoices").unwrap();
    assert!(list
        .prepare(&Default::default(), &unknown, &Default::default())
        .is_err());
}

#[test]
fn tenant_authority_and_retention_cannot_be_changed_by_tool_arguments() {
    let tenant = "5f3c9a2e-0000-0000-0000-000000000000";
    let scope = AccessScope::new("model-plane", vec![tenant.into()], true, false, true).unwrap();
    assert!(scope.authorize(tenant, false, false).is_ok());
    assert!(scope
        .authorize("aeb4f270-0000-0000-0000-000000000000", false, false)
        .is_err());
    assert!(scope.authorize(tenant, true, false).is_err());
    assert!(scope.authorize(tenant, false, true).is_err());
}

fn fixture_value(schema: &Value) -> Value {
    if let Some(value) = schema["enum"].as_array().and_then(|items| items.first()) {
        return value.clone();
    }
    match (schema["type"].as_str(), schema["format"].as_str()) {
        (_, Some("uuid")) => json!("5f3c9a2e-0000-0000-0000-000000000000"),
        (_, Some("date")) => json!("2026-01-01"),
        (_, Some("date-time")) => json!("2026-01-01T00:00:00Z"),
        (Some("integer"), _) | (Some("number"), _) => json!(1),
        (Some("boolean"), _) => json!(true),
        (Some("array"), _) => json!([fixture_value(&schema["items"])]),
        _ => json!("fixture"),
    }
}

#[test]
fn every_operation_prepares_all_documented_parameter_types_without_changing_host() {
    for op in catalog::operations() {
        let params = |location| {
            op.parameters
                .iter()
                .filter(|p| p.location == location)
                .map(|p| (p.name.clone(), fixture_value(&p.schema)))
                .collect()
        };
        let prepared = op
            .prepare(&params("path"), &params("query"), &params("header"))
            .unwrap_or_else(|error| panic!("{}: {error}", op.tool_name()));
        assert_eq!(prepared.url.host_str(), Some("api.xero.com"));
        assert_eq!(prepared.url.scheme(), "https");
        assert_eq!(prepared.binary, op.binary());
    }
}

#[test]
fn invalid_filter_arrays_and_pagination_are_rejected() {
    let op = catalog::operation("accounting", "getInvoices").unwrap();
    for query in [
        json!({"IDs":["not-a-uuid"]}),
        json!({"InvoiceNumbers":["a,b"]}),
        json!({"pageSize":0}),
        json!({"pageSize":101}),
        json!({"page":-1}),
        json!({"page":1_000_001}),
        json!({"summaryOnly":"true"}),
        json!({"Statuses":[1]}),
    ] {
        assert!(
            op.prepare(
                &Default::default(),
                &serde_json::from_value(query.clone()).unwrap(),
                &Default::default()
            )
            .is_err(),
            "accepted {query}"
        );
    }
}
