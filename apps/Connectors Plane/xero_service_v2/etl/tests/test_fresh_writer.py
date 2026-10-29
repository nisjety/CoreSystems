"""Fresh staging commits must never mutate business history."""

import threading
import time
from concurrent.futures import ThreadPoolExecutor
from unittest.mock import MagicMock

import pytest
from google.api_core.exceptions import BadRequest, Forbidden, NotFound
from google.cloud import bigquery

from etl.common.fresh_writer import RECEIPT_LOCK_SHARDS, FreshBQWriter, SCHEMAS


def writer():
    value = object.__new__(FreshBQWriter)
    value.project = "prj-dw-dev"
    value.dataset = "dw_staging_xero"
    value.client = MagicMock()
    value.client.get_table.side_effect = NotFound("not found")
    return value


def test_transaction_uses_the_global_fence_and_selected_receipt_lock():
    value = writer()
    sql = value.transaction_sql({
        "v2_record_versions": "_tmp_fresh_records",
        "v2_ingest_receipts": "_tmp_fresh_receipts",
    })
    assert "BEGIN TRANSACTION" in sql and "COMMIT TRANSACTION" in sql
    assert "UPDATE `prj-dw-dev.dw_staging_xero.v2_control`" in sql
    assert "control_id = 'publish-00'" in sql
    assert "UPDATE `prj-dw-dev.dw_staging_xero.v2_receipt_locks`" in sql
    assert "IF NOT EXISTS" in sql and "@receipt_id" in sql and "@receipt_shard" in sql
    assert "shard_id = @receipt_shard" in sql
    assert "INSERT INTO `prj-dw-dev.dw_staging_xero.v2_record_versions`" in sql
    assert "DELETE" not in sql and "TRUNCATE" not in sql
    assert sql.count("UPDATE") == 2
    assert sql.index("v2_control") < sql.index("v2_receipt_locks") < sql.index("IF NOT EXISTS")


def test_receipt_lock_shard_is_stable_and_bounded():
    value = writer()

    first = value.receipt_shard_for("same-receipt")

    assert first == value.receipt_shard_for("same-receipt")
    assert isinstance(first, int)
    assert 0 <= first < RECEIPT_LOCK_SHARDS


def test_same_receipt_shard_uses_one_process_lock_but_other_shards_do_not():
    first = writer()
    second = writer()

    assert first._receipt_lock_for(7) is second._receipt_lock_for(7)
    assert first._receipt_lock_for(7) is not first._receipt_lock_for(8)


def test_same_shard_commits_are_serialized_in_one_function_instance():
    value = writer()
    active = 0
    max_active = 0
    guard = threading.Lock()

    def query(*_args, **_kwargs):
        nonlocal active, max_active
        with guard:
            active += 1
            max_active = max(max_active, active)
        time.sleep(0.02)
        with guard:
            active -= 1
        return MagicMock()

    value.client.query.side_effect = query
    with ThreadPoolExecutor(max_workers=2) as pool:
        futures = [pool.submit(value.commit, "sql", f"receipt-{index}", 17) for index in range(2)]
        for future in futures:
            future.result(timeout=2)

    assert max_active == 1


def test_bootstrap_provisions_a_range_partitioned_receipt_lock_table():
    value = writer()

    value.bootstrap()

    lock_table = next(
        call.args[0] for call in value.client.create_table.call_args_list
        if call.args[0].table_id == "v2_receipt_locks"
    )
    assert lock_table.range_partitioning.field == "shard_id"
    assert lock_table.range_partitioning.range_.start == 0
    assert lock_table.range_partitioning.range_.end == RECEIPT_LOCK_SHARDS
    assert lock_table.range_partitioning.range_.interval == 1
    assert lock_table.require_partition_filter

    lock_query = value.client.query.call_args.args[0]
    assert "GENERATE_ARRAY(0, 127)" in lock_query
    assert "v2_receipt_locks" in lock_query
    assert "T.shard_id BETWEEN 0 AND 127" in lock_query


