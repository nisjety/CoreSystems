"""Read-only, payload-free evidence matrix and nightly Xero exception report."""

import argparse
import json
import re
from datetime import datetime, timedelta, timezone

from google.cloud import bigquery

from etl.xero.extended import COLLECTION_ENDPOINTS


ACCOUNTING_HEADERS = frozenset({
    "accounts", "bank_transactions", "bank_transfers", "batch_payments",
    "branding_themes", "budgets", "contact_groups", "contacts", "credit_notes",
    "currencies", "invoices", "items", "journals", "linked_transactions",
    "manual_journals", "organisations", "overpayments", "payments",
    "purchase_orders", "quotes", "repeating_invoices", "tax_rates",
    "tracking_categories", "users",
})
SILVER_HEADERS = {
    "accounts": "dim_account", "contacts": "dim_contact",
    "invoices": "fact_invoice", "credit_notes": "fact_credit_note",
    "bank_transactions": "fact_bank_transaction", "payments": "fact_payment",
    "journals": "fact_journal", "manual_journals": "fact_manual_journal",
    "purchase_orders": "fact_purchase_order", "quotes": "fact_quote",
    "budgets": "fact_budget",
}
EXCEPTIONS = frozenset({
    "failed_run", "missing_run", "stale", "duplicate_keys", "exact_gap",
    "pending_staging", "pending_current_model",
})


def safe_dataset(value: str) -> str:
    if value not in {"staging_xero", "staging_xero_sensitive"}:
        raise ValueError("dataset must be staging_xero or staging_xero_sensitive")
    return value


def _project(value: str) -> str:
    if not re.fullmatch(r"[a-z][a-z0-9-]{3,62}", value):
        raise ValueError("invalid GCP project ID")
    return value


def assess(outcomes, staging, silver, expected, now, max_age_hours, memberships=None):
    """Classify collection parity exactly; other accumulated counts are advisory."""
    observed = {(row["tenant_id"], row["endpoint"]): row for row in outcomes}
    memberships = memberships or {}
    keys = sorted(set(observed) | set(expected))
    result = []
    for tenant, endpoint in keys:
        row = observed.get((tenant, endpoint))
        stage = staging.get((tenant, endpoint))
        model = silver.get((tenant, endpoint))
        exact = endpoint in COLLECTION_ENDPOINTS
        raw_count = row["records"] if row else None
        stage_count = stage.get("rows") if stage else None
        model_count = model.get("rows") if model else None
        membership = memberships.get((tenant, endpoint, row["run_id"])) if row else None
        status = "advisory"
        if row is None:
            status = "missing_run"
        elif row["failed"]:
            status = "failed_run"
        elif now - row["observed_at"] > timedelta(hours=max_age_hours):
            status = "stale"
        elif stage and stage.get("distinct_keys") is not None and stage_count != stage["distinct_keys"]:
            status = "duplicate_keys"
        elif raw_count == 0 and (not exact or (membership is not None and
             membership["lists"] == row.get("requests") and membership["records"] == 0
             and model_count in (None, 0))):
            status = "no_data"
        elif exact:
            if model is None or model.get("run_id") != row["run_id"]:
                status = "pending_current_model"
            elif raw_count != model_count:
                status = "exact_gap"
            else:
                status = "ok"
        elif raw_count and not stage_count:
            status = "pending_staging"
        result.append({
            "tenant_id": tenant, "endpoint": endpoint,
            "check_class": "exact" if exact else "advisory",
            "status": status, "run_id": row["run_id"] if row else None,
            "mode": row["mode"] if row else None,
            "observed_at": row["observed_at"].isoformat() if row else None,
            "raw_pages": row["pages"] if row else None,
            "list_requests": row.get("requests") if row else None,
            "membership_lists": membership["lists"] if membership else None,
            "membership_records": membership["records"] if membership else None,
            "raw_records": raw_count,
            "staging_rows": stage_count,
            "staging_distinct_keys": stage.get("distinct_keys") if stage else None,
            "silver_rows": model_count,
        })
    return result


def _query_rows(client, sql: str):
    return [dict(row) for row in client.query(sql).result()]


def _tables(client, project: str, dataset: str) -> set[str]:
    sql = f"SELECT table_name FROM `{project}.{dataset}.INFORMATION_SCHEMA.TABLES`"
    return {row["table_name"] for row in _query_rows(client, sql)}


def _latest_outcomes(client, project: str, dataset: str):
    sql = f"""
WITH grouped AS (
  SELECT e.tenant_id, e.endpoint, e.run_id, ANY_VALUE(r.mode) AS mode,
    SUM(e.pages) AS pages, SUM(e.records) AS records, COUNT(*) AS requests,
    MAX(e.synced_at) AS observed_at,
    LOGICAL_OR(e.error_present OR e.termination IN ('error', 'hit_max_pages_cap')) AS failed
  FROM `{project}.{dataset}.sync_run_entities` AS e
  JOIN `{project}.{dataset}.sync_runs` AS r USING (run_id)
  GROUP BY e.tenant_id, e.endpoint, e.run_id
)
SELECT * FROM grouped
QUALIFY ROW_NUMBER() OVER (
  PARTITION BY tenant_id, endpoint ORDER BY observed_at DESC, run_id DESC
) = 1
"""
    return _query_rows(client, sql)


