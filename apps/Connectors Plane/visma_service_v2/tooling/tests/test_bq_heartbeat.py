"""The sync heartbeat: its row, its write, the never-fails rule, and the stage CLI that writes it.

BigQuery is mocked throughout: no credentials, no network.
"""

from __future__ import annotations

import json
import logging
import threading
import time
from datetime import UTC, datetime
from unittest.mock import MagicMock, patch

import pytest
from typer.testing import CliRunner

from vsv2_tooling.bigquery import heartbeat
from vsv2_tooling.bigquery.cli import stage_app
from vsv2_tooling.bigquery.heartbeat import (
    HEARTBEAT_DEADLINE_S,
    HEARTBEAT_SCHEMA,
    HEARTBEAT_TABLE,
    HeartbeatError,
    heartbeat_row,
    record_heartbeat,
    write_heartbeat,
)

PROJECT = "aquatiq-datavarehus"
DATASET = "dw_staging_visma"
TABLE_ID = f"{PROJECT}.{DATASET}.{HEARTBEAT_TABLE}"
TENANT = "33f319b9-4d57-11ee-960c-025417856183"
WHEN = datetime(2026, 10, 10, 9, 30, 15, 123456, tzinfo=UTC)
STAGE_RESULT = {"rows": 3, "changed": 2, "deleted": 0, "delete_skipped": 0}


def _client(insert_errors: list | None = None) -> MagicMock:
    client = MagicMock()
    client.insert_rows_json.return_value = insert_errors or []
    return client


def _row(rows_written: int = 3, endpoint: str = "customer_invoices") -> dict:
    return {
        "tenant_id": TENANT,
        "endpoint": endpoint,
        "finished_at": "2026-10-10T09:30:15.123456+00:00",
        "status": "succeeded",
        "rows_written": rows_written,
    }


# ─── the row ──────────────────────────────────────────────────────────────────


@pytest.mark.unit
def test_the_row_has_the_five_columns_in_utc() -> None:
    row = heartbeat_row(
        tenant_id=TENANT, endpoint="customer_invoices", rows_written=3, finished_at=WHEN
    )

    assert row == _row(3)
    assert [field.name for field in HEARTBEAT_SCHEMA] == list(row)


@pytest.mark.unit
def test_a_local_time_is_stored_as_utc() -> None:
    from zoneinfo import ZoneInfo

    local = WHEN.astimezone(ZoneInfo("Europe/Oslo"))
    row = heartbeat_row(
        tenant_id=TENANT, endpoint="supplier_invoices", rows_written=0, finished_at=local
    )

    assert row["finished_at"] == "2026-10-10T09:30:15.123456+00:00"


# ─── the write ────────────────────────────────────────────────────────────────


@pytest.mark.unit
def test_write_creates_the_table_and_inserts_exactly_one_row() -> None:
    client = _client()

    write_heartbeat(
        client,
        project=PROJECT,
        dataset=DATASET,
        tenant_id=TENANT,
        endpoint="customer_invoices",
        rows_written=3,
        finished_at=WHEN,
    )

    table = client.create_table.call_args.args[0]
    assert (table.project, table.dataset_id, table.table_id) == (PROJECT, DATASET, HEARTBEAT_TABLE)
    assert [field.name for field in table.schema] == [field.name for field in HEARTBEAT_SCHEMA]
    assert client.create_table.call_args.kwargs["exists_ok"] is True
    client.insert_rows_json.assert_called_once()
    (table_id, rows), kwargs = client.insert_rows_json.call_args
    assert (table_id, rows) == (TABLE_ID, [_row(3)])
    assert kwargs["timeout"] == HEARTBEAT_DEADLINE_S


@pytest.mark.unit
def test_every_call_carries_the_deadline_as_timeout_and_retry_deadline() -> None:
    client = _client()

    write_heartbeat(
        client,
        project=PROJECT,
        dataset=DATASET,
        tenant_id=TENANT,
        endpoint="customer_invoices",
        rows_written=3,
        finished_at=WHEN,
    )

    for call in (client.create_table.call_args, client.insert_rows_json.call_args):
        assert call.kwargs["timeout"] == HEARTBEAT_DEADLINE_S
        assert call.kwargs["retry"].deadline == HEARTBEAT_DEADLINE_S


@pytest.mark.unit
def test_a_hanging_write_is_abandoned_at_the_deadline(
    monkeypatch: pytest.MonkeyPatch, caplog: pytest.LogCaptureFixture
) -> None:
    """A call BigQuery never answers must not hold up the sync: return at the deadline."""
    monkeypatch.setattr(heartbeat, "HEARTBEAT_DEADLINE_S", 0.2)
    release = threading.Event()
    client = _client()
    client.insert_rows_json.side_effect = lambda *args, **kwargs: release.wait(30) and []

    started = time.monotonic()
    with caplog.at_level(logging.WARNING):
        written = record_heartbeat(
            project=PROJECT,
            dataset=DATASET,
            tenant_id=TENANT,
            endpoint="customer_invoices",
            rows_written=3,
            client=client,
        )
    elapsed = time.monotonic() - started
    release.set()  # let the abandoned thread finish, so the test leaves nothing running

    assert written is False
    assert elapsed < 5
    assert "no answer from BigQuery within" in caplog.text


@pytest.mark.unit
def test_a_refused_insert_raises_heartbeat_error() -> None:
    client = _client(insert_errors=[{"index": 0, "errors": [{"reason": "invalid"}]}])

    with pytest.raises(HeartbeatError, match="refused the heartbeat"):
        write_heartbeat(
            client,
            project=PROJECT,
            dataset=DATASET,
            tenant_id=TENANT,
            endpoint="customer_invoices",
            rows_written=3,
            finished_at=WHEN,
        )


