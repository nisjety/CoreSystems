"""The staging layer's row building and MERGE, without BigQuery."""

from __future__ import annotations

from datetime import UTC, datetime, timedelta
from unittest.mock import MagicMock

import pytest
from google.api_core.exceptions import NotFound

from vsv2_tooling.bigquery import staging as staging_module
from vsv2_tooling.bigquery.staging import (
    STAGING_SCHEMA,
    VOLATILE_FIELDS,
    build_stage_rows,
    deletion_allowed,
    ensure_dataset,
    mark_missing_sql,
    merge_sql,
    missing_count_sql,
    stage,
)

NOW = datetime(2026, 10, 1, 12, 0, tzinfo=UTC)
KEY = ["tenant_id", "referenceNumber"]


def record(number: str, **extra) -> dict:
    return {
        "tenant_id": "t1",
        "referenceNumber": number,
        "_source": "erp_rest",
        "_synced_at": "2026-10-01T10:00:00Z",
        **extra,
    }


@pytest.mark.unit
def test_a_row_carries_identity_payload_hash_and_fetch_time() -> None:
    rows, dropped = build_stage_rows([record("1", amount=5)], KEY, source="x", now=NOW)

    assert dropped == 0
    row = rows[0]
    assert (row["tenant_id"], row["source"]) == ("t1", "erp_rest")
    assert row["record_key"] == "t1\x1f1"
    assert row["fetched_at"] == "2026-10-01T10:00:00Z"
    assert row["payload"]["amount"] == 5
    assert len(row["payload_hash"]) == 64


@pytest.mark.unit
def test_volatile_fields_are_not_in_the_payload_or_the_hash() -> None:
    first = record("1", _page_index=0)
    later = record("1", _page_index=7)
    later["_synced_at"] = "2026-10-02T10:00:00Z"

    a, _ = build_stage_rows([first], KEY, source="x", now=NOW)
    b, _ = build_stage_rows([later], KEY, source="x", now=NOW)

    assert not VOLATILE_FIELDS & set(a[0]["payload"])
    assert a[0]["payload_hash"] == b[0]["payload_hash"], "a re-fetch must not look like a change"


@pytest.mark.unit
def test_the_paging_block_every_erp_row_carries_is_kept_but_does_not_make_a_change() -> None:
    """One new invoice raises `totalCount` in every other invoice's row."""
    before = record("1", amount=5, metadata={"totalCount": 21_000, "maxPageSize": 1000})
    after = record("1", amount=5, metadata={"totalCount": 21_001, "maxPageSize": 1000})

    a, _ = build_stage_rows([before], KEY, source="x", now=NOW)
    b, _ = build_stage_rows([after], KEY, source="x", now=NOW)

    assert a[0]["payload"]["metadata"] == {"totalCount": 21_000, "maxPageSize": 1000}
    assert a[0]["payload_hash"] == b[0]["payload_hash"]


@pytest.mark.unit
def test_a_record_with_and_without_the_paging_block_is_the_same_record() -> None:
    with_block = record("1", amount=5, metadata={"totalCount": 3, "maxPageSize": 100})
    without = record("1", amount=5)

    a, _ = build_stage_rows([with_block], KEY, source="x", now=NOW)
    b, _ = build_stage_rows([without], KEY, source="x", now=NOW)

    assert a[0]["payload_hash"] == b[0]["payload_hash"]


@pytest.mark.unit
def test_a_changed_field_or_a_metadata_that_is_not_the_paging_block_still_counts() -> None:
    base = record("1", amount=5, metadata={"totalCount": 3, "maxPageSize": 100})
    changed = record("1", amount=6, metadata={"totalCount": 3, "maxPageSize": 100})
    other = record("1", amount=5, metadata={"totalCount": 3, "source": "import"})
    other_changed = record("1", amount=5, metadata={"totalCount": 3, "source": "manual"})

    hashes = [
        build_stage_rows([r], KEY, source="x", now=NOW)[0][0]["payload_hash"]
        for r in (base, changed, other, other_changed)
    ]

    assert hashes[0] != hashes[1], "a real field change is a change"
    assert hashes[2] != hashes[3], "metadata with fields of its own is data, not paging"


@pytest.mark.unit
def test_an_exact_duplicate_is_dropped_and_a_conflicting_one_is_an_error() -> None:
    rows, dropped = build_stage_rows([record("1"), record("1")], KEY, source="x", now=NOW)
    assert (len(rows), dropped) == (1, 1)

    with pytest.raises(ValueError, match="duplicate primary key with different payload"):
        build_stage_rows([record("1", amount=1), record("1", amount=2)], KEY, source="x", now=NOW)


@pytest.mark.unit
def test_records_without_a_tenant_are_refused() -> None:
    bad = record("1")
    del bad["tenant_id"]

    with pytest.raises(ValueError, match="tenant_id"):
        build_stage_rows([bad], ["referenceNumber"], source="x", now=NOW)


@pytest.mark.unit
def test_a_missing_fetch_time_falls_back_to_now_and_a_missing_source_to_the_argument() -> None:
    bare = {"tenant_id": "t1", "referenceNumber": "1"}

    rows, _ = build_stage_rows([bare], KEY, source="datamart_graphql", now=NOW)

    assert rows[0]["fetched_at"] == NOW.isoformat()
    assert rows[0]["source"] == "datamart_graphql"


@pytest.mark.unit
def test_nested_values_and_big_integers_survive_into_the_payload() -> None:
    lines = [{"n": 1, "project": {"id": "10773"}}]
    rows, _ = build_stage_rows(
        [record("1", lines=lines, id=9007199254740993)], KEY, source="x", now=NOW
    )

    assert rows[0]["payload"]["lines"] == lines
    assert rows[0]["payload"]["id"] == 9007199254740993


