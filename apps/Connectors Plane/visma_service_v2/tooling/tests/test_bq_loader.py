"""Unit tests for the BigQuery loader helpers (no live BQ required)."""

from __future__ import annotations

from unittest.mock import MagicMock

import pytest

from vsv2_tooling.bigquery.loader import (
    CHUNK_SIZE,
    _apply_normalizer,
    _record_pk,
    chunk_size_for,
    chunked,
)


@pytest.mark.unit
def test_chunked_exact_multiple() -> None:
    records = [{"id": i} for i in range(10)]
    chunks = list(chunked(records, size=5))
    assert len(chunks) == 2
    assert all(len(c) == 5 for c in chunks)


@pytest.mark.unit
def test_chunked_remainder() -> None:
    records = [{"id": i} for i in range(7)]
    chunks = list(chunked(records, size=3))
    assert len(chunks) == 3
    assert [len(c) for c in chunks] == [3, 3, 1]


@pytest.mark.unit
def test_chunked_empty() -> None:
    assert list(chunked([], size=500)) == []


@pytest.mark.unit
def test_chunked_smaller_than_size() -> None:
    records = [{"id": 0}, {"id": 1}]
    chunks = list(chunked(records, size=500))
    assert len(chunks) == 1
    assert chunks[0] == records


@pytest.mark.unit
def test_chunk_size_for_unknown_endpoint_returns_default() -> None:
    assert chunk_size_for("widgets") == CHUNK_SIZE
    assert chunk_size_for(None) == CHUNK_SIZE


@pytest.mark.unit
def test_chunk_size_for_known_high_volume_endpoint() -> None:
    """High-volume tx endpoints get 2000 to amortise load-job cost."""
    assert chunk_size_for("general_ledger_transactions") == 2000
    assert chunk_size_for("customer_invoices") == 2000


@pytest.mark.unit
def test_chunk_size_for_wide_payload_endpoint() -> None:
    """Wide-payload endpoints stay at 1000 to balance round-trips vs payload size."""
    assert chunk_size_for("sales_orders") == 1000
    assert chunk_size_for("sales_orders_v3") == 1000


@pytest.mark.unit
def test_merge_empty_records_returns_zero() -> None:
    from unittest.mock import patch

    with patch("vsv2_tooling.bigquery.loader.bigquery.Client") as mock_client_cls:
        from vsv2_tooling.bigquery.loader import merge

        result = merge(project="p", dataset="d", table="t", primary_key="id", records=[])
        assert result == 0
        mock_client_cls.assert_not_called()


@pytest.mark.unit
def test_write_shadow_empty_records_returns_zero() -> None:
    from unittest.mock import patch

    with patch("vsv2_tooling.bigquery.loader.bigquery.Client") as mock_client_cls:
        from vsv2_tooling.bigquery.loader import write_shadow

        result = write_shadow(project="p", dataset_shadow="s", table="t", records=[])
        assert result == 0
        mock_client_cls.assert_not_called()


@pytest.mark.unit
def test_apply_normalizer_reshapes_fields() -> None:
    records = [{"accountCD": "1500", "description": "Trade", "type": "Asset", "active": True}]
    out = _apply_normalizer(records, "normalize_account")
    assert out[0]["accountCD"] == "1500"
    assert "description" in out[0]


@pytest.mark.unit
def test_apply_normalizer_preserves_orchestrator_metadata() -> None:
    records = [
        {
            "tenant_id": "tenant-a",
            "accountCD": "1500",
            "description": "Trade",
            "type": "Asset",
            "active": True,
            "_source": "datamart_graphql",
            "_synced_at": "2026-04-30T10:00:00+00:00",
        }
    ]
    out = _apply_normalizer(records, "normalize_account")
    # Rust-injected value must win over normalizer's own "_source": "datamart"
    assert out[0]["tenant_id"] == "tenant-a"
    assert out[0]["_source"] == "datamart_graphql"
    assert out[0]["_synced_at"] == "2026-04-30T10:00:00+00:00"


@pytest.mark.unit
def test_apply_normalizer_sets_datamart_tombstone_at_load_boundary() -> None:
    records = [
        {
            "_source": "datamart_graphql",
            "companyId": 99,
            "accountId": 3000,
            "ledgerId": 2,
            "finPeriodId": "202605",
            "deletedDatabaseRecord": True,
        }
    ]

    out = _apply_normalizer(records, "normalize_general_ledger_balance")

    assert out[0]["isDeleted"] is True


