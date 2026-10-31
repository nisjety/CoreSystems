from unittest.mock import MagicMock

import pytest
from google.api_core.exceptions import NotFound

from etl.common.fresh_writer import SCHEMAS, WarehouseRawBQWriter
from etl.xero.run_history import ENTITY_SCHEMA, RUN_SCHEMA


def writer():
    value = object.__new__(WarehouseRawBQWriter)
    value.project = "aquatiq-datavarehus"
    value.dataset = "dw_staging_xero"
    value.client = MagicMock()
    value.client.get_table.side_effect = NotFound("not found")
    return value


def _row(schema, **values):
    return {field.name: values.get(field.name) for field in schema}


def test_raw_history_layout_provisions_natural_tables_without_labels_or_v2_names():
    value = writer()

    value.bootstrap()

    tables = [call.args[0] for call in value.client.create_table.call_args_list]
    names = {table.table_id for table in tables}
    assert {
        "raw_records", "raw_collections", "raw_children", "ingest_receipts",
        "sync_runs", "sync_run_entities", "raw_control", "raw_receipt_locks",
    } <= names
    assert not any(name.startswith("v2_") for name in names)
    assert all(not table.labels for table in tables)
    sync_runs = next(table for table in tables if table.table_id == "sync_runs")
    assert "clustering" not in sync_runs.to_api_repr()


def test_raw_history_layout_maps_internal_batches_to_natural_tables_insert_only():
    value = writer()
    value._write_temp = MagicMock()
    value._drop_temp = MagicMock()
    value.commit = MagicMock()
    batches = {
        "v2_record_versions": [_row(
            SCHEMAS["v2_record_versions"], endpoint="payments", receipt_id="receipt",
        )],
        "v2_collection_versions": [_row(SCHEMAS["v2_collection_versions"], endpoint="payments")],
        "v2_child_versions": [_row(SCHEMAS["v2_child_versions"], endpoint="payments")],
        "v2_ingest_receipts": [_row(SCHEMAS["v2_ingest_receipts"], receipt_id="receipt")],
    }

    value.publish_observation(batches)

    sql = value.commit.call_args.args[0]
    for table in ("raw_records", "raw_collections", "raw_children", "ingest_receipts"):
        assert f"dw_staging_xero.{table}" in sql
    assert "v2_" not in sql
    assert "DELETE" not in sql and "TRUNCATE" not in sql


def test_raw_history_layout_maps_manifest_rows_without_internal_generation_field():
    value = writer()
    value.publish = MagicMock()
    run = _row([*RUN_SCHEMA,], run_id="run") | {"generation": "generation"}
    entity = _row([*ENTITY_SCHEMA,], run_id="run", endpoint="payments") | {"generation": "generation"}
    receipt = _row(SCHEMAS["v2_ingest_receipts"], receipt_id="receipt")

    value.publish_observation({
        "v2_sync_runs": [run],
        "v2_sync_run_entities": [entity],
        "v2_ingest_receipts": [receipt],
    })

    mapped = value.publish.call_args.args[0]
    assert set(mapped) == {"sync_runs", "sync_run_entities", "ingest_receipts"}
    assert set(mapped["sync_runs"][0]) == {field.name for field in RUN_SCHEMA}
    assert set(mapped["sync_run_entities"][0]) == {field.name for field in ENTITY_SCHEMA}


def test_raw_history_manifest_publication_rejects_a_second_receipt_for_the_same_run_id():
    value = writer()

    sql = value.transaction_sql({
        "sync_runs": "incoming_sync_runs",
        "sync_run_entities": "incoming_sync_run_entities",
        "ingest_receipts": "incoming_ingest_receipts",
    })

    assert "manifest run ID already exists" in sql
    assert "incoming_sync_runs" in sql
    assert "dw_staging_xero.sync_runs" in sql
    assert "ASSERT NOT EXISTS" in sql


def test_raw_history_layout_rejects_unapproved_source_table_before_writing():
    value = writer()
    value.publish = MagicMock()

    with pytest.raises(ValueError, match="unapproved fresh staging target"):
        value.publish_observation({"unexpected": []})

    value.publish.assert_not_called()


def test_raw_history_layout_rejects_partial_internal_rows_before_writing():
    value = writer()
    value.publish = MagicMock()
    record = _row(SCHEMAS["v2_record_versions"], endpoint="payments")
    record.pop("payload_json")

    with pytest.raises(ValueError, match="fresh staging schema mismatch"):
        value.publish_observation({"v2_record_versions": [record]})

    value.publish.assert_not_called()


def test_raw_history_layout_batches_mapped_observations_without_v2_targets():
    value = writer()
    value.publish_many = MagicMock()
    first = {
        "v2_record_versions": [_row(SCHEMAS["v2_record_versions"], endpoint="assets_asset_details")],
        "v2_ingest_receipts": [_row(SCHEMAS["v2_ingest_receipts"], receipt_id="one")],
    }
    second = {
        "v2_record_versions": [_row(SCHEMAS["v2_record_versions"], endpoint="assets_asset_details")],
        "v2_ingest_receipts": [_row(SCHEMAS["v2_ingest_receipts"], receipt_id="two")],
    }

    value.publish_observations_many([first, second])

    mapped = value.publish_many.call_args.args[0]
    assert len(mapped) == 2
    assert set(mapped[0]) == {"raw_records", "ingest_receipts"}
    assert set(mapped[1]) == {"raw_records", "ingest_receipts"}