def test_bootstrap_adds_missing_nullable_fields_to_an_existing_raw_table():
    value = writer()
    entity_schema = SCHEMAS["v2_sync_run_entities"]
    existing = bigquery.Table(
        "prj-dw-dev.dw_staging_xero.v2_sync_run_entities",
        schema=[field for field in entity_schema if field.name != "request_key"],
    )

    def get_table(reference):
        if reference == "prj-dw-dev.dw_staging_xero.v2_sync_run_entities":
            return existing
        raise NotFound("not found")

    value.client.get_table.side_effect = get_table

    value.bootstrap()

    assert "request_key" in {field.name for field in existing.schema}
    assert any(
        call.args[0] is existing and call.args[1] == ["schema"]
        for call in value.client.update_table.call_args_list
    )


@pytest.mark.parametrize("table", ["invoices", "v2_evil", "v2_records; DROP TABLE payments"])
def test_unknown_business_target_is_rejected(table):
    with pytest.raises(ValueError, match="target"):
        writer().transaction_sql({table: "_tmp_fresh"})


def test_temp_identifier_is_validated():
    with pytest.raises(ValueError, match="identifier"):
        writer().transaction_sql({"v2_record_versions": "tmp`; DROP TABLE x"})


def test_conflicting_transactions_retry_but_permission_errors_do_not(monkeypatch):
    value = writer()
    monkeypatch.setattr("etl.common.fresh_writer.time.sleep", lambda _: None)
    value.client.query.side_effect = [BadRequest("Transaction aborted due to concurrent update"), MagicMock()]
    value.commit("sql", "receipt", 0)
    assert value.client.query.call_count == 2
    value.client.query.side_effect = Forbidden("no permission")
    with pytest.raises(Forbidden):
        value.commit("sql", "receipt", 0)


def test_conflict_retry_is_bounded(monkeypatch):
    value = writer()
    monkeypatch.setattr("etl.common.fresh_writer.time.sleep", lambda _: None)
    value.client.query.side_effect = BadRequest("concurrent update")
    with pytest.raises(BadRequest):
        value.commit("sql", "receipt", 0)
    assert value.client.query.call_count == 5


def test_publish_requires_one_receipt_and_cleans_private_temps_on_failure():
    value = writer()
    value._write_temp = MagicMock()
    value._drop_temp = MagicMock()
    value.commit = MagicMock(side_effect=RuntimeError("rollback"))
    rows = {table: [] for table in SCHEMAS}
    rows["v2_ingest_receipts"] = [{field.name: None for field in SCHEMAS["v2_ingest_receipts"]}]
    rows["v2_ingest_receipts"][0]["receipt_id"] = "abc"
    with pytest.raises(RuntimeError, match="rollback"):
        value.publish(rows)
    assert value._drop_temp.call_count == 1
    with pytest.raises(ValueError, match="one receipt"):
        value.publish({"v2_ingest_receipts": []})


def test_schemas_are_explicit_and_history_has_no_expiration():
    value = writer()
    value.bootstrap()
    targets = [call.args[0] for call in value.client.create_table.call_args_list]
    assert len(targets) == len(SCHEMAS) + 2
    assert all(table.expires is None for table in targets)
    assert all(table.table_id.startswith("v2_") for table in targets)
    assert all(field.field_type for fields in SCHEMAS.values() for field in fields)


def test_publish_rejects_unexpected_fields_before_loading():
    value = writer()
    with pytest.raises(ValueError, match="schema"):
        value.publish({"v2_ingest_receipts": [{"receipt_id": "a", "evil": 1}]})
    value.client.query.assert_not_called()


def test_publish_rejects_business_rows_owned_by_a_different_receipt():
    value = writer()
    value._write_temp = MagicMock()
    record = {field.name: None for field in SCHEMAS["v2_record_versions"]}
    record["receipt_id"] = "other"
    receipt = {field.name: None for field in SCHEMAS["v2_ingest_receipts"]}
    receipt["receipt_id"] = "owner"

    with pytest.raises(ValueError, match="receipt mismatch"):
        value.publish({
            "v2_record_versions": [record],
            "v2_ingest_receipts": [receipt],
        })

    value._write_temp.assert_not_called()