@pytest.mark.unit
def test_apply_normalizer_does_not_infer_erp_tombstone() -> None:
    records = [
        {
            "_source": "erp_rest",
            "companyId": 99,
            "accountId": 3000,
            "ledgerId": 2,
            "finPeriodId": "202605",
            "deletedDatabaseRecord": True,
        }
    ]

    out = _apply_normalizer(records, "normalize_general_ledger_balance")

    assert "isDeleted" not in out[0]


@pytest.mark.unit
def test_record_pk_supports_dotted_paths_inside_json_strings() -> None:
    record = {
        "account": '{"number":"3000"}',
        "subAccountCd": "010200",
        "financialPeriod": "202605",
        "ledger": '{"id":"2"}',
    }
    assert (
        _record_pk(
            record,
            ["account.number", "subAccountCd", "financialPeriod", "ledger.id"],
        )
        == "3000\x1f010200\x1f202605\x1f2"
    )


@pytest.mark.unit
def test_apply_normalizer_unknown_name_raises() -> None:
    with pytest.raises(ValueError, match="Unknown normalizer"):
        _apply_normalizer([{"id": 1}], "normalize_does_not_exist")


@pytest.mark.unit
def test_apply_normalizer_non_dict_output_raises_with_context() -> None:
    """Normalizer returning the wrong shape should surface as
    ``NormalizerOutputError`` carrying record_index and key context, not a
    generic crash deep in BQ load. Lets the orchestrator quarantine the
    batch instead of crash-loop."""
    from vsv2_tooling.bigquery.loader import NormalizerOutputError
    from vsv2_tooling.normalizers import REGISTRY

    def bad_normalizer(_dm: dict) -> list:  # type: ignore[return-type]
        return ["not", "a", "dict"]  # type: ignore[return-value]

    REGISTRY["__test_bad_shape"] = bad_normalizer  # type: ignore[assignment]
    try:
        with pytest.raises(NormalizerOutputError) as exc_info:
            _apply_normalizer([{"id": 1}, {"id": 2}], "__test_bad_shape")
        err = exc_info.value
        assert err.normalizer_name == "__test_bad_shape"
        assert err.record_index == 0
        assert "expected dict" in err.reason
        assert err.sample_keys == ["id"]
    finally:
        del REGISTRY["__test_bad_shape"]


@pytest.mark.unit
def test_merge_with_shadow_off_calls_only_merge() -> None:
    """shadow_mode='off' → only the live MERGE path runs; shadow append is skipped."""
    from unittest.mock import patch

    from vsv2_tooling.bigquery.loader import merge_with_shadow

    with (
        patch("vsv2_tooling.bigquery.loader.merge") as mock_merge,
        patch("vsv2_tooling.bigquery.loader.write_shadow") as mock_shadow,
    ):
        mock_merge.return_value = 7
        result = merge_with_shadow(
            project="p",
            dataset="d",
            table="t",
            primary_key="id",
            records=[{"id": "1"}],
            shadow_mode="off",
            client=MagicMock(),
        )
        assert result.merged == 7
        assert result.shadowed == 0
        mock_merge.assert_called_once()
        mock_shadow.assert_not_called()


@pytest.mark.unit
def test_merge_with_shadow_dual_calls_both() -> None:
    from unittest.mock import patch

    from vsv2_tooling.bigquery.loader import merge_with_shadow

    with (
        patch("vsv2_tooling.bigquery.loader.merge") as mock_merge,
        patch("vsv2_tooling.bigquery.loader.write_shadow") as mock_shadow,
    ):
        mock_merge.return_value = 7
        mock_shadow.return_value = 7
        result = merge_with_shadow(
            project="p",
            dataset="d",
            table="t",
            primary_key="id",
            records=[{"id": "1"}],
            shadow_mode="dual",
            client=MagicMock(),
        )
        assert result.merged == 7
        assert result.shadowed == 7
        mock_merge.assert_called_once()
        mock_shadow.assert_called_once()


