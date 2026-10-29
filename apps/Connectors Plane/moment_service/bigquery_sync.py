"""
BigQuery sync utilities for the Moment service.

Each entity gets a curated set of typed, commonly-queried columns PLUS a
`raw_json` column holding the untouched API payload (with a couple of huge,
redundant embedded objects stripped where noted). That combination means a
mapping mistake or an unmapped field never means lost data - anything not
surfaced as its own column is still queryable from `raw_json` via BigQuery's
JSON functions.

Every sync here upserts a full entity snapshot via GCPClient.merge_rows, not a
streaming insert and not a truncate-replace: new rows are inserted, existing
rows are updated in place (first_synced_at preserved), and rows that vanished
from Moment are soft-flagged is_deleted_in_moment - never deleted. Re-running a
sync is therefore idempotent, keeps history, and never accumulates duplicates.

Field names/shapes below were captured from live Moment (Milient Tibet) API
responses for company "aquatiqas" - see the includes-vs-bare-endpoint notes
inline where they affect what data is captured.
"""

import json
import logging
from datetime import datetime, timezone
from typing import Any, Dict, List, Optional

logger = logging.getLogger(__name__)

AUDIT_FIELDS = [
    {"name": "version", "type": "STRING", "mode": "NULLABLE"},
    {"name": "created_date", "type": "TIMESTAMP", "mode": "NULLABLE"},
    {"name": "last_modified_date", "type": "TIMESTAMP", "mode": "NULLABLE"},
    {"name": "created_by_system_user", "type": "INTEGER", "mode": "NULLABLE"},
    {"name": "created_by_system_user_full_name", "type": "STRING", "mode": "NULLABLE"},
    {"name": "last_modified_by_system_user", "type": "INTEGER", "mode": "NULLABLE"},
    {"name": "last_modified_by_system_user_full_name", "type": "STRING", "mode": "NULLABLE"},
]
TRAILER_FIELDS = [
    {"name": "raw_json", "type": "STRING", "mode": "NULLABLE"},
    # Last time this row was seen in a Moment snapshot (refreshed on every
    # merge that still contains the record).
    {"name": "synced_at", "type": "TIMESTAMP", "mode": "REQUIRED"},
    # Merge-tracking columns, computed by GCPClient.merge_rows (not taken from
    # the row dicts): when we first saw the record, and soft-delete flags for
    # records that later disappear from Moment - rows are never dropped.
    {"name": "first_synced_at", "type": "TIMESTAMP", "mode": "NULLABLE"},
    {"name": "is_deleted_in_moment", "type": "BOOLEAN", "mode": "NULLABLE"},
    {"name": "deleted_detected_at", "type": "TIMESTAMP", "mode": "NULLABLE"},
]


def _audit(item: Dict[str, Any]) -> Dict[str, Any]:
    return {
        "version": item.get("version"),
        "created_date": item.get("createdDate"),
        "last_modified_date": item.get("lastModifiedDate"),
        "created_by_system_user": item.get("createdBySystemUser"),
        "created_by_system_user_full_name": item.get("createdBySystemUserFullName"),
        "last_modified_by_system_user": item.get("lastModifiedBySystemUser"),
        "last_modified_by_system_user_full_name": item.get("lastModifiedBySystemUserFullName"),
    }


def _raw_json(item: Dict[str, Any], exclude: Optional[List[str]] = None) -> str:
    if exclude:
        item = {k: v for k, v in item.items() if k not in exclude}
    return json.dumps(item, default=str, ensure_ascii=False)


# ===== Schemas =====
# Field names/types were captured from live API responses; see module docstring.

PROJECTS_SCHEMA = [
    {"name": "id", "type": "INTEGER", "mode": "REQUIRED"},
    {"name": "name", "type": "STRING", "mode": "NULLABLE"},
    {"name": "project_number", "type": "STRING", "mode": "NULLABLE"},
    {"name": "project_state", "type": "STRING", "mode": "NULLABLE"},
    {"name": "is_template", "type": "BOOLEAN", "mode": "NULLABLE"},
    {"name": "customer_id", "type": "INTEGER", "mode": "NULLABLE"},
    {"name": "customer_number", "type": "STRING", "mode": "NULLABLE"},
    {"name": "customer_name", "type": "STRING", "mode": "NULLABLE"},
    {"name": "department_id", "type": "INTEGER", "mode": "NULLABLE"},
    {"name": "department_name", "type": "STRING", "mode": "NULLABLE"},
    {"name": "currency_code", "type": "STRING", "mode": "NULLABLE"},
    {"name": "startup_date", "type": "DATE", "mode": "NULLABLE"},
    {"name": "end_date", "type": "DATE", "mode": "NULLABLE"},
    {"name": "comment", "type": "STRING", "mode": "NULLABLE"},
    {"name": "project_size", "type": "STRING", "mode": "NULLABLE"},
    {"name": "parent_project_id", "type": "INTEGER", "mode": "NULLABLE"},
    {"name": "expected_profit", "type": "FLOAT", "mode": "NULLABLE"},
    {"name": "expected_profit_percentage", "type": "FLOAT", "mode": "NULLABLE"},
    {"name": "profit", "type": "FLOAT", "mode": "NULLABLE"},
    {"name": "work_in_progress_gross", "type": "FLOAT", "mode": "NULLABLE"},
    {"name": "work_in_progress_net", "type": "FLOAT", "mode": "NULLABLE"},
    {"name": "task_required", "type": "BOOLEAN", "mode": "NULLABLE"},
    {"name": "transfer_state", "type": "STRING", "mode": "NULLABLE"},
    *AUDIT_FIELDS,
    *TRAILER_FIELDS,
]

