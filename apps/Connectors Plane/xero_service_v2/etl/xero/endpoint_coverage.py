"""Payload-free route inventory and conservative fresh-generation evidence.

Verification describes the latest operation/tenant attempt, not field parity,
Silver coverage, entitlement availability, or exhaustive parent/ID discovery.
Direct Accounting GET counts are response envelopes, never collection members.
Only explicit CLI subcommands write: populate-catalog is insert-only, while
install-view separately installs the metadata SELECT in the chosen namespace.
"""

import argparse
import json
from pathlib import Path
import re

from google.cloud import bigquery


_SPEC_COMMIT = "fd9d44b04bf4934a7509b8e7ece51a9e0e462e4f"
OUT_OF_SCOPE_OPERATION_IDS = frozenset({
    "getPaymentServices",
    "getBrandingThemePaymentServices",
})
_ACCOUNTING_CATALOG = (
    Path(__file__).resolve().parents[2] / "core/crates/xero-client/accounting_get_catalog.json"
)
CATALOG_SCHEMA = tuple(bigquery.SchemaField(name, "STRING", mode="REQUIRED") for name in (
    "operation_id", "api_family", "path", "extraction_class", "source_endpoint", "spec_commit",
))

# Only aliases actually emitted by EntityType::as_str; unscheduled routes
# retain their direct Accounting GET endpoint instead of invented sync aliases.
_ACCOUNTING_ALIASES = {
    "getAccounts": "accounts", "getBankTransactions": "bank_transactions",
    "getBankTransfers": "bank_transfers", "getBatchPayments": "batch_payments",
    "getBrandingThemes": "branding_themes", "getBudgets": "budgets",
    "getContactGroups": "contact_groups", "getContacts": "contacts",
    "getCreditNotes": "credit_notes", "getCurrencies": "currencies",
    "getExpenseClaims": "expense_claims", "getInvoices": "invoices", "getItems": "items",
    "getJournals": "journals", "getLinkedTransactions": "linked_transactions",
    "getManualJournals": "manual_journals", "getOrganisations": "organisations",
    "getOverpayments": "overpayments", "getPayments": "payments",
    "getPaymentServices": "payment_services", "getPrepayments": "prepayments",
    "getPurchaseOrders": "purchase_orders", "getQuotes": "quotes", "getReceipts": "receipts",
    "getRepeatingInvoices": "repeating_invoices", "getTaxRates": "tax_rates",
    "getTrackingCategories": "tracking_categories", "getUsers": "users",
    "getReportAgedPayablesByContact": "report_aged_payables_by_contact",
    "getReportAgedReceivablesByContact": "report_aged_receivables_by_contact",
    "getReportBalanceSheet": "report_balance_sheet", "getReportBankSummary": "report_bank_summary",
    "getReportBudgetSummary": "report_budget_summary",
    "getReportExecutiveSummary": "report_executive_summary",
    "getReportProfitAndLoss": "report_profit_and_loss",
    "getReportTrialBalance": "report_trial_balance",
}