@pytest.mark.unit
def test_record_returns_true_when_the_row_is_written() -> None:
    client = _client()

    written = record_heartbeat(
        project=PROJECT,
        dataset=DATASET,
        tenant_id=TENANT,
        endpoint="supplier_invoices",
        rows_written=0,
        client=client,
    )

    assert written is True
    (table_id, rows), _ = client.insert_rows_json.call_args
    assert table_id == TABLE_ID
    assert rows[0]["rows_written"] == 0
    assert rows[0]["status"] == "succeeded"


@pytest.mark.unit
def test_a_refused_row_is_logged_and_never_raised(caplog: pytest.LogCaptureFixture) -> None:
    client = _client(insert_errors=[{"index": 0, "errors": [{"reason": "invalid"}]}])

    with caplog.at_level(logging.WARNING):
        written = record_heartbeat(
            project=PROJECT,
            dataset=DATASET,
            tenant_id=TENANT,
            endpoint="customer_invoices",
            rows_written=3,
            client=client,
        )

    assert written is False
    assert "heartbeat not written" in caplog.text
    assert TENANT in caplog.text and "customer_invoices" in caplog.text


@pytest.mark.unit
def test_a_network_error_is_never_raised() -> None:
    client = _client()
    client.insert_rows_json.side_effect = OSError("connection reset")

    assert (
        record_heartbeat(
            project=PROJECT,
            dataset=DATASET,
            tenant_id=TENANT,
            endpoint="customer_invoices",
            rows_written=3,
            client=client,
        )
        is False
    )


@pytest.mark.unit
def test_no_client_at_all_is_logged_and_never_raised() -> None:
    with patch(
        "vsv2_tooling.bigquery.heartbeat.bigquery.Client",
        side_effect=RuntimeError("no credentials"),
    ):
        written = record_heartbeat(
            project=PROJECT,
            dataset=DATASET,
            tenant_id=TENANT,
            endpoint="customer_invoices",
            rows_written=3,
        )

    assert written is False


# ─── vsv2-bq-stage writes the heartbeat after its own successful write ──────────


def _stage_args(tenant: str | None = TENANT, endpoint: str = "customer_invoices") -> list[str]:
    args = [
        "--project",
        PROJECT,
        "--dataset",
        DATASET,
        "--table",
        endpoint,
        "--pk",
        "tenant_id,referenceNumber",
        "--input",
        "-",
        "--run-id",
        "11111111-1111-4111-8111-111111111111",
        "--endpoint",
        endpoint,
        "--source",
        "erp_rest",
    ]
    if tenant is not None:
        args += ["--tenant", tenant]
    return args


@pytest.mark.unit
def test_stage_writes_one_heartbeat_after_a_successful_write() -> None:
    with (
        patch("vsv2_tooling.bigquery.cli.stage", return_value=STAGE_RESULT) as stage,
        patch("vsv2_tooling.bigquery.cli.record_heartbeat", return_value=True) as heartbeat,
    ):
        result = CliRunner().invoke(stage_app, _stage_args(), input="")

    assert result.exit_code == 0, result.output
    assert json.loads(result.stdout) == STAGE_RESULT
    stage.assert_called_once()
    heartbeat.assert_called_once_with(
        project=PROJECT,
        dataset=DATASET,
        tenant_id=TENANT,
        endpoint="customer_invoices",
        rows_written=3,
    )


@pytest.mark.unit
def test_an_empty_batch_still_writes_a_heartbeat_with_zero_rows() -> None:
    empty = {"rows": 0, "changed": 0, "deleted": 0, "delete_skipped": 0}
    with (
        patch("vsv2_tooling.bigquery.cli.stage", return_value=empty),
        patch("vsv2_tooling.bigquery.cli.record_heartbeat", return_value=True) as heartbeat,
    ):
        result = CliRunner().invoke(stage_app, _stage_args(endpoint="supplier_invoices"), input="")

    assert result.exit_code == 0, result.output
    heartbeat.assert_called_once()
    assert heartbeat.call_args.kwargs["rows_written"] == 0
    assert heartbeat.call_args.kwargs["endpoint"] == "supplier_invoices"


@pytest.mark.unit
def test_a_failed_stage_writes_no_heartbeat() -> None:
    with (
        patch("vsv2_tooling.bigquery.cli.stage", side_effect=RuntimeError("bq down")),
        patch("vsv2_tooling.bigquery.cli.record_heartbeat") as heartbeat,
    ):
        result = CliRunner().invoke(stage_app, _stage_args(), input="")

    assert result.exit_code != 0
    heartbeat.assert_not_called()


@pytest.mark.unit
def test_a_stage_without_a_tenant_writes_no_heartbeat() -> None:
    with (
        patch("vsv2_tooling.bigquery.cli.stage", return_value=STAGE_RESULT),
        patch("vsv2_tooling.bigquery.cli.record_heartbeat") as heartbeat,
    ):
        result = CliRunner().invoke(stage_app, _stage_args(tenant=None), input="")

    assert result.exit_code == 0, result.output
    heartbeat.assert_not_called()


@pytest.mark.unit
def test_a_refused_heartbeat_does_not_fail_the_stage() -> None:
    with (
        patch("vsv2_tooling.bigquery.cli.stage", return_value=STAGE_RESULT),
        patch(
            "vsv2_tooling.bigquery.heartbeat.bigquery.Client",
            side_effect=RuntimeError("no credentials"),
        ),
    ):
        result = CliRunner().invoke(stage_app, _stage_args(), input="")

    assert result.exit_code == 0, result.output
    assert json.loads(result.stdout) == STAGE_RESULT
