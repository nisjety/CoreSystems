"""Execute the coverage SELECT offline, with metadata-only SQLite fixtures.

JSON_VALUE/JSON_QUERY and SAFE_CAST are adapted to SQLite in the test harness;
the joins, aggregates, ranking and status CASE run unchanged. CLI calls are mocked.
"""

import json
from collections import Counter
from datetime import datetime
from pathlib import Path
import re
import sqlite3
from unittest.mock import Mock

import pytest

from etl.xero import endpoint_coverage as coverage


ROOT = Path(__file__).resolve().parents[2]
PROJECT, DATASET = "prj-dw-dev", "dw_staging_xero"
STAMP = "2026-10-06T00:00:00Z"
CATALOG_FIELDS = (
    "operation_id", "api_family", "path", "extraction_class", "source_endpoint", "spec_commit",
)
OUTPUT_FIELDS = {
    "generation", "tenant_id", "api_family", "operation_id", "path", "extraction_class",
    "last_observed", "extracted_records", "staging_records", "operation_status",
}
SCHEMAS = {
    "endpoint_catalog": CATALOG_FIELDS,
    "sync_run_entities": (
        "generation", "run_id", "outcome_index", "tenant_id", "endpoint", "request_key", "pages", "records",
        "termination", "error_present", "outcome_class", "http_status", "synced_at",
    ),
    "sync_runs": ("generation", "run_id", "finished_at", "status", "mode"),
    "ingest_receipts": (
        "generation", "run_id", "tenant_id", "endpoint", "receipt_id", "http_status",
        "expected_records", "record_count", "metadata_json", "observed_at",
    ),
}


def entity(endpoint="invoices", pages=1, records=2, termination="ended_on_empty_page", **extra):
    return {
        "generation": "g1", "run_id": "r1", "outcome_index": 0, "tenant_id": "t1",
        "endpoint": endpoint, "request_key": None, "pages": pages, "records": records,
        "termination": termination, "error_present": False, "outcome_class": None,
        "http_status": None, "synced_at": STAMP, **extra,
    }


def run(**extra):
    return {
        "generation": "g1", "run_id": "r1", "finished_at": STAMP,
        "status": "succeeded", "mode": "master", **extra,
    }


def receipt(endpoint="invoices", expected=2, staged=None, page=1, meta=None, **extra):
    metadata = {"x-page": str(page), "x-sync-type": "master", **(meta or {})}
    return {
        "generation": "g1", "run_id": "r1", "tenant_id": "t1", "endpoint": endpoint,
        "receipt_id": f"{endpoint}-{page}", "http_status": 200,
        "expected_records": expected, "record_count": expected if staged is None else staged,
        "metadata_json": json.dumps(metadata), "observed_at": STAMP, **extra,
    }


def direct(operation="getInvoices", **extra):
    return receipt(
        "accounting_get_" + operation.lower(), expected=1,
        meta={"x-sync-type": "accounting-get", "x-operation-id": operation,
              "x-spec-commit": coverage.catalog_rows()[0]["spec_commit"]}, **extra,
    )


def json_value(raw, path):
    try:
        value = json.loads(raw).get(path.removeprefix('$."').removesuffix('"'))
    except (TypeError, ValueError, AttributeError):
        return None
    if isinstance(value, (dict, list)) or value is None:
        return None
    return str(value) if not isinstance(value, bool) else str(value).lower()


def json_query(raw, path):
    try:
        value = json.loads(raw).get(path.removeprefix('$."').removesuffix('"'))
    except (TypeError, ValueError, AttributeError):
        return None
    return None if value is None else json.dumps(value)