# Official GET operation IDs/path templates at the parity document's revision,
# mapped to extended.rs and extended/finance.rs storage endpoints. Identity's
# official /Connections spelling differs from Rust's /connections request.
_EXTENDED_ROUTES = (
    ("getAssets", "assets", "/Assets", "collection", "assets_assets"),
    ("getAssetById", "assets", "/Assets/{id}", "detail", "assets_asset_details"),
    ("getAssetTypes", "assets", "/AssetTypes", "collection", "assets_asset_types"),
    ("getAssetSettings", "assets", "/Settings", "configuration", "assets_settings"),
    ("getFeedConnections", "bankfeeds", "/FeedConnections", "collection",
     "bankfeeds_feed_connections"),
    ("getFeedConnection", "bankfeeds", "/FeedConnections/{id}", "detail",
     "bankfeeds_feed_connection_details"),
    ("getStatements", "bankfeeds", "/Statements", "collection", "bankfeeds_statements"),
    ("getStatement", "bankfeeds", "/Statements/{statementId}", "detail",
     "bankfeeds_statement_details"),
    ("getProjects", "projects", "/Projects", "collection", "projects_projects"),
    ("getProject", "projects", "/Projects/{projectId}", "detail", "projects_project_details"),
    ("getProjectUsers", "projects", "/ProjectsUsers", "collection", "projects_users"),
    ("getTasks", "projects", "/Projects/{projectId}/Tasks", "collection", "projects_tasks"),
    ("getTask", "projects", "/Projects/{projectId}/Tasks/{taskId}", "detail",
     "projects_task_details"),
    ("getTimeEntries", "projects", "/Projects/{projectId}/Time", "collection", "projects_time"),
    ("getTimeEntry", "projects", "/Projects/{projectId}/Time/{timeEntryId}", "detail",
     "projects_time_details"),
    ("getConnections", "identity", "/Connections", "entitlement", "identity_connections"),
    ("getCashValidation", "finance", "/CashValidation", "report", "finance_cash_validation"),
    ("getFinancialStatementBalanceSheet", "finance", "/FinancialStatements/BalanceSheet",
     "report", "finance_balance_sheet"),
    ("getFinancialStatementCashflow", "finance", "/FinancialStatements/Cashflow",
     "report", "finance_cashflow"),
    ("getFinancialStatementProfitAndLoss", "finance", "/FinancialStatements/ProfitAndLoss",
     "report", "finance_profit_and_loss"),
    ("getFinancialStatementTrialBalance", "finance", "/FinancialStatements/TrialBalance",
     "report", "finance_trial_balance"),
    ("getFinancialStatementContactsRevenue", "finance", "/FinancialStatements/contacts/revenue",
     "report", "finance_revenue_by_contact"),
    ("getFinancialStatementContactsExpense", "finance", "/FinancialStatements/contacts/expense",
     "report", "finance_expense_by_contact"),
    ("getBankStatementAccounting", "finance", "/BankStatementsPlus/statements",
     "report", "finance_bank_statements_plus"),
)


def _accounting_class(operation: dict) -> str:
    path = operation["path"]
    if set(operation["response_content_types"]) & {"application/pdf", "application/octet-stream"}:
        return "document"
    if path.endswith("/History"):
        return "history"
    if path.endswith("/Attachments"):
        return "attachment_metadata"
    if path == "/Reports" or path.startswith("/Reports/"):
        return "report"
    if operation["operation_id"] in {"getInvoiceReminders", "getOrganisationActions"}:
        return "configuration"
    has_path_parameter = any(p["location"] == "path" for p in operation["parameters"])
    return "detail" if has_path_parameter else "collection"


def catalog_rows() -> list[dict[str, str]]:
    """Return independent rows for exactly the documented 150 GET operations."""
    catalog = json.loads(_ACCOUNTING_CATALOG.read_text(encoding="utf-8"))
    if catalog["spec_commit"] != _SPEC_COMMIT or len(catalog["operations"]) != 126:
        raise ValueError("Accounting catalog revision/count differs from extended route baseline")
    accounting = [
        {"operation_id": op["operation_id"], "api_family": "accounting", "path": op["path"],
         "extraction_class": _accounting_class(op),
         "source_endpoint": _ACCOUNTING_ALIASES.get(
             op["operation_id"], "accounting_get_" + op["operation_id"].lower()),
         "spec_commit": catalog["spec_commit"]}
        for op in catalog["operations"]
    ]
    extended = [
        dict(zip((field.name for field in CATALOG_SCHEMA), (*route, _SPEC_COMMIT), strict=True))
        for route in _EXTENDED_ROUTES
    ]
    rows = sorted((*accounting, *extended),
                  key=lambda row: (row["api_family"], row["operation_id"]))
    if len(rows) != 150 or len({row["operation_id"] for row in rows}) != 150:
        raise ValueError("endpoint catalog must contain 150 unique operations")
    return rows


def _namespace(project: str, dataset: str) -> str:
    if not isinstance(project, str) or not re.fullmatch(r"[A-Za-z][A-Za-z0-9-]{0,62}", project):
        raise ValueError("invalid BigQuery project identifier")
    if not isinstance(dataset, str) or not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]{0,1023}", dataset):
        raise ValueError("invalid BigQuery dataset identifier")
    return f"{project}.{dataset}"