@pytest.mark.unit
def test_the_merge_writes_only_new_changed_tombstoned_and_returning_rows() -> None:
    sql = merge_sql("p.d.accounts", "p.d.__stage_accounts_ab12")

    assert "ON T.tenant_id = S.tenant_id AND T.record_key = S.record_key" in sql
    assert "WHEN MATCHED AND T.payload_hash != S.payload_hash" in sql
    assert "WHEN MATCHED AND S.is_deleted AND T.deleted_at IS NULL" in sql
    assert "WHEN MATCHED AND NOT S.is_deleted AND T.deleted_at IS NOT NULL" in sql
    assert "WHEN NOT MATCHED THEN" in sql
    assert "IF(S.is_deleted, S.fetched_at, NULL)" in sql, "a new tombstone arrives deleted"
    assert "last_seen" not in sql, "an unchanged row must not be rewritten on every sync"


@pytest.mark.unit
def test_a_datamart_tombstone_is_flagged_on_its_row() -> None:
    live = record("1")
    gone = record("2", deletedDatabaseRecord=True)

    rows, _ = build_stage_rows([live, gone], KEY, source="x", now=NOW)

    assert [r["is_deleted"] for r in rows] == [False, True]
    assert rows[1]["payload"]["deletedDatabaseRecord"] is True, "the payload stays as it arrived"


@pytest.mark.unit
def test_missing_rows_are_found_and_marked_for_the_tenant_only() -> None:
    count = missing_count_sql("p.d.accounts", "p.d.tmp")
    update = mark_missing_sql("p.d.accounts", "p.d.tmp")

    for sql in (count, update):
        assert "T.tenant_id = @tenant" in sql, "another tenant's rows are never candidates"
        assert "T.deleted_at IS NULL" in sql, "a row already deleted is not deleted again"
    assert "NOT EXISTS" in update and "S.record_key = T.record_key" in update
    assert "SET deleted_at = CURRENT_TIMESTAMP()" in update


@pytest.mark.unit
@pytest.mark.parametrize(
    ("live", "missing", "allowed"),
    [
        (10_000, 0, True),  # nothing missing
        (10_000, 2_000, True),  # a fifth is the limit
        (10_000, 2_001, False),  # more than a fifth: a partial answer looks like this
        (100, 40, True),  # small tables may change freely up to the floor
        (100, 60, False),
        (30, 30, True),  # all of a table under the floor
        (0, 0, True),
    ],
)
def test_a_complete_scan_may_not_delete_most_of_a_tenant(live, missing, allowed) -> None:
    assert deletion_allowed(live, missing) is allowed


@pytest.mark.unit
def test_force_overrides_the_guard() -> None:
    assert deletion_allowed(10_000, 9_000, force=True) is True


@pytest.mark.unit
def test_a_complete_scan_needs_to_know_which_tenant_it_is_complete_for() -> None:
    with pytest.raises(ValueError, match="tenant"):
        stage(
            project="p",
            dataset="d",
            table="accounts",
            primary_key="tenant_id,referenceNumber",
            records=[],
            run_id="r",
            endpoint="accounts",
            source="erp_rest",
            complete=True,
            client=object(),  # type: ignore[arg-type]  # never reached
        )


@pytest.mark.unit
def test_the_staging_schema_has_the_columns_the_merge_writes() -> None:
    names = [field.name for field in STAGING_SCHEMA]

    assert names == [
        "tenant_id",
        "record_key",
        "source",
        "payload",
        "payload_hash",
        "first_seen_at",
        "updated_at",
        "last_run_id",
        "deleted_at",
    ]
    assert {f.name: f.field_type for f in STAGING_SCHEMA}["payload"] == "JSON"


@pytest.mark.unit
def test_a_missing_dataset_is_created_in_the_wanted_location() -> None:
    client = MagicMock()
    client.get_dataset.side_effect = NotFound("no such dataset")

    ensure_dataset(client, "proj", "dw_staging_visma", "europe-north2")

    (dataset,), _ = client.create_dataset.call_args
    assert (dataset.project, dataset.dataset_id, dataset.location) == (
        "proj",
        "dw_staging_visma",
        "europe-north2",
    )


@pytest.mark.unit
def test_an_existing_dataset_is_left_alone_whatever_its_location() -> None:
    client = MagicMock()

    ensure_dataset(client, "proj", "dw_staging_visma", "europe-north2")

    client.get_dataset.assert_called_once()
    client.create_dataset.assert_not_called()


@pytest.mark.unit
def test_a_temporary_table_expires_a_day_after_it_is_created_even_if_the_step_waited(
    monkeypatch,
) -> None:
    """A step that waits hours for its records must not give BigQuery an expiry already in the past.

    The step reads the clock when it starts and creates its temporary table three hours later.
    """
    clock = [NOW, NOW + timedelta(hours=3)]

    class Clock(datetime):
        @classmethod
        def now(cls, tz=None):
            return clock.pop(0) if len(clock) > 1 else clock[0]

    monkeypatch.setattr(staging_module, "datetime", Clock)
    client = MagicMock()

    stage(
        project="proj",
        dataset="dw_staging_visma",
        table="branches",
        primary_key="referenceNumber",
        records=[record("1")],
        run_id="run-1",
        endpoint="branches",
        source="erp_rest",
        tenant="t1",
        client=client,
    )

    (temp,), _ = client.create_table.call_args
    assert temp.expires == NOW + timedelta(hours=3) + staging_module.TEMP_TABLE_TTL
