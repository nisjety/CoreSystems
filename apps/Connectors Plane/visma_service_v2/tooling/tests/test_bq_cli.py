"""Unit tests for vsv2-bq-merge and vsv2-bq-shadow CLI entry points.

BigQuery client is fully mocked — no GCP credentials required.
"""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any
from unittest.mock import MagicMock, patch

import pytest
from typer.testing import CliRunner

from vsv2_tooling.bigquery.cli import merge_app, quarantine_app, shadow_app

# ─── helpers ──────────────────────────────────────────────────────────────────


def _jsonl(*records: dict[str, Any]) -> str:
    return "\n".join(json.dumps(r) for r in records)


def _write_jsonl(tmp_path: Path, *records: dict[str, Any]) -> Path:
    p = tmp_path / "input.jsonl"
    p.write_text(_jsonl(*records))
    return p


RECORDS = [
    {"accountCD": "1500", "description": "Trade receivables", "type": "Asset"},
    {"accountCD": "1600", "description": "Other receivables", "type": "Asset"},
]


# ─── vsv2-bq-merge ────────────────────────────────────────────────────────────


@pytest.mark.unit
def test_merge_outputs_row_count(tmp_path: Path) -> None:
    """merge command prints {"rows": N} on stdout."""
    inp = _write_jsonl(tmp_path, *RECORDS)

    with patch("vsv2_tooling.bigquery.cli.merge", return_value=2) as mock_merge:
        runner = CliRunner()
        result = runner.invoke(
            merge_app,
            [
                "--project",
                "my-project",
                "--dataset",
                "visma_bronze",
                "--table",
                "visma_accounts",
                "--pk",
                "accountCD",
                "--input",
                str(inp),
            ],
        )

    assert result.exit_code == 0, result.output
    out = json.loads(result.output.strip())
    assert out == {"rows": 2}

    mock_merge.assert_called_once_with(
        project="my-project",
        dataset="visma_bronze",
        table="visma_accounts",
        primary_key="accountCD",
        records=RECORDS,
        normalizer_name=None,
        run_id=None,
        endpoint=None,
        source=None,
    )


@pytest.mark.unit
def test_merge_empty_input_exits_zero(tmp_path: Path) -> None:
    """Empty JSONL file returns {"rows": 0} without touching BQ."""
    inp = tmp_path / "empty.jsonl"
    inp.write_text("")

    with patch("vsv2_tooling.bigquery.cli.merge", return_value=0) as mock_merge:
        runner = CliRunner()
        result = runner.invoke(
            merge_app,
            [
                "--project",
                "p",
                "--dataset",
                "ds",
                "--table",
                "t",
                "--pk",
                "id",
                "--input",
                str(inp),
            ],
        )

    assert result.exit_code == 0, result.output
    assert json.loads(result.output.strip()) == {"rows": 0}
    mock_merge.assert_called_once_with(
        project="p",
        dataset="ds",
        table="t",
        primary_key="id",
        records=[],
        normalizer_name=None,
        run_id=None,
        endpoint=None,
        source=None,
    )


@pytest.mark.unit
def test_merge_passes_staging_metadata(tmp_path: Path) -> None:
    inp = _write_jsonl(tmp_path, *RECORDS)

    with patch("vsv2_tooling.bigquery.cli.merge", return_value=2) as mock_merge:
        runner = CliRunner()
        result = runner.invoke(
            merge_app,
            [
                "--project",
                "p",
                "--dataset",
                "bronze",
                "--table",
                "visma_accounts",
                "--pk",
                "tenant_id,accountCD",
                "--input",
                str(inp),
                "--run-id",
                "run-1",
                "--endpoint",
                "accounts",
                "--source",
                "erp_rest",
            ],
        )

    assert result.exit_code == 0, result.output
    mock_merge.assert_called_once_with(
        project="p",
        dataset="bronze",
        table="visma_accounts",
        primary_key="tenant_id,accountCD",
        records=RECORDS,
        normalizer_name=None,
        run_id="run-1",
        endpoint="accounts",
        source="erp_rest",
    )


@pytest.mark.unit
def test_merge_propagates_bq_error(tmp_path: Path) -> None:
    """If merge() raises, the CLI must exit non-zero."""
    inp = _write_jsonl(tmp_path, *RECORDS)

    with patch(
        "vsv2_tooling.bigquery.cli.merge",
        side_effect=RuntimeError("BQ quota exceeded"),
    ):
        runner = CliRunner()
        result = runner.invoke(
            merge_app,
            [
                "--project",
                "p",
                "--dataset",
                "ds",
                "--table",
                "t",
                "--pk",
                "id",
                "--input",
                str(inp),
            ],
        )

    assert result.exit_code != 0


# ─── vsv2-bq-shadow ───────────────────────────────────────────────────────────