@pytest.fixture
def evaluate():
    def execute(entities=(), runs=(), receipts=(), catalog=None):
        db = sqlite3.connect(":memory:")
        db.row_factory = sqlite3.Row
        db.create_function("JSON_VALUE", 2, json_value)
        db.create_function("JSON_QUERY", 2, json_query)
        db.create_function("SAFE_INT", 1, lambda v: int(v) if isinstance(v, str)
                           and re.fullmatch(r"[+-]?[0-9]+", v) else None)
        catalog = coverage.catalog_rows() if catalog is None else catalog
        batches = (catalog, entities, runs, receipts)
        try:
            for (name, fields), rows in zip(SCHEMAS.items(), batches, strict=True):
                numeric = {"outcome_index", "pages", "records", "error_present", "http_status",
                           "expected_records", "record_count"}
                columns = ", ".join(f"{f} {'INTEGER' if f in numeric else 'TEXT'}" for f in fields)
                db.execute(f"CREATE TABLE `{PROJECT}.{DATASET}.{name}` ({columns})")
                placeholders = ",".join("?" for _ in fields)
                db.executemany(
                    f"INSERT INTO `{PROJECT}.{DATASET}.{name}` VALUES ({placeholders})",
                    (tuple(row[f].isoformat().replace("+00:00", "Z")
                           if isinstance(row[f], datetime) else row[f] for f in fields)
                     for row in rows),
                )
            sql = coverage.coverage_query(PROJECT, DATASET)
            sql = sql.replace("UNION DISTINCT", "UNION")
            sql = re.sub(r"SAFE_CAST\((JSON_VALUE\([^)]*\)) AS INT64\)", r"SAFE_INT(\1)", sql)
            sql = sql.replace("CONCAT('accounting_get_', LOWER(c.operation_id))",
                              "('accounting_get_' || LOWER(c.operation_id))")
            db.execute("PRAGMA query_only = ON")
            before = db.total_changes
            result = tuple(dict(row) for row in db.execute(sql))
            assert db.total_changes == before
            return result
        finally:
            db.close()
    return execute


def operation(rows, operation_id="getInvoices", tenant="t1", generation=None):
    matches = [r for r in rows if r["tenant_id"] == tenant and r["operation_id"] == operation_id
               and (generation is None or r["generation"] == generation)]
    assert len(matches) == 1
    return matches[0]


def test_catalog_is_exact_pinned_126_plus_official_24_with_six_string_fields():
    rows = coverage.catalog_rows()
    assert len(rows) == len({r["operation_id"] for r in rows}) == 150
    assert Counter(r["api_family"] for r in rows) == {
        "accounting": 126, "assets": 4, "projects": 7, "bankfeeds": 4, "finance": 8, "identity": 1,
    }
    assert all(set(r) == set(CATALOG_FIELDS) for r in rows)
    assert all(isinstance(v, str) and v for r in rows for v in r.values())
    source = ROOT / "core/crates/xero-client/accounting_get_catalog.json"
    accounting = json.loads(source.read_text())
    official = json.loads((ROOT / "core/crates/xero-mcp/catalog.json").read_text())
    warehouse_families = {"accounting", "assets", "bankfeeds", "projects", "identity", "finance"}
    assert {(r["api_family"], r["operation_id"], r["path"]) for r in rows} == {
        (o["family"], o["operation_id"], o["path"])
        for o in official["operations"]
        if o["family"] in warehouse_families
    }
    assert {r["spec_commit"] for r in rows} == {accounting["spec_commit"]}
    assert [(f.name, f.field_type) for f in coverage.CATALOG_SCHEMA] == [
        (f, "STRING") for f in CATALOG_FIELDS
    ]


def test_catalog_classifies_routes_without_claiming_extraction():
    rows = {r["operation_id"]: r for r in coverage.catalog_rows()}
    expected = {
        "getInvoices": "collection", "getInvoice": "detail", "getInvoiceHistory": "history",
        "getInvoiceAttachments": "attachment_metadata", "getInvoiceAsPdf": "document",
        "getInvoiceAttachmentById": "document", "getReportFromId": "report",
        "getInvoiceReminders": "configuration", "getAssetSettings": "configuration",
        "getConnections": "entitlement", "getFinancialStatementBalanceSheet": "report",
        "getTasks": "collection", "getTask": "detail",
    }
    assert {op: rows[op]["extraction_class"] for op in expected} == expected
    classes = Counter(r["extraction_class"] for r in rows.values()
                      if r["api_family"] == "accounting")
    assert classes == {
        "collection": 28, "configuration": 2, "detail": 32, "history": 16,
        "attachment_metadata": 11, "document": 26, "report": 11,
    }
    assert coverage.OUT_OF_SCOPE_OPERATION_IDS == {
        "getPaymentServices", "getBrandingThemePaymentServices",
    }


