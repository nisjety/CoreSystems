from __future__ import annotations

import json
from pathlib import Path

import pytest

from vsv2_tooling.discovery import generate_queries


@pytest.mark.unit
def test_generate_queries_writes_catalog_and_report(tmp_path: Path) -> None:
    introspection_path = tmp_path / "datamart.introspection.json"
    out_dir = tmp_path / "queries"
    report_path = tmp_path / "report.md"
    endpoint_dir = tmp_path / "endpoints"
    endpoint_dir.mkdir()

    payload = {
        "__schema": {
            "queryType": {"name": "Query"},
            "types": [
                {
                    "kind": "OBJECT",
                    "name": "Query",
                    "fields": [
                        {
                            "name": "accounts",
                            "args": [
                                {
                                    "name": "lastId",
                                    "type": {"kind": "SCALAR", "name": "String"},
                                    "defaultValue": None,
                                },
                                {
                                    "name": "updatedInDataMart",
                                    "type": {"kind": "SCALAR", "name": "DateTime"},
                                    "defaultValue": None,
                                },
                                {
                                    "name": "pageSize",
                                    "type": {
                                        "kind": "NON_NULL",
                                        "ofType": {"kind": "SCALAR", "name": "Int"},
                                    },
                                    "defaultValue": "5000",
                                },
                            ],
                            "type": {
                                "kind": "NON_NULL",
                                "ofType": {
                                    "kind": "LIST",
                                    "ofType": {"kind": "OBJECT", "name": "Account"},
                                },
                            },
                        }
                    ],
                },
                {
                    "kind": "OBJECT",
                    "name": "Account",
                    "fields": [
                        {"name": "id", "type": {"kind": "SCALAR", "name": "String"}},
                        {"name": "companyId", "type": {"kind": "SCALAR", "name": "Int"}},
                        {"name": "accountCd", "type": {"kind": "SCALAR", "name": "String"}},
                        {
                            "name": "branch",
                            "type": {"kind": "OBJECT", "name": "Branch"},
                        },
                    ],
                },
                {
                    "kind": "OBJECT",
                    "name": "Branch",
                    "fields": [
                        {"name": "id", "type": {"kind": "SCALAR", "name": "String"}},
                        {"name": "branchId", "type": {"kind": "SCALAR", "name": "Int"}},
                    ],
                },
            ],
        }
    }
    introspection_path.write_text(json.dumps(payload))
    (endpoint_dir / "accounts.json").write_text(
        json.dumps(
            {
                "endpoint_name": "accounts",
                "bigquery_table": "visma_accounts",
                "primary_key": "accountCD",
                "source": "erp_rest",
                "api_method": "get_accounts",
                "api_endpoint": "/v1/account",
                "pagination_profile": "finance_page_number",
                "modified_field": None,
                "merge_type": "upsert",
                "sync_pattern": "full_refresh_upsert",
                "validation_fields": ["accountCD"],
                "datamart": {
                    "ready": False,
                    "tenant_overrides": {},
                    "type_name": "Account",
                    "query_file": "config/queries/account.graphql",
                    "cursor_field": "updatedInDataMartDateTime",
                    "id_field": "id",
                    "company_filter": False,
                    "fallback_source": "erp_rest",
                },
            }
        )
    )

    generate_queries.run(
        out_dir=str(out_dir),
        report_path=str(report_path),
        introspection_path=str(introspection_path),
        endpoint_dir=str(endpoint_dir),
        sync_configs=False,
    )

    query_text = (out_dir / "accounts.graphql").read_text()
    assert "query Accounts(" in query_text
    assert "$updatedInDataMart: DateTime" in query_text
    assert "$pageSize: Int! = 5000" in query_text
    assert "branch {" in query_text
    assert "branchId" in query_text

    report_text = report_path.read_text()
    assert "DataMart GraphQL Catalog" in report_text
    assert "`accounts`" in report_text


@pytest.mark.unit
def test_generate_queries_syncs_safe_endpoint_config_and_missing_query_file(tmp_path: Path) -> None:
    introspection_path = tmp_path / "datamart.introspection.json"
    query_out_dir = tmp_path / "config" / "queries" / "datamart"
    report_path = tmp_path / "report.md"
    endpoint_dir = tmp_path / "config" / "endpoints"
    endpoint_dir.mkdir(parents=True)

    payload = {
        "__schema": {
            "queryType": {"name": "Query"},
            "types": [
                {
                    "kind": "OBJECT",
                    "name": "Query",
                    "fields": [
                        {
                            "name": "salesOrders",
                            "args": [],
                            "type": {
                                "kind": "NON_NULL",
                                "ofType": {
                                    "kind": "LIST",
                                    "ofType": {"kind": "OBJECT", "name": "SalesOrder"},
                                },
                            },
                        }
                    ],
                },
                {
                    "kind": "OBJECT",
                    "name": "SalesOrder",
                    "fields": [
                        {"name": "id", "type": {"kind": "SCALAR", "name": "String"}},
                        {"name": "companyId", "type": {"kind": "SCALAR", "name": "Int"}},
                    ],
                },
            ],
        }
    }
    introspection_path.write_text(json.dumps(payload))
    config_path = endpoint_dir / "sales_orders_v3.json"
    config_path.write_text(
        json.dumps(
            {
                "endpoint_name": "sales_orders_v3",
                "bigquery_table": "visma_sales_orders_v3",
                "primary_key": "orderId",
                "source": "erp_rest",
                "api_method": "get_sales_orders",
                "api_endpoint": "https://salesorder.visma.net/api/v3/SalesOrders",
                "pagination_profile": "salesorder_page_index",
                "modified_field": "lastModified",
                "merge_type": "upsert",
                "sync_pattern": "incremental_upsert",
                "validation_fields": ["orderId", "lastModified"],
                "datamart": {
                    "ready": False,
                    "query_file": "config/queries/sales_order.graphql",
                    "cursor_field": "updatedInDataMartDateTime",
                    "id_field": "id",
                    "company_filter": True,
                    "fallback_source": "erp_rest",
                },
            }
        )
    )

    generate_queries.run(
        out_dir=str(query_out_dir),
        report_path=str(report_path),
        introspection_path=str(introspection_path),
        endpoint_dir=str(endpoint_dir),
        sync_configs=True,
    )

    synced = json.loads(config_path.read_text())
    assert synced["datamart"]["root_field"] == "salesOrders"
    assert synced["datamart"]["type_name"] == "SalesOrder"
    assert (tmp_path / "config" / "queries" / "sales_order.graphql").exists()


@pytest.mark.unit
def test_gl_balance_query_requests_deleted_rows_by_default() -> None:
    field = generate_queries.QueryField(
        root_name="generalLedgerBalancesV2",
        type_name="GeneralLedgerBalance",
        is_list=True,
        args=(
            {
                "name": "includeDeleted",
                "type": {
                    "kind": "NON_NULL",
                    "ofType": {"kind": "SCALAR", "name": "Boolean"},
                },
                "defaultValue": "false",
            },
        ),
    )
    types = {
        "GeneralLedgerBalance": {
            "kind": "OBJECT",
            "name": "GeneralLedgerBalance",
            "fields": [
                {
                    "name": "deletedDatabaseRecord",
                    "type": {"kind": "SCALAR", "name": "Boolean"},
                },
            ],
        },
    }

    query = generate_queries._render_query(field, types)

    assert "$includeDeleted: Boolean! = true" in query
    assert "includeDeleted: $includeDeleted" in query
    assert "deletedDatabaseRecord" in query