def _summary(client, project: str, dataset: str, table: str, endpoint=None):
    if endpoint is None:
        sql = f"""SELECT tenant_id, COUNT(*) AS rows,
          COUNT(DISTINCT record_id) AS distinct_keys
          FROM `{project}.{dataset}.{table}` GROUP BY tenant_id"""
    else:
        sql = f"""SELECT tenant_id, endpoint, COUNT(*) AS rows,
          COUNT(DISTINCT record_id) AS distinct_keys
          FROM `{project}.{dataset}.{table}` GROUP BY tenant_id, endpoint"""
    return _query_rows(client, sql)


def _memberships(client, project: str, dataset: str):
    sql = f"""SELECT tenant_id, source_endpoint, run_id,
      COUNT(*) AS lists, SUM(record_count) AS records
      FROM `{project}.{dataset}.extended_list_snapshots`
      GROUP BY tenant_id, source_endpoint, run_id"""
    return {
        (row["tenant_id"], row["source_endpoint"], row["run_id"]): row
        for row in _query_rows(client, sql)
    }


def warehouse_evidence(client, project: str, dataset: str, expected, now, max_age_hours):
    """Read only the selected security domain; never return payloads or error text."""
    project, dataset = _project(project), safe_dataset(dataset)
    staging_tables = _tables(client, project, dataset)
    if not {"sync_runs", "sync_run_entities"}.issubset(staging_tables):
        raise RuntimeError("run-history tables are missing; replay manifests before reporting")
    outcomes = _latest_outcomes(client, project, dataset)
    staging = {}
    if "extended_records" in staging_tables:
        for row in _summary(client, project, dataset, "extended_records", endpoint=True):
            staging[(row["tenant_id"], row["endpoint"])] = row
    if dataset == "staging_xero":
        for endpoint in sorted(ACCOUNTING_HEADERS & staging_tables):
            for row in _summary(client, project, dataset, endpoint):
                staging[(row["tenant_id"], endpoint)] = row

    silver_dataset = ("dw_1_silver_xero_sensitive" if dataset == "staging_xero_sensitive"
                      else "dw_1_silver_xero")
    silver_tables = _tables(client, project, silver_dataset)
    current_table = ("current_sensitive_extended_records" if dataset == "staging_xero_sensitive"
                     else "current_extended_records")
    silver = {}
    if current_table in silver_tables:
        sql = f"""SELECT tenant_id, endpoint, run_id, COUNT(*) AS rows
          FROM `{project}.{silver_dataset}.{current_table}`
          GROUP BY tenant_id, endpoint, run_id"""
        for row in _query_rows(client, sql):
            silver[(row["tenant_id"], row["endpoint"])] = row
    if dataset == "staging_xero":
        for endpoint, table in SILVER_HEADERS.items():
            if table not in silver_tables:
                continue
            sql = f"SELECT tenant_id, COUNT(*) AS rows FROM `{project}.{silver_dataset}.{table}` GROUP BY tenant_id"
            for row in _query_rows(client, sql):
                silver[(row["tenant_id"], endpoint)] = row
    memberships = (_memberships(client, project, dataset)
                   if "extended_list_snapshots" in staging_tables else {})
    return assess(outcomes, staging, silver, expected, now, max_age_hours, memberships)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--project", required=True)
    parser.add_argument("--dataset", choices=["staging_xero", "staging_xero_sensitive"],
                        default="staging_xero")
    parser.add_argument("--max-age-hours", type=int, default=36)
    parser.add_argument("--expect", action="append", default=[], metavar="TENANT:ENDPOINT")
    parser.add_argument("--fail-on-exceptions", action="store_true")
    args = parser.parse_args(argv)
    if args.max_age_hours <= 0:
        parser.error("--max-age-hours must be positive")
    expected = []
    for value in args.expect:
        tenant, separator, endpoint = value.partition(":")
        if not separator or not re.fullmatch(r"[A-Za-z0-9_-]{1,128}", tenant) or not re.fullmatch(
                r"[a-z][a-z0-9_]*", endpoint):
            parser.error("--expect must be TENANT:ENDPOINT")
        expected.append((tenant, endpoint))
    rows = warehouse_evidence(bigquery.Client(project=_project(args.project)), args.project,
                              args.dataset, expected, datetime.now(timezone.utc), args.max_age_hours)
    print(json.dumps({"dataset": args.dataset, "generated_at": datetime.now(timezone.utc).isoformat(),
                      "exceptions": sum(row["status"] in EXCEPTIONS for row in rows),
                      "rows": rows}, indent=2))
    return 2 if args.fail_on_exceptions and any(row["status"] in EXCEPTIONS for row in rows) else 0


if __name__ == "__main__":
    raise SystemExit(main())
