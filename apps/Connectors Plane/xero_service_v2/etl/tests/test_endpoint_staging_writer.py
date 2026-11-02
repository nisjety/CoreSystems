from unittest.mock import MagicMock

import pytest

from etl.common.endpoint_config import ARRAY_KEYS
from etl.common.fresh_writer import SCHEMAS, EndpointBQWriter


def writer():
    value = object.__new__(EndpointBQWriter)
    value.project = "prj-dw-dev"
    value.dataset = "dw_staging_xero_main"
    value.client = MagicMock()
    return value


def _row(table, **values):
    return {field.name: values.get(field.name) for field in SCHEMAS[table]}


def test_endpoint_layout_provisions_unlabeled_raw_tables_without_version_suffix():
    value = writer()
    value.bootstrap()
    tables = [call.args[0] for call in value.client.create_table.call_args_list]
    names = {table.table_id for table in tables}

    assert set(ARRAY_KEYS) <= names
    assert {"collection_versions", "child_versions", "ingest_receipts", "sync_runs"} <= names
    assert not any(name.startswith("v2_") for name in names)
    assert all(not table.labels for table in tables)
    assert all(table.expires is None for table in tables)


def test_endpoint_layout_publishes_one_receipt_with_records_insert_only():
    value = writer()
    value._write_temp = MagicMock()
    value._drop_temp = MagicMock()
    value.commit = MagicMock()
    batches = {
        "v2_record_versions": [_row("v2_record_versions", endpoint="payments", version_id="v1")],
        "v2_ingest_receipts": [_row("v2_ingest_receipts", receipt_id="abc")],
    }

    value.publish_observation(batches)

    sql = value.commit.call_args.args[0]
    assert "dw_staging_xero_main.payments" in sql
    assert "dw_staging_xero_main.ingest_receipts" in sql
    assert "dw_staging_xero_main.control" in sql
    assert "v2_" not in sql
    assert "DELETE" not in sql and "TRUNCATE" not in sql
    assert value._drop_temp.call_count == 2


def test_endpoint_layout_rejects_unknown_endpoint_before_writing():
    value = writer()
    value._write_temp = MagicMock()
    with pytest.raises(ValueError, match="endpoint"):
        value.publish_observation({
            "v2_record_versions": [_row("v2_record_versions", endpoint="evil; DROP TABLE")],
            "v2_ingest_receipts": [_row("v2_ingest_receipts", receipt_id="abc")],
        })
    value._write_temp.assert_not_called()


def test_endpoint_layout_routes_accounting_get_json_but_not_binary_or_payment_services():
    assert EndpointBQWriter._record_target("accounting_get_getinvoice") == "accounting_get_responses"
    for endpoint in (
        "accounting_get_getinvoiceaspdf",
        "accounting_get_getpaymentservices",
    ):
        with pytest.raises(ValueError, match="ordinary Xero staging"):
            EndpointBQWriter._record_target(endpoint)


def test_endpoint_layout_maps_records_and_metadata_before_publication():
    value = writer()
    value.publish = MagicMock()
    record = _row("v2_record_versions", endpoint="payments", version_id="v1")
    receipt = _row("v2_ingest_receipts", receipt_id="abc")

    value.publish_observation({
        "v2_record_versions": [record],
        "v2_ingest_receipts": [receipt],
    })

    assert value.publish.call_args.args[0] == {
        "payments": [record],
        "ingest_receipts": [receipt],
    }


def test_endpoint_layout_rejects_unapproved_source_table():
    value = writer()
    value.publish = MagicMock()
    with pytest.raises(ValueError, match="unapproved fresh staging target"):
        value.publish_observation({"v2_unapproved": []})
    value.publish.assert_not_called()