def test_aliases_match_existing_endpoints_and_missing_routes_remain_direct():
    rows = {r["operation_id"]: r for r in coverage.catalog_rows()}
    aliases = {
        "getOrganisations": "organisations", "getBankTransactions": "bank_transactions",
        "getPaymentServices": "payment_services", "getInvoices": "invoices",
        "getReportBalanceSheet": "report_balance_sheet",
        "getReportAgedPayablesByContact": "report_aged_payables_by_contact",
        "getAssetById": "assets_asset_details", "getTimeEntries": "projects_time",
        "getTimeEntry": "projects_time_details", "getStatement": "bankfeeds_statement_details",
        "getBankStatementAccounting": "finance_bank_statements_plus",
        "getConnections": "identity_connections",
    }
    assert {op: rows[op]["source_endpoint"] for op in aliases} == aliases
    for op in ("getInvoice", "getContactByContactNumber", "getInvoiceHistory",
               "getInvoiceReminders", "getOrganisationActions", "getReportsList",
               "getReportFromId", "getReportTenNinetyNine"):
        assert rows[op]["source_endpoint"] == "accounting_get_" + op.lower()
    from etl.xero.extended import EXTENDED_ENDPOINTS
    assert {r["source_endpoint"] for r in rows.values() if r["api_family"] != "accounting"} == set(
        EXTENDED_ENDPOINTS
    )


def test_catalog_returns_independent_rows():
    first = coverage.catalog_rows()
    first[0]["path"] = "/tampered"
    assert coverage.catalog_rows()[0]["path"] != "/tampered"


@pytest.mark.parametrize("corruption", ["revision", "count", "duplicate"])
def test_catalog_rejects_revision_drift_count_drift_and_collisions(monkeypatch, corruption):
    source = json.loads(coverage._ACCOUNTING_CATALOG.read_text())
    if corruption == "revision":
        changed = {**source, "spec_commit": "0" * 40}
    elif corruption == "count":
        changed = {**source, "operations": source["operations"][:-1]}
    else:
        changed = {**source, "operations": [source["operations"][1], *source["operations"][1:]]}
    catalog = Mock(read_text=Mock(return_value=json.dumps(changed)))
    monkeypatch.setattr(coverage, "_ACCOUNTING_CATALOG", catalog)
    with pytest.raises(ValueError, match="revision/count|unique"):
        coverage.catalog_rows()


def test_query_is_payload_free_select_in_one_namespace():
    sql = coverage.coverage_query(PROJECT, DATASET)
    assert re.match(r"\s*(WITH|SELECT)\b", sql, re.I)
    assert not re.search(r"\b(CREATE|INSERT|UPDATE|DELETE|MERGE|DROP|ALTER|TRUNCATE)\b", sql, re.I)
    assert ";" not in sql
    assert "payload_json" not in sql and "error_message" not in sql
    assert "CROSS JOIN" in sql and "LEFT JOIN" in sql
    assert 'JSON_VALUE(r.metadata_json, \'$."x-operation-id"\')' in sql
    assert set(re.findall(r"`([^`]+)`", sql)) == {
        f"{PROJECT}.{DATASET}.{name}" for name in SCHEMAS
    }


@pytest.mark.parametrize("project", [None, 1, "", "1project", "bad_project", "p.x", "p`",
                                         "p\n", " p", "p;DROP", "p" * 64])
def test_project_identifiers_are_strict(project):
    with pytest.raises(ValueError, match="project|identifier"):
        coverage.coverage_query(project, DATASET)


@pytest.mark.parametrize("dataset", [None, 1, "", "1data", "data-set", "d.x", "d`", "d\n",
                                         " d", "d;DROP", "d" * 1025])
def test_dataset_identifiers_are_strict(dataset):
    with pytest.raises(ValueError, match="dataset|identifier"):
        coverage.coverage_query(PROJECT, dataset)


def test_no_fresh_tenants_means_no_rows(evaluate):
    assert evaluate() == ()


