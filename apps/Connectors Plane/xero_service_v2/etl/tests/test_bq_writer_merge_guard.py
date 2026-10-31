"""Current staging rows must not move backward on replayed events."""

import pytest
from datetime import datetime, timezone
from types import SimpleNamespace
from google.cloud import bigquery

from etl.common.bq_writer import BQWriter


class _Job:
    def result(self):
        return None


class _Client:
    def __init__(self):
        self.sql = None

    def query(self, sql):
        self.sql = sql
        return _Job()


def test_merge_guard_rejects_older_synced_rows():
    writer = object.__new__(BQWriter)
    writer.client = _Client()
    writer._run_merge(
        "`project.dataset.target`", "`project.dataset.source`",
        ["tenant_id", "record_id", "synced_at", "amount"],
        ("tenant_id", "record_id"),
    )
    assert "WHEN MATCHED AND (T.synced_at IS NULL OR S.synced_at >= T.synced_at) THEN" in writer.client.sql


def test_merge_without_synced_at_keeps_unconditional_update():
    writer = object.__new__(BQWriter)
    writer.client = _Client()
    writer._run_merge(
        "`project.dataset.target`", "`project.dataset.source`",
        ["tenant_id", "record_id", "amount"],
        ("tenant_id", "record_id"),
    )
    assert "WHEN MATCHED THEN" in writer.client.sql


def test_new_synced_at_uses_existing_typed_schema():
    class Client:
        def load_table_from_json(self, rows, table, job_config):
            self.rows = rows
            self.job_config = job_config
            return _Job()

    writer = object.__new__(BQWriter)
    writer.client = Client()
    writer._set_temp_expiration = lambda _table: None
    observed = datetime(2026, 1, 2, tzinfo=timezone.utc)
    existing = [
        bigquery.SchemaField("tenant_id", "STRING"),
        bigquery.SchemaField("account_code", "STRING"),
    ]

    writer._write_temp("`project.dataset.temp`", [
        {"tenant_id": "t", "account_code": "200", "synced_at": observed},
    ], schema=existing)

    assert writer.client.job_config.autodetect is not True
    assert [(field.name, field.field_type) for field in writer.client.job_config.schema] == [
        ("tenant_id", "STRING"), ("account_code", "STRING"), ("synced_at", "TIMESTAMP"),
    ]
    assert writer.client.rows[0]["synced_at"] == observed.isoformat()


def test_legacy_child_table_merge_keeps_newer_row_and_adds_timestamp():
    class Client:
        def __init__(self):
            self.schema = [
                bigquery.SchemaField("tenant_id", "STRING"),
                bigquery.SchemaField("record_id", "STRING"),
                bigquery.SchemaField("line_item_id", "STRING"),
                bigquery.SchemaField("account_code", "STRING"),
            ]
            self.sql = []

        def get_table(self, table):
            schema = self.temp_schema if "_tmp_" in table else self.schema
            return SimpleNamespace(schema=schema)

        def load_table_from_json(self, rows, table, job_config):
            self.rows = rows
            self.temp_schema = job_config.schema
            return _Job()

        def query(self, sql):
            self.sql.append(sql)
            return _Job()

        def delete_table(self, *_args, **_kwargs):
            pass

    writer = object.__new__(BQWriter)
    writer.project = "project"
    writer.dataset = "dataset"
    writer.client = Client()
    writer._set_temp_expiration = lambda _table: None
    key = {"tenant_id": "t", "record_id": "r", "line_item_id": "l"}
    newer = datetime(2026, 1, 2, tzinfo=timezone.utc)
    older = datetime(2026, 1, 1, tzinfo=timezone.utc)

    written = writer.merge("invoice_lines", [
        {**key, "account_code": "200", "synced_at": newer},
        {**key, "account_code": "100", "synced_at": older},
    ], key_columns=("tenant_id", "record_id", "line_item_id"))

    assert written == 1
    assert writer.client.rows[0]["account_code"] == "200"
    assert "ADD COLUMN IF NOT EXISTS synced_at TIMESTAMP" in writer.client.sql[0]
    assert "WHEN MATCHED AND (T.synced_at IS NULL OR S.synced_at >= T.synced_at) THEN" in writer.client.sql[1]
    assert "DELETE" not in writer.client.sql[1]


def test_insert_if_absent_never_updates_existing_snapshot():
    writer = object.__new__(BQWriter)
    writer.client = _Client()
    writer._run_insert_if_absent(
        "`project.dataset.attachment_list_snapshots`",
        "`project.dataset.source`",
        ["tenant_id", "snapshot_id", "attachment_ids"],
        ("tenant_id", "snapshot_id"),
    )
    assert "WHEN NOT MATCHED THEN" in writer.client.sql
    assert "WHEN MATCHED" not in writer.client.sql
    assert "UPDATE" not in writer.client.sql


def test_insert_if_absent_rejects_dynamic_sql_identifiers():
    writer = object.__new__(BQWriter)
    writer.project = "project"
    writer.dataset = "dataset"
    with pytest.raises(ValueError, match="identifier"):
        writer.insert_if_absent(
            "snapshots; DROP TABLE metadata", [{"snapshot_id": "id"}],
            ("snapshot_id",), [],
        )