CUSTOMERS_SCHEMA = [
    {"name": "id", "type": "INTEGER", "mode": "REQUIRED"},
    {"name": "name", "type": "STRING", "mode": "NULLABLE"},
    {"name": "customer_number", "type": "STRING", "mode": "NULLABLE"},
    {"name": "organization_number", "type": "STRING", "mode": "NULLABLE"},
    {"name": "vat_identification_number", "type": "STRING", "mode": "NULLABLE"},
    {"name": "country_code", "type": "STRING", "mode": "NULLABLE"},
    {"name": "customer_state", "type": "STRING", "mode": "NULLABLE"},
    {"name": "is_ourself", "type": "BOOLEAN", "mode": "NULLABLE"},
    {"name": "is_supplier", "type": "BOOLEAN", "mode": "NULLABLE"},
    {"name": "is_customer", "type": "BOOLEAN", "mode": "NULLABLE"},
    {"name": "currency_code", "type": "STRING", "mode": "NULLABLE"},
    {"name": "phone_number", "type": "STRING", "mode": "NULLABLE"},
    {"name": "email", "type": "STRING", "mode": "NULLABLE"},
    {"name": "invoice_email", "type": "STRING", "mode": "NULLABLE"},
    {"name": "website", "type": "STRING", "mode": "NULLABLE"},
    {"name": "customer_classification", "type": "STRING", "mode": "NULLABLE"},
    {"name": "rating", "type": "FLOAT", "mode": "NULLABLE"},
    *AUDIT_FIELDS,
    *TRAILER_FIELDS,
]

USER_ACCOUNTS_SCHEMA = [
    {"name": "id", "type": "INTEGER", "mode": "REQUIRED"},
    {"name": "first_name", "type": "STRING", "mode": "NULLABLE"},
    {"name": "middle_name", "type": "STRING", "mode": "NULLABLE"},
    {"name": "last_name", "type": "STRING", "mode": "NULLABLE"},
    {"name": "full_name", "type": "STRING", "mode": "NULLABLE"},
    {"name": "username", "type": "STRING", "mode": "NULLABLE"},
    {"name": "email", "type": "STRING", "mode": "NULLABLE"},
    {"name": "enabled", "type": "BOOLEAN", "mode": "NULLABLE"},
    {"name": "currently_hired", "type": "BOOLEAN", "mode": "NULLABLE"},
    {"name": "department_id", "type": "INTEGER", "mode": "NULLABLE"},
    {"name": "department_name", "type": "STRING", "mode": "NULLABLE"},
    {"name": "access_role_id", "type": "INTEGER", "mode": "NULLABLE"},
    {"name": "access_role_name", "type": "STRING", "mode": "NULLABLE"},
    {"name": "employed_date", "type": "DATE", "mode": "NULLABLE"},
    {"name": "quit_date", "type": "DATE", "mode": "NULLABLE"},
    {"name": "employment_type", "type": "STRING", "mode": "NULLABLE"},
    {"name": "system_user_id", "type": "INTEGER", "mode": "NULLABLE"},
    {"name": "manager_id", "type": "INTEGER", "mode": "NULLABLE"},
    {"name": "version", "type": "STRING", "mode": "NULLABLE"},
    {"name": "created_date", "type": "TIMESTAMP", "mode": "NULLABLE"},
    {"name": "last_modified_date", "type": "TIMESTAMP", "mode": "NULLABLE"},
    *TRAILER_FIELDS,
]

TIME_RECORDS_SCHEMA = [
    {"name": "id", "type": "INTEGER", "mode": "REQUIRED"},
    {"name": "minutes", "type": "INTEGER", "mode": "NULLABLE"},
    {"name": "billable_minutes", "type": "INTEGER", "mode": "NULLABLE"},
    {"name": "not_billable_minutes", "type": "INTEGER", "mode": "NULLABLE"},
    {"name": "time_record_date", "type": "DATE", "mode": "NULLABLE"},
    {"name": "description", "type": "STRING", "mode": "NULLABLE"},
    {"name": "user_account_id", "type": "INTEGER", "mode": "NULLABLE"},
    {"name": "user_account_full_name", "type": "STRING", "mode": "NULLABLE"},
    {"name": "project_id", "type": "INTEGER", "mode": "NULLABLE"},
    {"name": "project_name", "type": "STRING", "mode": "NULLABLE"},
    {"name": "project_number", "type": "STRING", "mode": "NULLABLE"},
    {"name": "task_id", "type": "INTEGER", "mode": "NULLABLE"},
    {"name": "task_name", "type": "STRING", "mode": "NULLABLE"},
    {"name": "invoiced_state", "type": "STRING", "mode": "NULLABLE"},
    {"name": "sale_price_per_hour", "type": "FLOAT", "mode": "NULLABLE"},
    {"name": "sale_price_total", "type": "FLOAT", "mode": "NULLABLE"},
    {"name": "sale_price_currency_code", "type": "STRING", "mode": "NULLABLE"},
    {"name": "version", "type": "STRING", "mode": "NULLABLE"},
    {"name": "created_date", "type": "TIMESTAMP", "mode": "NULLABLE"},
    {"name": "last_modified_date", "type": "TIMESTAMP", "mode": "NULLABLE"},
    *TRAILER_FIELDS,
]