def test_known_tenants_cross_every_catalog_operation_and_exclude_global(evaluate):
    receipts = tuple(receipt("unknown", tenant_id=t) for t in ("t1", "t2", "run", "global", ""))
    rows = evaluate(receipts=receipts)
    assert len(rows) == 300
    assert {r["tenant_id"] for r in rows} == {"t1", "t2"}
    assert all(set(r) == OUTPUT_FIELDS for r in rows)
    assert all(r["operation_status"] in {"not_attempted", "out_of_scope"} for r in rows)
    assert all(
        r["operation_status"] == "out_of_scope"
        for r in rows
        if r["operation_id"] in {"getPaymentServices", "getBrandingThemePaymentServices"}
    )
    assert all(
        r["operation_status"] == "not_attempted"
        for r in rows
        if r["operation_id"] not in {"getPaymentServices", "getBrandingThemePaymentServices"}
    )
    assert all(r["last_observed"] is None and r["extracted_records"] is None
               and r["staging_records"] is None for r in rows)


def test_payment_service_routes_are_catalogued_but_out_of_warehouse_scope(evaluate):
    rows = evaluate(receipts=(receipt("unknown"),))
    assert {
        operation(rows, operation_id)["operation_status"]
        for operation_id in ("getPaymentServices", "getBrandingThemePaymentServices")
    } == {"out_of_scope"}
    assert len(evaluate(entities=(entity(endpoint="unknown"),))) == 150


def test_excluded_route_evidence_is_a_policy_violation(evaluate):
    rows = evaluate(
        entities=(entity(endpoint="payment_services"),),
        runs=(run(),),
        receipts=(
            receipt(endpoint="payment_services"),
            direct("getBrandingThemePaymentServices"),
        ),
    )
    assert {
        operation(rows, operation_id)["operation_status"]
        for operation_id in ("getPaymentServices", "getBrandingThemePaymentServices")
    } == {"policy_violation"}


@pytest.mark.parametrize("sync_type", ["master", "open-sweep"])
def test_unfiltered_reconciled_collection_is_full(evaluate, sync_type):
    rows = evaluate((entity(),), (run(),), (receipt(meta={"x-sync-type": sync_type}),))
    row = operation(rows)
    assert row["operation_status"] == "full_collection_verified"
    assert (row["extracted_records"], row["staging_records"]) == (2, 2)
    assert operation(rows, "getInvoice")["operation_status"] == "not_attempted"


@pytest.mark.parametrize("key", ["x-where", "x-modified-after", "x-business-from", "x-business-to",
                                   "x-request-key", "x-snapshot-date", "x-filter", "x-status"])
def test_filters_or_scoped_requests_never_certify_full(evaluate, key):
    rows = evaluate((entity(),), (run(),), (receipt(meta={key: "bounded"}),))
    assert operation(rows)["operation_status"] == "incremental_verified"


def test_even_an_empty_filter_tag_prevents_full_certification(evaluate):
    rows = evaluate((entity(),), (run(),), (receipt(meta={"x-where": ""}),))
    assert operation(rows)["operation_status"] == "incremental_verified"


@pytest.mark.parametrize("sync_type", ["incremental", "rolling-full", "backfill"])
def test_windowed_sync_is_incremental_even_if_counts_match(evaluate, sync_type):
    rows = evaluate((entity(),), (run(),), (receipt(meta={"x-sync-type": sync_type}),))
    assert operation(rows)["operation_status"] == "incremental_verified"


def test_unknown_sync_type_is_incomplete(evaluate):
    rows = evaluate((entity(),), (run(),), (receipt(meta={"x-sync-type": "future"}),))
    assert operation(rows)["operation_status"] == "incomplete"


@pytest.mark.parametrize("operation_id", ["getInvoices", "getInvoice", "getInvoiceHistory",
                                            "getInvoiceAttachments", "getInvoiceReminders"])
def test_single_direct_response_never_certifies_a_collection(evaluate, operation_id):
    observation = direct(operation_id)
    outcome = entity(observation["endpoint"], records=1, termination="single_response")
    rows = evaluate((outcome,), (run(mode="accounting-get"),), (observation,))
    assert operation(rows, operation_id)["operation_status"] == "response_verified"


def test_direct_get_even_with_master_tag_and_pagination_end_never_certifies_full(evaluate):
    observation = direct()
    observation = {**observation, "metadata_json": json.dumps({
        **json.loads(observation["metadata_json"]), "x-sync-type": "master",
    })}
    rows = evaluate((entity(observation["endpoint"], records=1),), (run(),), (observation,))
    assert operation(rows)["operation_status"] == "incomplete"