_COVERAGE_SQL = """WITH catalog AS (
  SELECT operation_id, api_family, path, extraction_class, source_endpoint, spec_commit
  FROM `{namespace}.endpoint_catalog`
  WHERE spec_commit = '{spec_commit}'
), endpoint_map AS (
  SELECT c.*, c.source_endpoint AS endpoint, FALSE AS is_direct
  FROM catalog AS c WHERE c.source_endpoint NOT LIKE 'accounting_get_%'
  UNION ALL
  SELECT c.*, CONCAT('accounting_get_', LOWER(c.operation_id)) AS endpoint, TRUE AS is_direct
  FROM catalog AS c WHERE c.api_family = 'accounting'
), known_tenants AS (
  SELECT tenant_id FROM `{namespace}.sync_run_entities`
  UNION DISTINCT
  SELECT tenant_id FROM `{namespace}.ingest_receipts`
), tenants AS (
  SELECT tenant_id FROM known_tenants
  WHERE tenant_id IS NOT NULL
    AND TRIM(tenant_id) <> '' AND LOWER(TRIM(tenant_id)) NOT IN ('run', 'global')
), manifests AS (
  SELECT m.tenant_id, m.run_id, m.endpoint, c.operation_id, c.api_family,
    MAX(COALESCE(s.finished_at, m.synced_at)) AS last_observed,
    SUM(m.pages) AS manifest_pages, SUM(m.records) AS manifest_records,
    SUM(CASE WHEN NOT m.error_present AND m.outcome_class IS NULL THEN 1 ELSE 0 END)
      AS successful_outcomes,
    MAX(CASE WHEN m.outcome_class = 'not_applicable' THEN 1 ELSE 0 END)
      AS has_not_applicable,
    MAX(CASE WHEN m.outcome_class = 'source_unavailable' THEN 1 ELSE 0 END)
      AS has_source_unavailable,
    MAX(CASE WHEN m.outcome_class = 'access_denied' THEN 1 ELSE 0 END)
      AS has_access_denied,
    MAX(CASE WHEN m.outcome_class IN ('failed_retryable', 'failed_permanent', 'failed_unknown')
      OR (m.outcome_class IS NULL AND (m.error_present OR m.termination IN
        ('error', 'hit_max_pages_cap', 'partial_detail_failure')))
      THEN 1 ELSE 0 END) AS failed,
    MAX(CASE WHEN s.run_id IS NULL OR s.status NOT IN ('succeeded', 'failed')
      OR s.status IS NULL OR s.finished_at IS NULL OR m.synced_at IS NULL
      OR m.error_present IS NULL OR m.pages IS NULL
      OR (m.outcome_class IS NULL AND m.pages <= 0)
      OR m.records IS NULL OR m.records < 0
      OR (m.outcome_class IS NULL AND c.is_direct AND (m.pages <> 1 OR m.records <> 1))
      THEN 1 ELSE 0 END) AS invalid_manifest,
    MAX(CASE
      WHEN m.outcome_class IS NOT NULL THEN 0
      WHEN c.is_direct AND m.termination = 'single_response' THEN 0
      WHEN NOT c.is_direct AND m.termination = 'ended_on_empty_page' THEN 0
      WHEN NOT c.is_direct AND c.operation_id = 'getJournals'
        AND m.termination = 'offset_not_advancing' THEN 0
      WHEN NOT c.is_direct AND c.extraction_class = 'report'
        AND m.termination = 'report-snapshot' THEN 0
      ELSE 1 END) AS invalid_termination
  FROM `{namespace}.sync_run_entities` AS m
  JOIN endpoint_map AS c ON m.endpoint = c.endpoint
  LEFT JOIN `{namespace}.sync_runs` AS s ON m.run_id = s.run_id
  GROUP BY m.tenant_id, m.run_id, m.endpoint, c.operation_id, c.api_family
), receipt_pages AS (
  SELECT r.*, c.operation_id, c.api_family, c.extraction_class, c.is_direct,
    CASE WHEN c.extraction_class = 'report'
      AND JSON_VALUE(r.metadata_json, '$."x-sync-type"') = 'report-snapshot'
      AND JSON_QUERY(r.metadata_json, '$."x-page"') IS NULL THEN 1
      ELSE SAFE_CAST(JSON_VALUE(r.metadata_json, '$."x-page"') AS INT64) END AS page_number,
    CASE WHEN c.is_direct AND (
      COALESCE(JSON_VALUE(r.metadata_json, '$."x-operation-id"'), '') <> c.operation_id
      OR COALESCE(JSON_VALUE(r.metadata_json, '$."x-spec-commit"'), '') <> c.spec_commit
    ) THEN 1 ELSE 0 END AS invalid_operation,
    CASE WHEN JSON_VALUE(r.metadata_json, '$."x-sync-type"') IN ('master', 'open-sweep')
      AND JSON_QUERY(r.metadata_json, '$."x-where"') IS NULL
      AND JSON_QUERY(r.metadata_json, '$."x-modified-after"') IS NULL
      AND JSON_QUERY(r.metadata_json, '$."x-business-from"') IS NULL
      AND JSON_QUERY(r.metadata_json, '$."x-business-to"') IS NULL
      AND JSON_QUERY(r.metadata_json, '$."x-request-key"') IS NULL
      AND JSON_QUERY(r.metadata_json, '$."x-snapshot-date"') IS NULL
      AND JSON_QUERY(r.metadata_json, '$."x-filter"') IS NULL
      AND JSON_QUERY(r.metadata_json, '$."x-status"') IS NULL
      THEN 0 ELSE 1 END AS bounded_request,
    CASE WHEN JSON_VALUE(r.metadata_json, '$."x-sync-type"') IN
      ('master', 'open-sweep', 'incremental', 'rolling-full', 'backfill')
      THEN 0 ELSE 1 END AS unknown_collection_mode
  FROM `{namespace}.ingest_receipts` AS r
  JOIN endpoint_map AS c ON r.endpoint = c.endpoint
), receipts AS (
  SELECT tenant_id, run_id, endpoint, operation_id, api_family,
    MAX(observed_at) AS last_observed,
    SUM(CASE WHEN http_status = 200 THEN expected_records END) AS extracted_records,
    SUM(CASE WHEN http_status = 200 THEN record_count END) AS staging_records,
    SUM(CASE WHEN http_status = 200 THEN 1 ELSE 0 END) AS loaded_pages,
    COUNT(DISTINCT CASE WHEN http_status = 200 THEN page_number END) AS distinct_pages,
    MIN(CASE WHEN http_status = 200 THEN page_number END) AS first_page,
    MAX(CASE WHEN http_status = 200 THEN page_number END) AS last_page,
    MAX(CASE WHEN http_status IS NULL OR http_status NOT IN (200, 304)
      OR observed_at IS NULL OR receipt_id IS NULL OR receipt_id = ''
      OR (http_status = 200 AND (invalid_operation = 1
      OR page_number IS NULL OR page_number <= 0 OR expected_records IS NULL
      OR expected_records < 0 OR record_count IS NULL OR record_count < 0))
      THEN 1 ELSE 0 END) AS invalid_receipt,
    MAX(CASE WHEN http_status = 200 AND record_count > expected_records
      THEN 1 ELSE 0 END) AS overfilled_page,
    MAX(CASE WHEN http_status = 200 AND record_count < expected_records
      AND NOT (extraction_class = 'document' AND is_direct AND record_count = 0)
      THEN 1 ELSE 0 END) AS underfilled_page,
    MAX(CASE WHEN http_status = 200 THEN bounded_request ELSE 0 END) AS bounded_request,
    MAX(CASE WHEN http_status = 200 THEN unknown_collection_mode ELSE 0 END)
      AS unknown_collection_mode
  FROM receipt_pages
  GROUP BY tenant_id, run_id, endpoint, operation_id, api_family
), event_keys AS (
  SELECT tenant_id, run_id, endpoint, operation_id, api_family, last_observed
  FROM manifests
  UNION ALL
  SELECT tenant_id, run_id, endpoint, operation_id, api_family, last_observed
  FROM receipts
), events AS (
  SELECT tenant_id, run_id, endpoint, operation_id, api_family,
    MAX(last_observed) AS last_observed
  FROM event_keys
  GROUP BY tenant_id, run_id, endpoint, operation_id, api_family
), classified AS (
  SELECT e.*, r.extracted_records, r.staging_records,
    CASE
      WHEN m.has_access_denied = 1 THEN 'access_denied'
      WHEN m.has_source_unavailable = 1 THEN 'source_unavailable'
      WHEN m.failed = 1 THEN 'failed'
      WHEN m.successful_outcomes = 0 AND m.has_not_applicable = 1 THEN 'not_applicable'
      WHEN m.run_id IS NULL OR m.invalid_manifest = 1 OR m.invalid_termination = 1
        OR r.invalid_receipt = 1 OR r.overfilled_page = 1 THEN 'incomplete'
      WHEN COALESCE(r.loaded_pages, 0) < m.manifest_pages THEN 'awaiting_staging'
      WHEN r.loaded_pages <> m.manifest_pages OR r.distinct_pages <> r.loaded_pages
        OR r.first_page <> 1 OR r.last_page <> m.manifest_pages
        OR r.extracted_records <> m.manifest_records THEN 'incomplete'
      WHEN r.underfilled_page = 1 THEN 'awaiting_staging'
      WHEN c.is_direct OR c.extraction_class <> 'collection' OR c.path LIKE '%{{%'
        THEN 'response_verified'
      WHEN r.unknown_collection_mode = 1 THEN 'incomplete'
      WHEN r.bounded_request = 0 THEN 'full_collection_verified'
      ELSE 'incremental_verified'
    END AS operation_status
  FROM events AS e
  JOIN endpoint_map AS c ON e.endpoint = c.endpoint AND e.operation_id = c.operation_id
    AND e.api_family = c.api_family
  LEFT JOIN manifests AS m ON e.tenant_id = m.tenant_id
    AND e.run_id = m.run_id AND e.endpoint = m.endpoint AND e.operation_id = m.operation_id
    AND e.api_family = m.api_family
  LEFT JOIN receipts AS r ON e.tenant_id = r.tenant_id
    AND e.run_id = r.run_id AND e.endpoint = r.endpoint AND e.operation_id = r.operation_id
    AND e.api_family = r.api_family
), ranked AS (
  SELECT e.*, ROW_NUMBER() OVER (
    PARTITION BY tenant_id, api_family, operation_id
    ORDER BY last_observed DESC,
      CASE operation_status WHEN 'failed' THEN 0 WHEN 'incomplete' THEN 1
        WHEN 'awaiting_staging' THEN 2 ELSE 3 END, run_id DESC, endpoint DESC
  ) AS event_rank FROM classified AS e
)
SELECT 'main' AS generation, t.tenant_id, c.api_family, c.operation_id, c.path, c.extraction_class,
  e.last_observed, e.extracted_records, e.staging_records,
  CASE WHEN c.operation_id IN ('getPaymentServices', 'getBrandingThemePaymentServices')
    AND e.operation_id IS NOT NULL THEN 'policy_violation'
  WHEN c.operation_id IN ('getPaymentServices', 'getBrandingThemePaymentServices')
    THEN 'out_of_scope'
    ELSE COALESCE(e.operation_status, 'not_attempted')
  END AS operation_status
FROM tenants AS t CROSS JOIN catalog AS c
LEFT JOIN ranked AS e ON t.tenant_id = e.tenant_id
  AND c.api_family = e.api_family AND c.operation_id = e.operation_id AND e.event_rank = 1"""