TAGS_SCHEMA = [
    {"name": "id", "type": "INTEGER", "mode": "REQUIRED"},
    {"name": "tag_group_id", "type": "INTEGER", "mode": "NULLABLE"},
    {"name": "tag_group_name", "type": "STRING", "mode": "NULLABLE"},
    {"name": "name", "type": "STRING", "mode": "NULLABLE"},
    {"name": "description", "type": "STRING", "mode": "NULLABLE"},
    {"name": "color", "type": "STRING", "mode": "NULLABLE"},
    {"name": "icon", "type": "STRING", "mode": "NULLABLE"},
    {"name": "code", "type": "STRING", "mode": "NULLABLE"},
    {"name": "sort_order", "type": "INTEGER", "mode": "NULLABLE"},
    {"name": "enabled", "type": "BOOLEAN", "mode": "NULLABLE"},
    {"name": "shared", "type": "BOOLEAN", "mode": "NULLABLE"},
    {"name": "show_in_project_overview", "type": "BOOLEAN", "mode": "NULLABLE"},
    {"name": "tag_type", "type": "STRING", "mode": "NULLABLE"},
    {"name": "hourly_rate", "type": "FLOAT", "mode": "NULLABLE"},
    {"name": "hourly_rate_percentage", "type": "FLOAT", "mode": "NULLABLE"},
    *AUDIT_FIELDS,
    *TRAILER_FIELDS,
]

# Invoices embed the FULL project and customer objects inline (100+ / 60+ keys
# each) - that's excluded from raw_json below to avoid massive duplication;
# projects/customers are synced as their own tables and joined by id.
INVOICES_SCHEMA = [
    {"name": "id", "type": "INTEGER", "mode": "REQUIRED"},
    {"name": "invoice_number", "type": "INTEGER", "mode": "NULLABLE"},
    {"name": "invoice_type", "type": "STRING", "mode": "NULLABLE"},
    {"name": "invoice_state", "type": "STRING", "mode": "NULLABLE"},
    {"name": "issue_date", "type": "DATE", "mode": "NULLABLE"},
    {"name": "due_date", "type": "DATE", "mode": "NULLABLE"},
    {"name": "delivery_date", "type": "DATE", "mode": "NULLABLE"},
    {"name": "currency_code", "type": "STRING", "mode": "NULLABLE"},
    {"name": "company_currency_code", "type": "STRING", "mode": "NULLABLE"},
    {"name": "project_id", "type": "INTEGER", "mode": "NULLABLE"},
    {"name": "project_name", "type": "STRING", "mode": "NULLABLE"},
    {"name": "project_number", "type": "STRING", "mode": "NULLABLE"},
    {"name": "customer_id", "type": "INTEGER", "mode": "NULLABLE"},
    {"name": "customer_number", "type": "STRING", "mode": "NULLABLE"},
    {"name": "department_id", "type": "INTEGER", "mode": "NULLABLE"},
    {"name": "department_name", "type": "STRING", "mode": "NULLABLE"},
    {"name": "outstanding_amount", "type": "FLOAT", "mode": "NULLABLE"},
    {"name": "total_sum_excluding_tax", "type": "FLOAT", "mode": "NULLABLE"},
    {"name": "total_sum_excluding_tax_in_company_currency", "type": "FLOAT", "mode": "NULLABLE"},
    {"name": "total_sum_including_tax_after_rounding", "type": "FLOAT", "mode": "NULLABLE"},
    {"name": "total_sum_including_tax_after_rounding_in_company_currency", "type": "FLOAT", "mode": "NULLABLE"},
    {"name": "paid_amount", "type": "FLOAT", "mode": "NULLABLE"},
    {"name": "sending_state", "type": "STRING", "mode": "NULLABLE"},
    {"name": "transfer_state", "type": "STRING", "mode": "NULLABLE"},
    {"name": "finalized_by_user_account_full_name", "type": "STRING", "mode": "NULLABLE"},
    {"name": "finalized_date", "type": "TIMESTAMP", "mode": "NULLABLE"},
    {"name": "version", "type": "STRING", "mode": "NULLABLE"},
    {"name": "created_date", "type": "TIMESTAMP", "mode": "NULLABLE"},
    {"name": "last_modified_date", "type": "TIMESTAMP", "mode": "NULLABLE"},
    *TRAILER_FIELDS,
]

INVOICE_LINES_SCHEMA = [
    {"name": "id", "type": "INTEGER", "mode": "REQUIRED"},
    {"name": "invoice_id", "type": "INTEGER", "mode": "NULLABLE"},
    {"name": "invoice_number", "type": "INTEGER", "mode": "NULLABLE"},
    {"name": "line_number", "type": "INTEGER", "mode": "NULLABLE"},
    {"name": "item_name", "type": "STRING", "mode": "NULLABLE"},
    {"name": "quantity", "type": "FLOAT", "mode": "NULLABLE"},
    {"name": "quantity_unit", "type": "STRING", "mode": "NULLABLE"},
    {"name": "unit_price", "type": "FLOAT", "mode": "NULLABLE"},
    {"name": "discount_percentage", "type": "FLOAT", "mode": "NULLABLE"},
    {"name": "surcharge_percentage", "type": "FLOAT", "mode": "NULLABLE"},
    {"name": "product_code", "type": "STRING", "mode": "NULLABLE"},
    {"name": "product_name", "type": "STRING", "mode": "NULLABLE"},
    {"name": "financial_account_number", "type": "STRING", "mode": "NULLABLE"},
    {"name": "financial_account_name", "type": "STRING", "mode": "NULLABLE"},
    {"name": "invoice_line_type", "type": "STRING", "mode": "NULLABLE"},
    {"name": "is_text_line", "type": "BOOLEAN", "mode": "NULLABLE"},
    {"name": "sum_net_amount", "type": "FLOAT", "mode": "NULLABLE"},
    {"name": "sum_tax", "type": "FLOAT", "mode": "NULLABLE"},
    {"name": "vat_category_code", "type": "STRING", "mode": "NULLABLE"},
    {"name": "vat_percentage", "type": "FLOAT", "mode": "NULLABLE"},
    {"name": "period_start", "type": "DATE", "mode": "NULLABLE"},
    {"name": "period_end", "type": "DATE", "mode": "NULLABLE"},
    *TRAILER_FIELDS,
]