@pytest.mark.parametrize("count", [0, 2])
def test_single_response_manifest_must_describe_exactly_one_response(evaluate, count):
    observation = direct(expected_records=count, record_count=count)
    outcome = entity(observation["endpoint"], records=count, termination="single_response")
    rows = evaluate((outcome,), (run(),), (observation,))
    assert operation(rows)["operation_status"] == "incomplete"


def test_direct_single_response_cannot_be_fabricated_from_two_loaded_pages(evaluate):
    observations = (direct(), direct(page=2))
    rows = evaluate((entity(observations[0]["endpoint"], pages=2, records=2,
                            termination="single_response"),), (run(),), observations)
    assert operation(rows)["operation_status"] == "incomplete"


@pytest.mark.parametrize("bad_meta", [{"x-operation-id": "getInvoice"}, {"x-operation-id": None},
                                       {"x-spec-commit": "wrong"}])
def test_direct_metadata_must_match_operation_and_pinned_spec(evaluate, bad_meta):
    observation = direct()
    observation = {**observation, "metadata_json": json.dumps({
        **json.loads(observation["metadata_json"]), **bad_meta,
    })}
    rows = evaluate((entity(observation["endpoint"], records=1, termination="single_response"),),
                    (run(),), (observation,))
    assert operation(rows)["operation_status"] == "incomplete"
    assert operation(rows, "getInvoice")["operation_status"] == "not_attempted"


def test_document_pointer_is_response_evidence_not_a_missing_financial_row(evaluate):
    observation = direct("getInvoiceAsPdf", staged=0)
    rows = evaluate((entity(observation["endpoint"], records=1, termination="single_response"),),
                    (run(),), (observation,))
    row = operation(rows, "getInvoiceAsPdf")
    assert row["operation_status"] == "response_verified"
    assert (row["extracted_records"], row["staging_records"]) == (1, 0)


def test_empty_success_is_zero_not_not_attempted(evaluate):
    rows = evaluate((entity(records=0),), (run(),), (receipt(expected=0),))
    row = operation(rows)
    assert row["operation_status"] == "full_collection_verified"
    assert row["extracted_records"] == row["staging_records"] == 0
    assert operation(rows, "getAccounts")["extracted_records"] is None


@pytest.mark.parametrize("kind", ["empty_collection", "empty_direct_json", "document_pointer"])
def test_actual_fresh_receipt_contract_for_empty_and_wrapper_responses(evaluate, kind):
    from etl.xero.fresh_staging import build_observation

    generation = "11111111-1111-4111-8111-111111111111"
    run_id = "22222222-2222-4222-8222-222222222222"
    operation_id = "getInvoiceAsPdf" if kind == "document_pointer" else "getInvoices"
    endpoint = ("invoices" if kind == "empty_collection"
                else "accounting_get_" + operation_id.lower())
    meta = {
        "x-source-system": "xero_v2", "x-ingestion-generation": generation,
        "x-tenant-id": "t1", "x-endpoint": endpoint, "x-run-id": run_id,
        "x-synced-at": STAMP, "x-data-generation": "1", "x-http-status": "200",
        "x-record-count": "0" if kind == "empty_collection" else "1", "x-page": "1",
        "x-sync-type": "master" if kind == "empty_collection" else "accounting-get",
        "x-operation-id": operation_id, "x-spec-commit": coverage.catalog_rows()[0]["spec_commit"],
    }
    records = [] if kind == "empty_collection" else [{
        "tenant_id": "t1", "record_id": run_id,
        "payload": None if kind == "document_pointer" else {"Invoices": []},
    }]
    batches = build_observation(meta, records, "raw-bucket", "raw/response.json.meta.json", "1")
    observation, = batches["v2_ingest_receipts"]
    outcome = entity(endpoint, records=int(meta["x-record-count"]), generation=generation,
                     run_id=run_id, termination="ended_on_empty_page" if kind == "empty_collection"
                     else "single_response")
    rows = evaluate((outcome,), (run(generation=generation, run_id=run_id),), (observation,))
    row = operation(rows, operation_id)
    assert row["operation_status"] == (
        "full_collection_verified" if kind == "empty_collection" else "response_verified"
    )
    assert row["extracted_records"] == int(meta["x-record-count"])
    assert row["staging_records"] == (1 if kind == "empty_direct_json" else 0)