@pytest.mark.unit
def test_merge_with_shadow_only_skips_live_merge() -> None:
    """shadow_only is for endpoints under isolated validation — don't touch bronze."""
    from unittest.mock import patch

    from vsv2_tooling.bigquery.loader import merge_with_shadow

    with (
        patch("vsv2_tooling.bigquery.loader.merge") as mock_merge,
        patch("vsv2_tooling.bigquery.loader.write_shadow") as mock_shadow,
    ):
        mock_shadow.return_value = 3
        result = merge_with_shadow(
            project="p",
            dataset="d",
            table="t",
            primary_key="id",
            records=[{"id": "1"}, {"id": "2"}, {"id": "3"}],
            shadow_mode="shadow_only",
            client=MagicMock(),
        )
        assert result.merged == 0
        assert result.shadowed == 3
        mock_merge.assert_not_called()
        mock_shadow.assert_called_once()


@pytest.mark.unit
def test_merge_with_shadow_unknown_mode_raises() -> None:
    from vsv2_tooling.bigquery.loader import merge_with_shadow

    with pytest.raises(ValueError, match="shadow_mode must be one of"):
        merge_with_shadow(
            project="p",
            dataset="d",
            table="t",
            primary_key="id",
            records=[{"id": "1"}],
            shadow_mode="bogus",
        )


@pytest.mark.unit
def test_merge_with_shadow_empty_records_short_circuits() -> None:
    from unittest.mock import patch

    from vsv2_tooling.bigquery.loader import merge_with_shadow

    with (
        patch("vsv2_tooling.bigquery.loader.merge") as mock_merge,
        patch("vsv2_tooling.bigquery.loader.write_shadow") as mock_shadow,
    ):
        result = merge_with_shadow(
            project="p",
            dataset="d",
            table="t",
            primary_key="id",
            records=[],
            shadow_mode="dual",
        )
        assert result.merged == 0
        assert result.shadowed == 0
        mock_merge.assert_not_called()
        mock_shadow.assert_not_called()


@pytest.mark.unit
def test_merge_with_shadow_dataset_default() -> None:
    """dataset_shadow defaults to f'{dataset}_dm_shadow' when not supplied."""
    from unittest.mock import patch

    from vsv2_tooling.bigquery.loader import merge_with_shadow

    captured: dict = {}

    def fake_shadow(**kwargs):
        captured.update(kwargs)
        return 1

    with (
        patch("vsv2_tooling.bigquery.loader.merge", return_value=0),
        patch("vsv2_tooling.bigquery.loader.write_shadow", side_effect=fake_shadow),
    ):
        merge_with_shadow(
            project="p",
            dataset="dw_1_bronze_visma",
            table="visma_x",
            primary_key="id",
            records=[{"id": "1"}],
            shadow_mode="dual",
        )
        assert captured["dataset_shadow"] == "dw_1_bronze_visma_dm_shadow"


@pytest.mark.unit
def test_apply_normalizer_exception_wrapped_with_index() -> None:
    """If a normalizer raises mid-batch, surface the record index and the
    original exception type so log triage can find the offending payload
    quickly."""
    from vsv2_tooling.bigquery.loader import NormalizerOutputError
    from vsv2_tooling.normalizers import REGISTRY

    def fragile_normalizer(dm: dict) -> dict:
        if dm.get("id") == 5:
            raise KeyError("required_field_missing")
        return {"id": dm["id"]}

    REGISTRY["__test_fragile"] = fragile_normalizer
    try:
        records = [{"id": i} for i in range(10)]
        with pytest.raises(NormalizerOutputError) as exc_info:
            _apply_normalizer(records, "__test_fragile")
        err = exc_info.value
        assert err.record_index == 5
        assert "KeyError" in err.reason
        assert "required_field_missing" in err.reason
    finally:
        del REGISTRY["__test_fragile"]


@pytest.mark.unit
def test_merge_rejects_missing_primary_key_field() -> None:
    from vsv2_tooling.bigquery.loader import merge

    mock_client = MagicMock()
    account_field = MagicMock()
    account_field.name = "accountCD"
    tenant_field = MagicMock()
    tenant_field.name = "tenant_id"
    target_meta = MagicMock()
    target_meta.schema = [tenant_field, account_field]
    temp_meta = MagicMock()
    temp_meta.schema = [account_field]
    mock_client.get_table.side_effect = [target_meta, temp_meta]

    mock_load_job = MagicMock()
    mock_client.load_table_from_json.return_value = mock_load_job

    with pytest.raises(ValueError, match="tenant_id"):
        merge(
            project="p",
            dataset="ds",
            table="visma_accounts",
            primary_key="tenant_id,accountCD",
            records=[{"accountCD": "1500"}],
            client=mock_client,
        )