def coverage_query(project: str, dataset: str) -> str:
    """Return one GoogleSQL SELECT without accessing clients or financial payloads.

    Counts come from HTTP 200 receipts, excluding terminal 304s. Certification
    requires the matching generation/run/endpoint/tenant manifest and every page,
    with per-page as well as aggregate reconciliation. Unknown termination is
    incomplete; error/page-cap outcomes fail. Missing evidence is never zero.
    """
    return _COVERAGE_SQL.format(namespace=_namespace(project, dataset), spec_commit=_SPEC_COMMIT)


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--project", required=True)
    parser.add_argument("--dataset", required=True)
    parser.add_argument("action", choices=("populate-catalog", "install-view"))
    args = parser.parse_args(argv)
    namespace = _namespace(args.project, args.dataset)
    client = bigquery.Client(project=args.project)
    if args.action == "populate-catalog":
        from etl.common.bq_writer import BQWriter

        rows = catalog_rows()
        table = bigquery.Table(f"{namespace}.endpoint_catalog", schema=CATALOG_SCHEMA)
        client.create_table(table, exists_ok=True)
        BQWriter(args.project, args.dataset).insert_if_absent(
            "endpoint_catalog", rows,
            key_columns=("spec_commit", "api_family", "operation_id"), schema=CATALOG_SCHEMA,
        )
    else:
        view = bigquery.Table(f"{namespace}.endpoint_coverage")
        view.view_query = coverage_query(args.project, args.dataset)
        view.view_use_legacy_sql = False
        client.create_table(view, exists_ok=True)
        client.update_table(view, ("view_query", "view_use_legacy_sql"))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