# NOTE: this tenant (aquatiqas) currently has zero offers, so these field
# names could not be confirmed against live data (only id/audit fields follow
# the pattern seen on every other entity). raw_json is the safety net here -
# revalidate/extend this schema once a tenant with real offers is available.
OFFERS_SCHEMA = [
    {"name": "id", "type": "INTEGER", "mode": "REQUIRED"},
    *AUDIT_FIELDS,
    *TRAILER_FIELDS,
]

INVOICING_CURRENCY_SETUPS_SCHEMA = [
    {"name": "id", "type": "INTEGER", "mode": "REQUIRED"},
    {"name": "currency_code", "type": "STRING", "mode": "NULLABLE"},
    {"name": "country_code", "type": "STRING", "mode": "NULLABLE"},
    {"name": "bank_account_number", "type": "STRING", "mode": "NULLABLE"},
    {"name": "bank_name", "type": "STRING", "mode": "NULLABLE"},
    {"name": "bic_code", "type": "STRING", "mode": "NULLABLE"},
    {"name": "iban_code", "type": "STRING", "mode": "NULLABLE"},
    {"name": "sort_code", "type": "STRING", "mode": "NULLABLE"},
    {"name": "bank_account_type", "type": "STRING", "mode": "NULLABLE"},
    {"name": "allow_ocr", "type": "BOOLEAN", "mode": "NULLABLE"},
    *AUDIT_FIELDS,
    *TRAILER_FIELDS,
]

# projectsWithThisRole on the API is a preview capped at 3 items regardless of
# the true count - intentionally not surfaced as a column; use
# projects_with_this_role_count (the true total) instead.
PROJECT_MEMBERSHIP_ROLES_SCHEMA = [
    {"name": "id", "type": "INTEGER", "mode": "REQUIRED"},
    {"name": "name", "type": "STRING", "mode": "NULLABLE"},
    {"name": "description", "type": "STRING", "mode": "NULLABLE"},
    {"name": "permission", "type": "STRING", "mode": "NULLABLE"},
    {"name": "is_project_owner", "type": "BOOLEAN", "mode": "NULLABLE"},
    {"name": "project_membership_role_state", "type": "STRING", "mode": "NULLABLE"},
    {"name": "access_role_id", "type": "INTEGER", "mode": "NULLABLE"},
    {"name": "access_role_name", "type": "STRING", "mode": "NULLABLE"},
    {"name": "sort_order", "type": "INTEGER", "mode": "NULLABLE"},
    {"name": "projects_with_this_role_count", "type": "INTEGER", "mode": "NULLABLE"},
    {"name": "created_date", "type": "TIMESTAMP", "mode": "NULLABLE"},
    {"name": "last_modified_date", "type": "TIMESTAMP", "mode": "NULLABLE"},
    *TRAILER_FIELDS,
]

EXPENSE_CATEGORIES_SCHEMA = [
    {"name": "id", "type": "INTEGER", "mode": "REQUIRED"},
    {"name": "name", "type": "STRING", "mode": "NULLABLE"},
    {"name": "display_in_report", "type": "BOOLEAN", "mode": "NULLABLE"},
    *AUDIT_FIELDS,
    *TRAILER_FIELDS,
]

# This tenant has zero custom fields configured for any entity, so the item
# shape is unconfirmed beyond id - raw_json is the safety net here too.
CUSTOM_FIELDS_SCHEMA = [
    {"name": "id", "type": "INTEGER", "mode": "REQUIRED"},
    *TRAILER_FIELDS,
]

# departments.invoicingPreferences is a ~80-field nested object that was
# entirely null (aside from its own id) in every sampled record - kept in
# raw_json only, not flattened into columns.
DEPARTMENTS_SCHEMA = [
    {"name": "id", "type": "INTEGER", "mode": "REQUIRED"},
    {"name": "department_number", "type": "STRING", "mode": "NULLABLE"},
    {"name": "name", "type": "STRING", "mode": "NULLABLE"},
    {"name": "external_department_number", "type": "STRING", "mode": "NULLABLE"},
    {"name": "comment", "type": "STRING", "mode": "NULLABLE"},
    {"name": "enabled", "type": "BOOLEAN", "mode": "NULLABLE"},
    *AUDIT_FIELDS,
    *TRAILER_FIELDS,
]

ABSENCES_SCHEMA = [
    {"name": "id", "type": "INTEGER", "mode": "REQUIRED"},
    {"name": "user_account_id", "type": "INTEGER", "mode": "NULLABLE"},
    {"name": "absence_date", "type": "DATE", "mode": "NULLABLE"},
    {"name": "absence_type_id", "type": "INTEGER", "mode": "NULLABLE"},
    {"name": "absence_type_name", "type": "STRING", "mode": "NULLABLE"},
    {"name": "absence_type_category", "type": "STRING", "mode": "NULLABLE"},
    {"name": "absence_minutes", "type": "INTEGER", "mode": "NULLABLE"},
    {"name": "description", "type": "STRING", "mode": "NULLABLE"},
    {"name": "absence_state", "type": "STRING", "mode": "NULLABLE"},
    {"name": "returned_comment", "type": "STRING", "mode": "NULLABLE"},
    {"name": "absence_percentage", "type": "FLOAT", "mode": "NULLABLE"},
    {"name": "last_assessed_by_user_account_id", "type": "INTEGER", "mode": "NULLABLE"},
    {"name": "last_assessed_by_user_account_full_name", "type": "STRING", "mode": "NULLABLE"},
    {"name": "created_date", "type": "TIMESTAMP", "mode": "NULLABLE"},
    {"name": "last_modified_date", "type": "TIMESTAMP", "mode": "NULLABLE"},
    *TRAILER_FIELDS,
]