@pytest.mark.unit
def test_merge_writes_append_only_raw_stage_when_run_metadata_present() -> None:
    from vsv2_tooling.bigquery.loader import merge

    mock_client = MagicMock()
    tenant_field = MagicMock()
    tenant_field.name = "tenant_id"
    account_field = MagicMock()
    account_field.name = "accountCD"
    description_field = MagicMock()
    description_field.name = "description"
    source_field = MagicMock()
    source_field.name = "_source"
    synced_field = MagicMock()
    synced_field.name = "_synced_at"
    target_meta = MagicMock()
    target_meta.schema = [
        tenant_field,
        account_field,
        description_field,
        source_field,
        synced_field,
    ]
    temp_meta = MagicMock()
    temp_meta.schema = [
        tenant_field,
        account_field,
        description_field,
        source_field,
        synced_field,
    ]
    mock_client.get_table.side_effect = [target_meta, temp_meta]

    mock_load_job = MagicMock()
    mock_client.load_table_from_json.return_value = mock_load_job
    mock_query_job = MagicMock()
    mock_query_job.num_dml_affected_rows = 1
    mock_client.query.return_value = mock_query_job

    records = [
        {
            "tenant_id": "tenant-a",
            "accountCD": "1500",
            "description": "Trade",
            "_source": "erp_rest",
            "_synced_at": "2026-05-01T10:00:00+00:00",
        }
    ]

    rows = merge(
        project="p",
        dataset="bronze",
        table="visma_accounts",
        primary_key="tenant_id,accountCD",
        records=records,
        run_id="run-1",
        endpoint="accounts",
        source="erp_rest",
        client=mock_client,
    )

    assert rows == 1
    assert mock_client.load_table_from_json.call_count == 2
    stage_records = mock_client.load_table_from_json.call_args_list[0].args[0]
    stage_table = mock_client.load_table_from_json.call_args_list[0].args[1]
    assert stage_table == "p.bronze.__vsv2_raw_stage"
    assert stage_records[0]["sync_run_id"] == "run-1"
    assert stage_records[0]["endpoint"] == "accounts"
    assert stage_records[0]["target_table"] == "visma_accounts"
    assert stage_records[0]["partition_key"] == "default"
    assert stage_records[0]["primary_key"] == "tenant-a\x1f1500"
    assert len(stage_records[0]["payload_hash"]) == 64
    assert '"accountCD":"1500"' in stage_records[0]["raw_payload_json"]


@pytest.mark.unit
def test_merge_rejects_duplicate_primary_key_with_different_payload() -> None:
    from vsv2_tooling.bigquery.loader import merge

    mock_client = MagicMock()
    tenant_field = MagicMock()
    tenant_field.name = "tenant_id"
    account_field = MagicMock()
    account_field.name = "accountCD"
    target_meta = MagicMock()
    target_meta.schema = [tenant_field, account_field]
    mock_client.get_table.return_value = target_meta

    with pytest.raises(ValueError, match="duplicate primary key"):
        merge(
            project="p",
            dataset="bronze",
            table="visma_accounts",
            primary_key="tenant_id,accountCD",
            records=[
                {"tenant_id": "tenant-a", "accountCD": "1500", "description": "Trade"},
                {"tenant_id": "tenant-a", "accountCD": "1500", "description": "Changed"},
            ],
            client=mock_client,
        )


@pytest.mark.unit
def test_write_quarantine_appends_rejected_rows() -> None:
    from vsv2_tooling.bigquery.loader import write_quarantine

    mock_client = MagicMock()
    mock_load_job = MagicMock()
    mock_client.load_table_from_json.return_value = mock_load_job

    rows = write_quarantine(
        project="p",
        dataset="bronze",
        records=[
            {
                "sync_run_id": "run-1",
                "tenant_id": "tenant-a",
                "endpoint": "accounts",
                "reasons_json": '["missing primary key"]',
                "raw_payload_json": '{"description":"Missing"}',
            }
        ],
        client=mock_client,
    )

    assert rows == 1
    args = mock_client.load_table_from_json.call_args.args
    assert args[1] == "p.bronze.__vsv2_quarantine"