@pytest.mark.parametrize("termination", ["error", "hit_max_pages_cap", "partial_detail_failure"])
def test_failed_attempts_visible_without_receipts(evaluate, termination):
    rows = evaluate((entity(pages=0, records=0, termination=termination),), (run(status="failed"),))
    assert operation(rows)["operation_status"] == "failed"


def test_entity_error_fails_even_with_successful_termination(evaluate):
    rows = evaluate((entity(error_present=True),), (run(),), (receipt(),))
    assert operation(rows)["operation_status"] == "failed"


@pytest.mark.parametrize(
    ("operation_id", "outcome_class", "http_status", "expected_status"),
    [
        ("getOnlineInvoice", "not_applicable", 404, "not_applicable"),
        ("getOrganisationCISSettings", "source_unavailable", 404, "source_unavailable"),
        ("getProjects", "access_denied", 403, "access_denied"),
    ],
)
def test_source_availability_gaps_are_visible_without_being_misreported_as_failures(
    evaluate, operation_id, outcome_class, http_status, expected_status,
):
    catalog = {row["operation_id"]: row for row in coverage.catalog_rows()}
    route = catalog[operation_id]
    rows = evaluate(
        (entity(
            route["source_endpoint"],
            pages=0,
            records=0,
            termination="error",
            error_present=True,
            outcome_class=outcome_class,
            http_status=http_status,
        ),),
        (run(),),
    )

    assert operation(rows, operation_id)["operation_status"] == expected_status


@pytest.mark.parametrize(
    ("outcome_class", "http_status", "expected_status"),
    [
        ("source_unavailable", 404, "source_unavailable"),
        ("access_denied", 403, "access_denied"),
    ],
)
def test_partial_fanout_gap_cannot_certify_an_endpoint(
    evaluate, outcome_class, http_status, expected_status,
):
    rows = evaluate(
        (
            entity(),
            entity(
                pages=0,
                records=0,
                termination="error",
                outcome_index=1,
                error_present=True,
                outcome_class=outcome_class,
                http_status=http_status,
            ),
        ),
        (run(status="failed" if outcome_class == "access_denied" else "succeeded"),),
        (receipt(),),
    )

    assert operation(rows)["operation_status"] == expected_status


@pytest.mark.parametrize("termination", [
    "future_termination", "not_modified", "offset_not_advancing",
])
def test_unknown_or_non_fresh_termination_cannot_certify(evaluate, termination):
    rows = evaluate((entity(termination=termination),), (run(),), (receipt(),))
    assert operation(rows)["operation_status"] == "incomplete"


def test_documented_journal_offset_termination_is_healthy(evaluate):
    rows = evaluate((entity("journals", termination="offset_not_advancing"),), (run(),),
                    (receipt("journals"),))
    assert operation(rows, "getJournals")["operation_status"] == "full_collection_verified"


def test_missing_manifest_is_incomplete_not_certified(evaluate):
    assert operation(evaluate(receipts=(receipt(),)))["operation_status"] == "incomplete"
    rows = evaluate((entity(),), receipts=(receipt(),))
    assert operation(rows)["operation_status"] == "incomplete"


def test_successful_manifest_without_loaded_page_is_awaiting_staging(evaluate):
    rows = evaluate((entity(records=0),), (run(),))
    row = operation(rows)
    assert row["operation_status"] == "awaiting_staging"
    assert row["extracted_records"] is None and row["staging_records"] is None


def test_missing_pages_remain_awaiting_even_when_record_totals_match(evaluate):
    rows = evaluate((entity(pages=2),), (run(),), (receipt(),))
    assert operation(rows)["operation_status"] == "awaiting_staging"


@pytest.mark.parametrize("observations", [
    (receipt(page=1), receipt(expected=0, page=1, receipt_id="duplicate")),
    (receipt(page=1), receipt(expected=0, page=3)),
    (receipt(page=0), receipt(expected=0, page=1)),
    (receipt(), receipt(expected=0, meta={"x-page": "bad"})),
])
def test_duplicate_gapped_or_invalid_pages_are_incomplete(evaluate, observations):
    rows = evaluate((entity(pages=2),), (run(),), observations)
    assert operation(rows)["operation_status"] == "incomplete"