def _batch(receipt_id):
    record = {field.name: None for field in SCHEMAS["v2_record_versions"]}
    record.update({"receipt_id": receipt_id, "endpoint": "assets_asset_details"})
    receipt = {field.name: None for field in SCHEMAS["v2_ingest_receipts"]}
    receipt.update({"receipt_id": receipt_id, "endpoint": "assets_asset_details"})
    return {
        "v2_record_versions": [record],
        "v2_ingest_receipts": [receipt],
    }


def test_batch_transaction_has_a_global_fence_and_receipt_guard():
    value = writer()

    sql = value.batch_transaction_sql({
        "v2_record_versions": "_tmp_fresh_records",
        "v2_ingest_receipts": "_tmp_fresh_receipts",
    })

    assert "UPDATE `prj-dw-dev.dw_staging_xero.v2_control`" in sql
    assert "control_id = 'publish-00'" in sql
    assert "NOT EXISTS (" in sql
    assert "FROM `prj-dw-dev.dw_staging_xero.v2_ingest_receipts` AS existing" in sql
    assert "INSERT INTO `prj-dw-dev.dw_staging_xero.v2_record_versions`" in sql
    assert "DELETE" not in sql and "TRUNCATE" not in sql


def test_batch_transaction_always_inserts_business_rows_before_receipts():
    value = writer()

    sql = value.batch_transaction_sql({
        "v2_ingest_receipts": "_tmp_fresh_receipts",
        "v2_record_versions": "_tmp_fresh_records",
    })

    assert sql.index("INSERT INTO `prj-dw-dev.dw_staging_xero.v2_record_versions`") < (
        sql.index("INSERT INTO `prj-dw-dev.dw_staging_xero.v2_ingest_receipts`")
    )


def test_publish_many_writes_one_atomic_batch_and_cleans_temps():
    value = writer()
    value._write_temp = MagicMock()
    value._drop_temp = MagicMock()
    value.commit_many = MagicMock()

    value.publish_many([_batch("receipt-one"), _batch("receipt-two")])

    assert value._write_temp.call_count == 2
    assert value.commit_many.call_count == 1
    assert value._drop_temp.call_count == 2
    sql = value.commit_many.call_args.args[0]
    assert "v2_record_versions" in sql
    assert "v2_ingest_receipts" in sql


def test_publish_many_rejects_duplicate_or_missing_receipt_before_writing():
    value = writer()
    value._write_temp = MagicMock()

    with pytest.raises(ValueError, match="duplicate receipt"):
        value.publish_many([_batch("receipt"), _batch("receipt")])
    with pytest.raises(ValueError, match="exactly one receipt"):
        value.publish_many([{"v2_ingest_receipts": []}])

    value._write_temp.assert_not_called()


def test_publish_many_rejects_rows_owned_by_a_different_receipt():
    value = writer()
    value._write_temp = MagicMock()
    invalid = _batch("owner")
    invalid["v2_record_versions"][0]["receipt_id"] = "other"

    with pytest.raises(ValueError, match="receipt mismatch"):
        value.publish_many([invalid])

    value._write_temp.assert_not_called()


def test_publish_many_keeps_collection_rows_tied_to_their_receipt():
    value = writer()
    value._write_temp = MagicMock()
    value._drop_temp = MagicMock()
    value.commit_many = MagicMock()
    collection = {field.name: None for field in SCHEMAS["v2_collection_versions"]}
    child = {field.name: None for field in SCHEMAS["v2_child_versions"]}
    receipt = {field.name: None for field in SCHEMAS["v2_ingest_receipts"]}
    receipt["receipt_id"] = "collection-receipt"

    value.publish_many([{
        "v2_collection_versions": [collection],
        "v2_child_versions": [child],
        "v2_ingest_receipts": [receipt],
    }])

    collection_write = next(
        call for call in value._write_temp.call_args_list
        if "v2_collection_versions" in call.args[0]
    )
    assert collection_write.args[1][0]["_batch_receipt_id"] == "collection-receipt"
    assert "_batch_receipt_id" in {field.name for field in collection_write.kwargs["schema"]}
    child_write = next(
        call for call in value._write_temp.call_args_list
        if "v2_child_versions" in call.args[0]
    )
    assert child_write.args[1][0]["_batch_receipt_id"] == "collection-receipt"
    assert "source._batch_receipt_id" in value.commit_many.call_args.args[0]