@pytest.mark.unit
def test_shadow_outputs_row_count(tmp_path: Path) -> None:
    """shadow command prints {"rows": N} on stdout."""
    inp = _write_jsonl(tmp_path, *RECORDS)

    with patch("vsv2_tooling.bigquery.cli.write_shadow", return_value=2) as mock_ws:
        runner = CliRunner()
        result = runner.invoke(
            shadow_app,
            [
                "--project",
                "my-project",
                "--dataset-shadow",
                "visma_shadow",
                "--table",
                "visma_accounts",
                "--input",
                str(inp),
            ],
        )

    assert result.exit_code == 0, result.output
    assert json.loads(result.output.strip()) == {"rows": 2}

    mock_ws.assert_called_once_with(
        project="my-project",
        dataset_shadow="visma_shadow",
        table="visma_accounts",
        records=RECORDS,
        normalizer_name=None,
    )


@pytest.mark.unit
def test_shadow_propagates_bq_error(tmp_path: Path) -> None:
    inp = _write_jsonl(tmp_path, *RECORDS)

    with patch(
        "vsv2_tooling.bigquery.cli.write_shadow",
        side_effect=RuntimeError("BQ write failed"),
    ):
        runner = CliRunner()
        result = runner.invoke(
            shadow_app,
            [
                "--project",
                "p",
                "--dataset-shadow",
                "shadow",
                "--table",
                "t",
                "--input",
                str(inp),
            ],
        )

    assert result.exit_code != 0


# ─── vsv2-bq-quarantine ──────────────────────────────────────────────────────


@pytest.mark.unit
def test_quarantine_outputs_row_count(tmp_path: Path) -> None:
    inp = _write_jsonl(tmp_path, {"sync_run_id": "run-1", "reason": "bad"})

    with patch("vsv2_tooling.bigquery.cli.write_quarantine", return_value=1) as mock_quarantine:
        runner = CliRunner()
        result = runner.invoke(
            quarantine_app,
            [
                "--project",
                "p",
                "--dataset",
                "bronze",
                "--input",
                str(inp),
            ],
        )

    assert result.exit_code == 0, result.output
    assert json.loads(result.output.strip()) == {"rows": 1}
    mock_quarantine.assert_called_once_with(
        project="p",
        dataset="bronze",
        records=[{"sync_run_id": "run-1", "reason": "bad"}],
    )


# ─── loader unit tests (no subprocess) ───────────────────────────────────────


@pytest.mark.unit
def test_loader_merge_calls_bq(tmp_path: Path) -> None:
    """loader.merge() calls BQ load + query and returns affected rows."""
    from vsv2_tooling.bigquery.loader import merge

    mock_client = MagicMock()
    # First get_table call checks the target schema, second reads temp schema.
    mock_field = MagicMock()
    mock_field.name = "accountCD"
    target_meta = MagicMock()
    target_meta.schema = [mock_field]
    temp_meta = MagicMock()
    temp_meta.schema = [mock_field]
    mock_client.get_table.side_effect = [target_meta, temp_meta]

    mock_load_job = MagicMock()
    mock_client.load_table_from_json.return_value = mock_load_job

    mock_query_job = MagicMock()
    mock_query_job.num_dml_affected_rows = 2
    mock_client.query.return_value = mock_query_job

    n = merge(
        project="p",
        dataset="ds",
        table="visma_accounts",
        primary_key="accountCD",
        records=[{"accountCD": "1500"}, {"accountCD": "1600"}],
        client=mock_client,
    )

    assert n == 2
    mock_client.update_table.assert_called_once()
    mock_client.load_table_from_json.assert_called_once()
    mock_client.query.assert_called_once()
    # Temp table should always be cleaned up
    mock_client.delete_table.assert_called_once()


@pytest.mark.unit
def test_loader_write_shadow_calls_bq() -> None:
    """loader.write_shadow() calls BQ load_table_from_json with WRITE_APPEND."""
    from vsv2_tooling.bigquery.loader import write_shadow

    mock_client = MagicMock()
    mock_load_job = MagicMock()
    mock_client.load_table_from_json.return_value = mock_load_job

    n = write_shadow(
        project="p",
        dataset_shadow="shadow",
        table="visma_accounts",
        records=[{"accountCD": "1500"}],
        client=mock_client,
    )

    assert n == 1
    mock_client.load_table_from_json.assert_called_once()
    call_args = mock_client.load_table_from_json.call_args
    # Second positional arg is the destination table id
    assert "shadow" in call_args.args[1]
    assert "_dm_shadow" in call_args.args[1]


def test_a_record_with_a_unicode_line_separator_stays_one_record(tmp_path: Path) -> None:
    """The Rust side writes U+2028, U+2029 and U+0085 unescaped; a record that holds one must not
    be split into two lines (the quarantine rows of a failed run would be lost)."""
    from vsv2_tooling.bigquery.cli import _read_records

    record = {"raw_payload_json": "left\u2028middle\u2029right\u0085end"}
    path = tmp_path / "rows.jsonl"
    path.write_text(json.dumps(record, ensure_ascii=False) + "\n", encoding="utf-8")

    assert _read_records(path) == [record]