def test_all_200_pages_reconcile_while_final_304_is_excluded(evaluate):
    observations = (receipt(), receipt(expected=0, page=2),
                    receipt(expected=99, staged=0, page=3, http_status=304))
    rows = evaluate((entity(pages=2),), (run(),), observations)
    row = operation(rows)
    assert row["operation_status"] == "full_collection_verified"
    assert (row["extracted_records"], row["staging_records"]) == (2, 2)


def test_only_304_is_not_an_empty_success(evaluate):
    rows = evaluate((entity(pages=0, records=0, termination="not_modified"),), (run(),),
                    (receipt(expected=0, http_status=304),))
    row = operation(rows)
    assert row["operation_status"] == "incomplete" and row["extracted_records"] is None


@pytest.mark.parametrize("expected,staged,status", [
    (2, 1, "awaiting_staging"), (2, 3, "incomplete"), (3, 3, "incomplete"),
])
def test_record_mismatches_cannot_certify(evaluate, expected, staged, status):
    rows = evaluate((entity(),), (run(),), (receipt(expected=expected, staged=staged),))
    assert operation(rows)["operation_status"] == status


def test_per_page_mismatches_cannot_cancel_each_other(evaluate):
    rows = evaluate((entity(pages=2, records=4),), (run(),),
                    (receipt(expected=2, staged=1), receipt(expected=2, staged=3, page=2)))
    assert operation(rows)["operation_status"] == "incomplete"


@pytest.mark.parametrize("table,field", [
    ("entity", "error_present"), ("entity", "pages"), ("entity", "records"),
    ("entity", "synced_at"), ("run", "status"), ("run", "finished_at"),
    ("receipt", "record_count"), ("receipt", "expected_records"),
    ("receipt", "metadata_json"), ("receipt", "observed_at"),
    ("receipt", "receipt_id"), ("receipt", "http_status"),
])
def test_nullable_evidence_fails_closed(evaluate, table, field):
    outcomes = (entity(**({field: None} if table == "entity" else {})),)
    runs = (run(**({field: None} if table == "run" else {})),)
    observations = (receipt(**({field: None} if table == "receipt" else {})),)
    rows = evaluate(outcomes, runs, observations)
    assert operation(rows)["operation_status"] == "incomplete"


def test_repeated_fanout_outcomes_are_summed_not_join_multiplied(evaluate):
    outcomes = (entity("projects_tasks", records=1),
                entity("projects_tasks", records=1, outcome_index=1))
    observations = (receipt("projects_tasks", expected=1, meta={"x-request-key": "p1"}),
                    receipt("projects_tasks", expected=1, page=2, meta={"x-request-key": "p2"}))
    row = operation(evaluate(outcomes, (run(),), observations), "getTasks")
    assert row["operation_status"] == "response_verified"
    assert (row["extracted_records"], row["staging_records"]) == (2, 2)


def test_report_without_page_tag_is_verified_as_response(evaluate):
    observation = receipt("report_balance_sheet", expected=1,
                          metadata_json=json.dumps({"x-sync-type": "report-snapshot"}))
    rows = evaluate((entity("report_balance_sheet", records=1, termination="report-snapshot"),),
                    (run(mode="report-snapshot"),), (observation,))
    assert operation(rows, "getReportBalanceSheet")["operation_status"] == "response_verified"


def test_run_endpoint_and_tenant_must_all_match(evaluate):
    outcomes = (entity(), entity(endpoint="unknown", tenant_id="t2"),
                entity(endpoint="unknown", generation="g2"))
    rows = evaluate(outcomes, (run(), run(generation="g2")), (
        receipt(generation="g2"), receipt(run_id="other"), receipt(tenant_id="t2"),
        receipt(endpoint="accounts"),
    ))
    assert len(rows) == 300
    assert operation(rows)["operation_status"] != "full_collection_verified"
    assert operation(rows, tenant="t2")["operation_status"] == "incomplete"
    assert operation(rows)["operation_status"] == "incomplete"


def test_latest_attempt_overrides_older_verified_run_and_does_not_accumulate_counts(evaluate):
    outcomes = (entity(), entity(run_id="r2", synced_at="2026-10-07T00:00:00Z",
                                  termination="error", error_present=True))
    rows = evaluate(outcomes, (run(), run(run_id="r2", finished_at="2026-10-07T00:00:00Z",
                                        status="failed")), (receipt(),))
    row = operation(rows)
    assert row["operation_status"] == "failed"
    assert row["last_observed"] == "2026-10-07T00:00:00Z"
    assert row["extracted_records"] is None


