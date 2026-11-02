"""Unit tests for delete-reconciliation helpers (no live BQ required)."""

from __future__ import annotations

import tempfile
from pathlib import Path

import pytest

from vsv2_tooling.bigquery.reconcile import load_erp_ids, reconcile


@pytest.mark.unit
def test_load_erp_ids_strings() -> None:
    with tempfile.NamedTemporaryFile(mode="w", suffix=".jsonl", delete=False) as f:
        f.write('"abc"\n"def"\n"abc"\n')
        path = Path(f.name)
    assert load_erp_ids(path) == {"abc", "def"}


@pytest.mark.unit
def test_load_erp_ids_numbers() -> None:
    with tempfile.NamedTemporaryFile(mode="w", suffix=".jsonl", delete=False) as f:
        f.write("1\n2\n3\n")
        path = Path(f.name)
    assert load_erp_ids(path) == {"1", "2", "3"}


@pytest.mark.unit
def test_load_erp_ids_ignores_blank_lines() -> None:
    with tempfile.NamedTemporaryFile(mode="w", suffix=".jsonl", delete=False) as f:
        f.write('"x"\n\n"y"\n  \n')
        path = Path(f.name)
    assert load_erp_ids(path) == {"x", "y"}


@pytest.mark.unit
def test_reconcile_empty_erp_ids_raises() -> None:
    with pytest.raises(ValueError, match="0 IDs"):
        reconcile(project="p", dataset="d", table="t", pk="id", erp_ids=set())


@pytest.mark.unit
def test_reconcile_empty_erp_does_not_call_bq() -> None:
    from unittest.mock import patch

    with patch("vsv2_tooling.bigquery.reconcile.bigquery.Client") as mock_cls:
        with pytest.raises(ValueError):
            reconcile(project="p", dataset="d", table="t", pk="id", erp_ids=set())
        mock_cls.assert_not_called()


@pytest.mark.unit
def test_reconcile_filters_bigquery_rows_by_tenant() -> None:
    from unittest.mock import MagicMock

    mock_client = MagicMock()
    deleted_at = MagicMock()
    deleted_at.name = "_deleted_at"
    table_meta = MagicMock()
    table_meta.schema = [deleted_at]
    mock_client.get_table.return_value = table_meta

    select_job = MagicMock()
    select_job.result.return_value = [{"pk_str": "a"}, {"pk_str": "b"}]
    update_job = MagicMock()
    update_job.num_dml_affected_rows = 1
    mock_client.query.side_effect = [select_job, update_job]

    deleted = reconcile(
        project="p",
        dataset="d",
        table="t",
        pk="id",
        erp_ids={"a"},
        tenant_id="tenant-a",
        client=mock_client,
    )

    assert deleted == 1
    queries = [call.args[0] for call in mock_client.query.call_args_list]
    assert all("`tenant_id` = 'tenant-a'" in query for query in queries)