class BigQuerySync:
    """Transforms Moment API entities and loads them into BigQuery."""

    def __init__(self, gcp_client):
        self.gcp_client = gcp_client
        self.logger = logging.getLogger(__name__)

    def _load(self, table_name: str, schema, rows: List[Dict[str, Any]]) -> Dict[str, Any]:
        """
        Merge a full entity snapshot into its BigQuery table (upsert + soft
        delete, see GCPClient.merge_rows). Never truncates and never removes
        rows, so reruns can't duplicate data and history is preserved.
        """
        if not rows:
            # Ensure the table exists even with zero rows, so downstream
            # consumers see a real empty table rather than a missing one.
            # Deliberately NO merge here: merging an empty snapshot would
            # soft-delete every existing row, and an entity that suddenly
            # returns nothing is far more likely an API hiccup than a genuine
            # total wipe - that case needs a human, not automation.
            try:
                self.gcp_client.ensure_table(table_name, schema)
                existing = self.gcp_client.query_bigquery(
                    f"SELECT COUNT(*) AS n FROM `{self.gcp_client.project_id}.{self.gcp_client.dataset_id}.{table_name}`"
                )[0].n
                if existing:
                    self.logger.warning(
                        f"{table_name}: Moment returned 0 rows but the table has {existing} - "
                        "skipping merge so nothing gets flagged deleted; investigate if unexpected"
                    )
            except Exception as e:
                self.logger.warning(f"Could not ensure empty table {table_name}: {e}")
            return {"success": True, "table": table_name, "records_synced": 0, "message": "No records to sync"}

        # Dedupe on id: pagination happens while Moment is live, so a record
        # can shift pages mid-fetch and appear twice; MERGE requires at most
        # one source row per target row. Last occurrence wins.
        deduped = list({r["id"]: r for r in rows if r.get("id") is not None}.values())
        dropped = len(rows) - len(deduped)
        if dropped:
            self.logger.info(f"{table_name}: dropped {dropped} duplicate/id-less rows before merge")

        try:
            result = self.gcp_client.merge_rows(table_name, deduped, schema)
            return {"success": True, "table": table_name, "records_synced": len(deduped), **result}
        except Exception as e:
            self.logger.error(f"Failed to merge {table_name}: {e}")
            return {"success": False, "table": table_name, "error": str(e)}

    # ===== Projects =====

    def sync_projects(self, projects: List[Dict[str, Any]]) -> Dict[str, Any]:
        synced_at = datetime.now(timezone.utc).isoformat()
        rows = []
        for p in projects:
            try:
                rows.append({
                    "id": p.get("id"),
                    "name": p.get("name"),
                    "project_number": p.get("projectNumber"),
                    "project_state": p.get("projectState"),
                    "is_template": p.get("isTemplate"),
                    "customer_id": p.get("customerId"),
                    "customer_number": p.get("customerNumber"),
                    "customer_name": p.get("customerName"),
                    "department_id": p.get("departmentId"),
                    "department_name": p.get("departmentName"),
                    "currency_code": p.get("currencyCode"),
                    "startup_date": p.get("startupDate"),
                    "end_date": p.get("endDate"),
                    "comment": p.get("comment"),
                    "project_size": p.get("projectSize"),
                    "parent_project_id": p.get("parentProjectId"),
                    "expected_profit": p.get("expectedProfit"),
                    "expected_profit_percentage": p.get("expectedProfitPercentage"),
                    "profit": p.get("profit"),
                    "work_in_progress_gross": p.get("workInProgressGross"),
                    "work_in_progress_net": p.get("workInProgressNet"),
                    "task_required": p.get("taskRequired"),
                    "transfer_state": p.get("transferState"),
                    **_audit(p),
                    "raw_json": _raw_json(p),
                    "synced_at": synced_at,
                })
            except Exception as e:
                self.logger.warning(f"Failed to transform project {p.get('id')}: {e}")
        return self._load("moment_projects", PROJECTS_SCHEMA, rows)

    # ===== Customers =====

    def sync_customers(self, customers: List[Dict[str, Any]]) -> Dict[str, Any]:
        synced_at = datetime.now(timezone.utc).isoformat()
        rows = []
        for c in customers:
            try:
                rows.append({
                    "id": c.get("id"),
                    "name": c.get("name"),
                    "customer_number": c.get("customerNumber"),
                    "organization_number": c.get("organizationNumber"),
                    "vat_identification_number": c.get("vatIdentificationNumber"),
                    "country_code": c.get("countryCode"),
                    "customer_state": c.get("customerState"),
                    "is_ourself": c.get("isOurself"),
                    "is_supplier": c.get("isSupplier"),
                    "is_customer": c.get("isCustomer"),
                    "currency_code": c.get("currencyCode"),
                    "phone_number": c.get("phoneNumber"),
                    "email": c.get("email"),
                    "invoice_email": c.get("invoiceEmail"),
                    "website": c.get("website"),
                    "customer_classification": c.get("customerClassification"),
                    "rating": c.get("rating"),
                    **_audit(c),
                    "raw_json": _raw_json(c),
                    "synced_at": synced_at,
                })
            except Exception as e:
                self.logger.warning(f"Failed to transform customer {c.get('id')}: {e}")
        return self._load("moment_customers", CUSTOMERS_SCHEMA, rows)

    # ===== User Accounts =====

    def sync_user_accounts(self, users: List[Dict[str, Any]]) -> Dict[str, Any]:
        synced_at = datetime.now(timezone.utc).isoformat()
        rows = []
        for u in users:
            try:
                rows.append({
                    "id": u.get("id"),
                    "first_name": u.get("firstName"),
                    "middle_name": u.get("middleName"),
                    "last_name": u.get("lastName"),
                    "full_name": u.get("fullName") or u.get("userAccountName"),
                    "username": u.get("username"),
                    "email": u.get("email"),
                    "enabled": u.get("enabled"),
                    "currently_hired": u.get("currentlyHired"),
                    "department_id": u.get("departmentId"),
                    "department_name": u.get("departmentName"),
                    "access_role_id": u.get("accessRoleId"),
                    "access_role_name": u.get("accessRoleName"),
                    "employed_date": u.get("employedDate"),
                    "quit_date": u.get("quitDate"),
                    "employment_type": u.get("employmentType"),
                    "system_user_id": u.get("systemUserId"),
                    "manager_id": u.get("managerId"),
                    "version": u.get("version"),
                    "created_date": u.get("createdDate"),
                    "last_modified_date": u.get("lastModifiedDate"),
                    "raw_json": _raw_json(u),
                    "synced_at": synced_at,
                })
            except Exception as e:
                self.logger.warning(f"Failed to transform user account {u.get('id')}: {e}")
        return self._load("moment_user_accounts", USER_ACCOUNTS_SCHEMA, rows)

    # ===== Time Records =====

    def sync_time_records(self, records: List[Dict[str, Any]]) -> Dict[str, Any]:
        synced_at = datetime.now(timezone.utc).isoformat()
        rows = []
        for r in records:
            try:
                rows.append({
                    "id": r.get("id"),
                    "minutes": r.get("minutes"),
                    "billable_minutes": r.get("billableMinutes"),
                    "not_billable_minutes": r.get("notBillableMinutes"),
                    "time_record_date": r.get("timeRecordDate"),
                    "description": r.get("description"),
                    "user_account_id": r.get("userAccountId"),
                    "user_account_full_name": r.get("userAccountFullName"),
                    "project_id": r.get("projectId"),
                    "project_name": r.get("projectName"),
                    "project_number": r.get("projectNumber"),
                    "task_id": r.get("taskId"),
                    "task_name": r.get("taskName"),
                    "invoiced_state": r.get("invoicedState"),
                    "sale_price_per_hour": r.get("salePricePerHour"),
                    "sale_price_total": r.get("salePriceTotal"),
                    "sale_price_currency_code": r.get("salePriceCurrencyCode"),
                    "version": r.get("version"),
                    "created_date": r.get("createdDate"),
                    "last_modified_date": r.get("lastModifiedDate"),
                    "raw_json": _raw_json(r),
                    "synced_at": synced_at,
                })
            except Exception as e:
                self.logger.warning(f"Failed to transform time record {r.get('id')}: {e}")
        return self._load("moment_time_records", TIME_RECORDS_SCHEMA, rows)

    # ===== Tags =====

    def sync_tags(self, tags: List[Dict[str, Any]]) -> Dict[str, Any]:
        synced_at = datetime.now(timezone.utc).isoformat()
        rows = []
        for t in tags:
            try:
                rows.append({
                    "id": t.get("id"),
                    "tag_group_id": t.get("tagGroupId"),
                    "tag_group_name": t.get("tagGroupName"),
                    "name": t.get("name"),
                    "description": t.get("description"),
                    "color": t.get("color"),
                    "icon": t.get("icon"),
                    "code": t.get("code"),
                    "sort_order": t.get("sortOrder"),
                    "enabled": t.get("enabled"),
                    "shared": t.get("shared"),
                    "show_in_project_overview": t.get("showInProjectOverview"),
                    "tag_type": t.get("tagType"),
                    "hourly_rate": t.get("hourlyRate"),
                    "hourly_rate_percentage": t.get("hourlyRatePercentage"),
                    **_audit(t),
                    "raw_json": _raw_json(t),
                    "synced_at": synced_at,
                })
            except Exception as e:
                self.logger.warning(f"Failed to transform tag {t.get('id')}: {e}")
        return self._load("moment_tags", TAGS_SCHEMA, rows)

    # ===== Invoices (+ invoice lines child table) =====

    def sync_invoices(self, invoices: List[Dict[str, Any]]) -> Dict[str, Any]:
        synced_at = datetime.now(timezone.utc).isoformat()
        rows = []
        line_rows = []
        for inv in invoices:
            try:
                rows.append({
                    "id": inv.get("id"),
                    "invoice_number": inv.get("invoiceNumber"),
                    "invoice_type": inv.get("invoiceType"),
                    "invoice_state": inv.get("invoiceState"),
                    "issue_date": inv.get("issueDate"),
                    "due_date": inv.get("dueDate"),
                    "delivery_date": inv.get("deliveryDate"),
                    "currency_code": inv.get("currencyCode"),
                    "company_currency_code": inv.get("companyCurrencyCode"),
                    "project_id": inv.get("projectId"),
                    "project_name": inv.get("projectName"),
                    "project_number": inv.get("projectNumber"),
                    "customer_id": inv.get("customerId"),
                    "customer_number": inv.get("customerNumber"),
                    "department_id": inv.get("departmentId"),
                    "department_name": inv.get("departmentName"),
                    "outstanding_amount": inv.get("outstandingAmount"),
                    "total_sum_excluding_tax": inv.get("totalSumExcludingTax"),
                    "total_sum_excluding_tax_in_company_currency": inv.get("totalSumExcludingTaxInCompanyCurrency"),
                    "total_sum_including_tax_after_rounding": inv.get("totalSumIncludingTaxAfterRounding"),
                    "total_sum_including_tax_after_rounding_in_company_currency": inv.get(
                        "totalSumIncludingTaxAfterRoundingInCompanyCurrency"
                    ),
                    "paid_amount": inv.get("paidAmount"),
                    "sending_state": inv.get("sendingState"),
                    "transfer_state": inv.get("transferState"),
                    "finalized_by_user_account_full_name": inv.get("finalizedByUserAccountFullName"),
                    "finalized_date": inv.get("finalizedDate"),
                    "version": inv.get("version"),
                    "created_date": inv.get("createdDate"),
                    "last_modified_date": inv.get("lastModifiedDate"),
                    # project/customer are the full embedded entities (100+/60+
                    # keys) - excluded here since they're synced as their own
                    # tables; join on project_id/customer_id instead.
                    "raw_json": _raw_json(inv, exclude=["project", "customer"]),
                    "synced_at": synced_at,
                })
                for line in inv.get("invoiceLines") or []:
                    line_rows.append({
                        "id": line.get("id"),
                        "invoice_id": inv.get("id"),
                        "invoice_number": line.get("invoiceNumber") or inv.get("invoiceNumber"),
                        "line_number": line.get("lineNumber"),
                        "item_name": line.get("itemName"),
                        "quantity": line.get("quantity"),
                        "quantity_unit": line.get("quantityUnit"),
                        "unit_price": line.get("unitPrice"),
                        "discount_percentage": line.get("discountPercentage"),
                        "surcharge_percentage": line.get("surchargePercentage"),
                        "product_code": line.get("productCode"),
                        "product_name": line.get("productName"),
                        "financial_account_number": line.get("financialAccountNumber"),
                        "financial_account_name": line.get("financialAccountName"),
                        "invoice_line_type": line.get("invoiceLineType"),
                        "is_text_line": line.get("isTextLine"),
                        "sum_net_amount": line.get("sumNetAmount"),
                        "sum_tax": line.get("sumTax"),
                        "vat_category_code": line.get("vatCategoryCode"),
                        "vat_percentage": line.get("vatPercentage"),
                        "period_start": line.get("periodStart"),
                        "period_end": line.get("periodEnd"),
                        "raw_json": _raw_json(line),
                        "synced_at": synced_at,
                    })
            except Exception as e:
                self.logger.warning(f"Failed to transform invoice {inv.get('id')}: {e}")

        result = self._load("moment_invoices", INVOICES_SCHEMA, rows)
        result["invoice_lines"] = self._load("moment_invoice_lines", INVOICE_LINES_SCHEMA, line_rows)
        return result

    # ===== Offers =====

    def sync_offers(self, offers: List[Dict[str, Any]]) -> Dict[str, Any]:
        synced_at = datetime.now(timezone.utc).isoformat()
        rows = []
        for o in offers:
            try:
                rows.append({
                    "id": o.get("id"),
                    **_audit(o),
                    "raw_json": _raw_json(o),
                    "synced_at": synced_at,
                })
            except Exception as e:
                self.logger.warning(f"Failed to transform offer {o.get('id')}: {e}")
        return self._load("moment_offers", OFFERS_SCHEMA, rows)

    # ===== Invoicing Currency Setups =====

    def sync_invoicing_currency_setups(self, setups: List[Dict[str, Any]]) -> Dict[str, Any]:
        synced_at = datetime.now(timezone.utc).isoformat()
        rows = []
        for s in setups:
            try:
                rows.append({
                    "id": s.get("id"),
                    "currency_code": s.get("currencyCode"),
                    "country_code": s.get("countryCode"),
                    "bank_account_number": s.get("bankAccountNumber"),
                    "bank_name": s.get("bankName"),
                    "bic_code": s.get("bicCode"),
                    "iban_code": s.get("ibanCode"),
                    "sort_code": s.get("sortCode"),
                    "bank_account_type": s.get("bankAccountType"),
                    "allow_ocr": s.get("allowOcr"),
                    **_audit(s),
                    "raw_json": _raw_json(s),
                    "synced_at": synced_at,
                })
            except Exception as e:
                self.logger.warning(f"Failed to transform invoicing currency setup {s.get('id')}: {e}")
        return self._load("moment_invoicing_currency_setups", INVOICING_CURRENCY_SETUPS_SCHEMA, rows)

    # ===== Project Membership Roles =====

    def sync_project_membership_roles(self, roles: List[Dict[str, Any]]) -> Dict[str, Any]:
        synced_at = datetime.now(timezone.utc).isoformat()
        rows = []
        for r in roles:
            try:
                rows.append({
                    "id": r.get("id"),
                    "name": r.get("name"),
                    "description": r.get("description"),
                    "permission": r.get("permission"),
                    "is_project_owner": r.get("isProjectOwner"),
                    "project_membership_role_state": r.get("projectMembershipRoleState"),
                    "access_role_id": r.get("accessRoleId"),
                    "access_role_name": r.get("accessRoleName"),
                    "sort_order": r.get("sortOrder"),
                    "projects_with_this_role_count": r.get("projectsWithThisRoleCount"),
                    "created_date": r.get("createdDate"),
                    "last_modified_date": r.get("lastModifiedDate"),
                    "raw_json": _raw_json(r),
                    "synced_at": synced_at,
                })
            except Exception as e:
                self.logger.warning(f"Failed to transform project membership role {r.get('id')}: {e}")
        return self._load("moment_project_membership_roles", PROJECT_MEMBERSHIP_ROLES_SCHEMA, rows)

    # ===== Expense Categories =====

    def sync_expense_categories(self, categories: List[Dict[str, Any]]) -> Dict[str, Any]:
        synced_at = datetime.now(timezone.utc).isoformat()
        rows = []
        for c in categories:
            try:
                rows.append({
                    "id": c.get("id"),
                    "name": c.get("name"),
                    "display_in_report": c.get("displayInReport"),
                    **_audit(c),
                    "raw_json": _raw_json(c),
                    "synced_at": synced_at,
                })
            except Exception as e:
                self.logger.warning(f"Failed to transform expense category {c.get('id')}: {e}")
        return self._load("moment_expense_categories", EXPENSE_CATEGORIES_SCHEMA, rows)

    # ===== Custom Fields =====

    def sync_custom_fields(self, fields: List[Dict[str, Any]]) -> Dict[str, Any]:
        synced_at = datetime.now(timezone.utc).isoformat()
        rows = []
        for f in fields:
            try:
                rows.append({
                    "id": f.get("id"),
                    "raw_json": _raw_json(f),
                    "synced_at": synced_at,
                })
            except Exception as e:
                self.logger.warning(f"Failed to transform custom field {f.get('id')}: {e}")
        return self._load("moment_custom_fields", CUSTOM_FIELDS_SCHEMA, rows)

    # ===== Departments =====

    def sync_departments(self, departments: List[Dict[str, Any]]) -> Dict[str, Any]:
        synced_at = datetime.now(timezone.utc).isoformat()
        rows = []
        for d in departments:
            try:
                rows.append({
                    "id": d.get("id"),
                    "department_number": d.get("departmentNumber"),
                    "name": d.get("name"),
                    "external_department_number": d.get("externalDepartmentNumber"),
                    "comment": d.get("comment"),
                    "enabled": d.get("enabled"),
                    **_audit(d),
                    "raw_json": _raw_json(d),
                    "synced_at": synced_at,
                })
            except Exception as e:
                self.logger.warning(f"Failed to transform department {d.get('id')}: {e}")
        return self._load("moment_departments", DEPARTMENTS_SCHEMA, rows)

    # ===== Absences =====

    def sync_absences(self, absences: List[Dict[str, Any]]) -> Dict[str, Any]:
        synced_at = datetime.now(timezone.utc).isoformat()
        rows = []
        for a in absences:
            try:
                absence_type = a.get("absenceType") or {}
                rows.append({
                    "id": a.get("id"),
                    "user_account_id": a.get("userAccountId"),
                    "absence_date": a.get("absenceDate"),
                    "absence_type_id": a.get("absenceTypeId"),
                    "absence_type_name": absence_type.get("name"),
                    "absence_type_category": absence_type.get("absenceTypeCategory"),
                    "absence_minutes": a.get("absenceMinutes"),
                    "description": a.get("description"),
                    "absence_state": a.get("absenceState"),
                    "returned_comment": a.get("returnedComment"),
                    "absence_percentage": a.get("absencePercentage"),
                    "last_assessed_by_user_account_id": a.get("lastAssessedByUserAccountId"),
                    "last_assessed_by_user_account_full_name": a.get("lastAssessedByUserAccountFullName"),
                    "created_date": a.get("createdDate"),
                    "last_modified_date": a.get("lastModifiedDate"),
                    "raw_json": _raw_json(a),
                    "synced_at": synced_at,
                })
            except Exception as e:
                self.logger.warning(f"Failed to transform absence {a.get('id')}: {e}")
        return self._load("moment_absences", ABSENCES_SCHEMA, rows)

    # ===== Read-back query helper (used by /api/moment/bigquery/data) =====

    TABLE_MAP = {
        "time_records": "moment_time_records",
        "projects": "moment_projects",
        "customers": "moment_customers",
        "user_accounts": "moment_user_accounts",
        "tags": "moment_tags",
        "invoices": "moment_invoices",
        "invoice_lines": "moment_invoice_lines",
        "offers": "moment_offers",
        "invoicing_currency_setups": "moment_invoicing_currency_setups",
        "project_membership_roles": "moment_project_membership_roles",
        "expense_categories": "moment_expense_categories",
        "custom_fields": "moment_custom_fields",
        "departments": "moment_departments",
        "absences": "moment_absences",
    }

    def query_bigquery_data(
        self,
        entity_type: str,
        limit: int = 100,
        filters: Optional[Dict[str, Any]] = None,
    ) -> Dict[str, Any]:
        """Query synced Moment data back out of BigQuery."""
        table_name = self.TABLE_MAP.get(entity_type)
        if not table_name:
            return {"success": False, "error": f"Invalid entity type: {entity_type}"}

        query = f"SELECT * FROM `{self.gcp_client.project_id}.{self.gcp_client.dataset_id}.{table_name}`"

        where_clauses = []
        if filters:
            if filters.get("project_id"):
                where_clauses.append(f"project_id = {int(filters['project_id'])}")
            if filters.get("customer_id"):
                where_clauses.append(f"customer_id = {int(filters['customer_id'])}")
            if filters.get("user_id"):
                where_clauses.append(f"user_account_id = {int(filters['user_id'])}")
            if filters.get("start_date"):
                where_clauses.append(f"time_record_date >= '{filters['start_date']}'")
            if filters.get("end_date"):
                where_clauses.append(f"time_record_date <= '{filters['end_date']}'")
        if where_clauses:
            query += " WHERE " + " AND ".join(where_clauses)
        query += f" ORDER BY synced_at DESC LIMIT {int(limit)}"

        try:
            results = self.gcp_client.query_bigquery(query)
            return {
                "success": True,
                "data": [dict(row) for row in results],
                "count": len(results),
                "query": query,
            }
        except Exception as e:
            self.logger.error(f"Failed to query BigQuery data: {e}")
            return {"success": False, "error": str(e)}