@pytest.mark.parametrize("reverse", [False, True])
def test_timestamp_ties_prefer_failure_without_input_order_dependence(evaluate, reverse):
    outcomes = (entity(), entity(run_id="r2", termination="error", error_present=True))
    rows = evaluate(outcomes[::-1] if reverse else outcomes,
                    (run(), run(run_id="r2", status="failed")), (receipt(),))
    assert operation(rows)["operation_status"] == "failed"


def test_sibling_failure_does_not_fail_a_reconciled_operation(evaluate):
    rows = evaluate((entity(), entity("accounts", termination="error", error_present=True)),
                    (run(status="failed"),), (receipt(),))
    assert operation(rows)["operation_status"] == "full_collection_verified"
    assert operation(rows, "getAccounts")["operation_status"] == "failed"


def test_old_reference_catalog_revision_does_not_duplicate_current_rows(evaluate):
    old = [{**row, "spec_commit": "0" * 40} for row in coverage.catalog_rows()]
    rows = evaluate(receipts=(receipt("unknown"),), catalog=old + coverage.catalog_rows())
    assert len(rows) == 150


@pytest.mark.parametrize("action", ["populate-catalog", "install-view"])
def test_main_keeps_catalog_population_and_view_installation_separate(monkeypatch, action):
    client, writer = Mock(), Mock()
    monkeypatch.setattr(coverage.bigquery, "Client", Mock(return_value=client))
    from etl.common import bq_writer
    monkeypatch.setattr(bq_writer, "BQWriter", Mock(return_value=writer))
    assert coverage.main(["--project", PROJECT, "--dataset", DATASET, action]) == 0
    if action == "populate-catalog":
        table = client.create_table.call_args.args[0]
        assert table.table_id == "endpoint_catalog"
        assert tuple(table.schema) == tuple(coverage.CATALOG_SCHEMA)
        writer.insert_if_absent.assert_called_once_with(
            "endpoint_catalog", coverage.catalog_rows(),
            key_columns=("spec_commit", "api_family", "operation_id"),
            schema=coverage.CATALOG_SCHEMA,
        )
        client.query.assert_not_called()
    else:
        view = client.create_table.call_args.args[0]
        assert view.table_id == "endpoint_coverage"
        assert view.view_query == coverage.coverage_query(PROJECT, DATASET)
        assert view.view_use_legacy_sql is False
        writer.insert_if_absent.assert_not_called()


def test_main_rejects_invalid_identifiers_before_creating_a_client(monkeypatch):
    client = Mock()
    monkeypatch.setattr(coverage.bigquery, "Client", client)
    with pytest.raises(ValueError):
        coverage.main(["--project", "bad`", "--dataset", DATASET, "install-view"])
    client.assert_not_called()


def test_main_does_not_swallow_installation_errors(monkeypatch):
    client = Mock()
    client.create_table.side_effect = RuntimeError("permission denied")
    monkeypatch.setattr(coverage.bigquery, "Client", Mock(return_value=client))
    with pytest.raises(RuntimeError, match="permission denied"):
        coverage.main(["--project", PROJECT, "--dataset", DATASET, "install-view"])


def test_deployed_import_query_and_view_install_need_no_core_catalog(monkeypatch, tmp_path):
    source = Path(coverage.__file__).read_text()
    deployed_path = tmp_path / "etl/xero/endpoint_coverage.py"
    namespace = {"__file__": str(deployed_path), "__name__": "deployed_endpoint_coverage"}
    monkeypatch.setattr(Path, "read_text", Mock(side_effect=AssertionError("runtime catalog read")))
    exec(compile(source, str(deployed_path), "exec"), namespace)
    assert not namespace["_ACCOUNTING_CATALOG"].exists()
    expected_sql = coverage.coverage_query(PROJECT, DATASET)
    assert namespace["coverage_query"](PROJECT, DATASET) == expected_sql
    client = Mock()
    monkeypatch.setattr(coverage.bigquery, "Client", Mock(return_value=client))
    assert namespace["main"](["--project", PROJECT, "--dataset", DATASET, "install-view"]) == 0
    assert client.create_table.call_args.args[0].view_query == expected_sql
