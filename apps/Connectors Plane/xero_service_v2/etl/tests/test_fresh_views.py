"""Execute the emitted SELECTs offline; no cloud clients or live writes.

SQLite supports the standard SQL subset used here. UTC timestamp fixtures use
fixed-width ISO strings, whose ordering matches BigQuery TIMESTAMP ordering.
These tests verify lifecycle results, not BigQuery deployment or query costs.
"""

import itertools
import re
import sqlite3

import pytest

from etl.xero.fresh_views import view_queries


PROJECT = "prj-dw-dev"
DATASET = "dw_staging_xero"
VIEW_NAMES = (
    "v2_records",
    "v2_collections",
    "v2_children",
    "v2_records_current",
    "v2_children_current",
)
IDENTITY = ("generation", "tenant_id", "endpoint", "record_id", "version_id")
SCHEMAS = {
    "v2_record_versions": IDENTITY
    + (
        "observed_at",
        "source_updated_at",
        "run_id",
        "receipt_id",
        "source_status",
        "source_deleted",
        "payload_hash",
        "payload_json",
    ),
    "v2_collection_versions": IDENTITY
    + (
        "collection_name",
        "observed_at",
        "source_updated_at",
        "child_count",
    ),
    "v2_child_versions": IDENTITY
    + (
        "collection_name",
        "child_key",
        "line_position",
        "native_id",
        "payload_hash",
        "payload_json",
    ),
}


def timestamp(day):
    return f"2026-01-{day:02d}T00:00:00.000000Z"


def record(version, revision=1, observed=None, **overrides):
    return {
        "generation": "g1",
        "tenant_id": "t1",
        "endpoint": "Invoices",
        "record_id": "r1",
        "version_id": version,
        "observed_at": timestamp(observed if observed is not None else revision),
        "source_updated_at": timestamp(revision) if revision is not None else None,
        "run_id": f"run-{version}",
        "receipt_id": f"receipt-{version}",
        "source_status": "AUTHORISED",
        "source_deleted": False,
        "payload_hash": f"hash-{version}",
        "payload_json": '{"Status":"AUTHORISED"}',
        **overrides,
    }


def collection(parent, keys=(), name="LineItems", **overrides):
    return {
        **{field: parent[field] for field in IDENTITY},
        "collection_name": name,
        "observed_at": parent["observed_at"],
        "source_updated_at": parent["source_updated_at"],
        "child_count": len(keys),
        **overrides,
    }


def child(parent, key="a", name="LineItems", **overrides):
    return {
        **{field: parent[field] for field in IDENTITY},
        "collection_name": name,
        "child_key": key,
        "line_position": 0,
        "native_id": key,
        "payload_hash": f"child-{parent['version_id']}-{key}",
        "payload_json": '{"Amount":12}',
        **overrides,
    }


@pytest.fixture
def evaluate():
    def run(records=(), collections=(), children=()):
        db = sqlite3.connect(":memory:")
        db.row_factory = sqlite3.Row
        try:
            batches = dict(zip(SCHEMAS, (records, collections, children), strict=True))
            for table, fields in SCHEMAS.items():
                # Test-only tables preserve the fully qualified, backticked names.
                columns = ", ".join(
                    f"{field} {'INTEGER' if field in ('source_deleted', 'child_count', 'line_position') else 'TEXT'}"
                    for field in fields
                )
                db.execute(f"CREATE TABLE `{PROJECT}.{DATASET}.{table}` ({columns})")
                placeholders = ", ".join("?" for _ in fields)
                db.executemany(
                    f"INSERT INTO `{PROJECT}.{DATASET}.{table}` VALUES ({placeholders})",
                    (tuple(row[field] for field in fields) for row in batches[table]),
                )
            queries = view_queries(PROJECT, DATASET)
            for name in VIEW_NAMES:
                db.execute(f"CREATE TEMP VIEW `{PROJECT}.{DATASET}.{name}` AS {queries[name]}")
            db.execute("PRAGMA query_only = ON")
            before = db.total_changes
            results = {
                name: tuple(dict(row) for row in db.execute(queries[name])) for name in VIEW_NAMES
            }
            assert db.total_changes == before
            return results
        finally:
            db.close()

    return run


def by_version(rows):
    return {row["version_id"]: row for row in rows}


def by_child(rows):
    return {(row["version_id"], row["child_key"]): row for row in rows}


def assert_flags(row, *, current=False, superseded=False, deleted=False, conflict=False):
    assert bool(row["is_current"]) is current
    assert bool(row["is_superseded"]) is superseded
    assert bool(row["is_deleted"]) is deleted
    assert bool(row["revision_conflict"]) is conflict


def test_api_is_five_select_statements_in_one_v2_namespace():
    queries = view_queries(PROJECT, DATASET)
    assert tuple(queries) == VIEW_NAMES
    allowed = set(VIEW_NAMES) | set(SCHEMAS)
    for sql in queries.values():
        assert re.match(r"\s*(WITH|SELECT)\b", sql, re.IGNORECASE)
        assert not re.search(
            r"\b(CREATE|ALTER|DROP|INSERT|UPDATE|DELETE|MERGE|TRUNCATE)\b", sql, re.I
        )
        assert ";" not in sql
        targets = re.findall(r"`([^`]+)`", sql)
        assert targets
        assert all(target.startswith(f"{PROJECT}.{DATASET}.") for target in targets)
        assert all(target.rsplit(".", 1)[-1] in allowed for target in targets)
        assert "v2_ingest_receipts" not in sql


@pytest.mark.parametrize(
    "project",
    [
        "",
        "1project",
        "-project",
        "prj_bad",
        "prj.other",
        "prj:other",
        "prj`",
        "prj;SELECT",
        "prj\n",
        " project",
        "p\u00e9",
        None,
        12,
    ],
)
def test_invalid_project_is_rejected(project):
    with pytest.raises(ValueError, match="project|identifier"):
        view_queries(project, DATASET)


@pytest.mark.parametrize(
    "dataset",
    [
        "",
        "1dataset",
        "data-set",
        "data.other",
        "data`",
        "data;DROP TABLE x",
        "data\n",
        " data",
        "d\u00e9",
        None,
        12,
    ],
)
def test_invalid_dataset_is_rejected(dataset):
    with pytest.raises(ValueError, match="dataset|identifier"):
        view_queries(PROJECT, dataset)


@pytest.mark.parametrize("project,dataset", [("A1-Z", "_d2"), ("p", "D"), (PROJECT, DATASET)])
def test_safe_identifiers_are_used_without_default_or_cross_dataset_references(project, dataset):
    queries = view_queries(project, dataset)
    assert all(
        target.startswith(f"{project}.{dataset}.v2_")
        for sql in queries.values()
        for target in re.findall(r"`([^`]+)`", sql)
    )
    assert queries == view_queries(project, dataset)
    assert queries is not view_queries(project, dataset)


def test_empty_history_returns_empty_views(evaluate):
    assert all(not rows for rows in evaluate().values())


def test_single_record_preserves_all_raw_fields_and_has_no_supersession(evaluate):
    parent = record("v1", payload_json='{"InvoiceNumber":"INV-01"}')
    result = evaluate((parent,))
    (row,) = result["v2_records"]
    assert {field: row[field] for field in SCHEMAS["v2_record_versions"]} == parent
    assert_flags(row, current=True)
    assert row["record_state"] == "current"
    assert row["superseded_by"] is None
    assert row["superseded_at"] is None
    assert result["v2_records_current"] == result["v2_records"]


@pytest.mark.parametrize("order", tuple(itertools.permutations(range(3))))
def test_source_revision_beats_arrival_order_and_lag_tracks_immediate_successor(evaluate, order):
    parents = (record("z-old", 1, 9), record("a-middle", 2, 3), record("m-new", 3, 4))
    result = evaluate(tuple(parents[index] for index in order))
    rows = by_version(result["v2_records"])
    assert_flags(rows["m-new"], current=True)
    assert_flags(rows["a-middle"], superseded=True)
    assert_flags(rows["z-old"], superseded=True)
    assert rows["z-old"]["record_state"] == "superseded"
    assert rows["z-old"]["superseded_by"] == "a-middle"
    assert rows["z-old"]["superseded_at"] == timestamp(3)
    assert rows["a-middle"]["superseded_by"] == "m-new"
    assert rows["a-middle"]["superseded_at"] == timestamp(4)
    assert tuple(row["version_id"] for row in result["v2_records_current"]) == ("m-new",)


def test_same_source_revision_uses_observation_before_version_id(evaluate):
    parents = (record("z", 1, 1), record("a", 1, 2))
    result = evaluate(parents)
    assert result["v2_records_current"][0]["version_id"] == "a"
    assert not any(row["revision_conflict"] for row in result["v2_records"])


def test_null_source_revision_falls_back_to_observation(evaluate):
    parents = (record("known", 2, 8), record("unknown", None, 3))
    result = evaluate(parents)
    assert result["v2_records_current"][0]["version_id"] == "unknown"


@pytest.mark.parametrize("order", [(0, 1), (1, 0)])
def test_identical_timestamp_and_hash_tie_is_stable_by_version_id(evaluate, order):
    parents = (record("a", payload_hash="same"), record("z", payload_hash="same"))
    result = evaluate(tuple(parents[index] for index in order))
    assert result["v2_records_current"][0]["version_id"] == "z"
    rows = by_version(result["v2_records"])
    assert_flags(rows["a"], superseded=True)
    assert rows["a"]["superseded_by"] == "z"


@pytest.mark.parametrize("order", tuple(itertools.permutations(range(3))))
def test_conflicting_latest_revision_never_promotes_a_tie_or_older_record(evaluate, order):
    parents = (record("old", 1), record("a", 2), record("z", 2))
    result = evaluate(tuple(parents[index] for index in order))
    assert result["v2_records_current"] == ()
    rows = by_version(result["v2_records"])
    assert_flags(rows["old"], superseded=True)
    for version in ("a", "z"):
        assert_flags(rows[version], superseded=version == "a", conflict=True)
        assert rows[version]["record_state"] == "conflict"


def test_conflicts_are_scoped_to_both_effective_and_observed_timestamps(evaluate):
    parents = (record("a", 1, 3), record("b", 2, 3), record("c", 2, 4))
    result = evaluate(parents)
    assert not any(row["revision_conflict"] for row in result["v2_records"])
    assert result["v2_records_current"][0]["version_id"] == "c"


def test_null_and_explicit_source_revision_with_same_effective_time_can_conflict(evaluate):
    result = evaluate((record("a", None, 2), record("b", 2, 2)))
    assert all(row["revision_conflict"] for row in result["v2_records"])
    assert result["v2_records_current"] == ()


def test_later_unambiguous_revision_resolves_current_but_preserves_historical_conflicts(evaluate):
    result = evaluate((record("a", 1), record("z", 1), record("later", 2)))
    rows = by_version(result["v2_records"])
    assert rows["a"]["record_state"] == rows["z"]["record_state"] == "conflict"
    assert_flags(rows["later"], current=True)
    assert result["v2_records_current"][0]["version_id"] == "later"


@pytest.mark.parametrize("status", ["AUTHORISED", "VOIDED", "ARCHIVED", None])
def test_status_or_absence_does_not_infer_source_deletion(evaluate, status):
    result = evaluate((record("v", source_status=status),))
    assert_flags(result["v2_records_current"][0], current=True)
    assert not result["v2_records_current"][0]["source_deleted"]


def test_current_records_include_explicit_tombstones_and_keep_history(evaluate):
    parents = (
        record("active", 1),
        record(
            "deleted",
            2,
            source_deleted=True,
            source_status="DELETED",
            payload_json='{"Status":"DELETED"}',
        ),
    )
    result = evaluate(parents)
    assert len(result["v2_records"]) == 2
    (row,) = result["v2_records_current"]
    assert_flags(row, current=True, deleted=True)
    assert row["record_state"] == "source_deleted"
    assert row["source_deleted"]


@pytest.mark.parametrize("identity_field", ["generation", "tenant_id", "endpoint", "record_id"])
def test_record_and_child_lifecycles_are_isolated_by_full_identity(evaluate, identity_field):
    old = record("old", 1)
    other = record("other", 2, **{identity_field: "other"})
    result = evaluate(
        (old, other),
        (collection(old, ("a",)), collection(other)),
        (child(old),),
    )
    assert len(result["v2_records_current"]) == 2
    assert len(result["v2_collections"]) == 2
    assert all(row["is_current"] for row in result["v2_collections"])
    assert_flags(result["v2_children_current"][0], current=True)


def test_child_joins_its_own_version_metadata_and_preserves_raw_fields(evaluate):
    old, new = record("old", 1), record("new", 2)
    raw = child(old, line_position=7, native_id=None)
    result = evaluate((old, new), (collection(old, ("a",)),), (raw,))
    (row,) = result["v2_children"]
    assert {field: row[field] for field in SCHEMAS["v2_child_versions"]} == raw
    assert row["observed_at"] == old["observed_at"]
    assert row["source_updated_at"] == old["source_updated_at"]
    assert_flags(row, current=True)


@pytest.mark.parametrize("order", [(0, 1), (1, 0)])
def test_new_complete_collection_supersedes_and_removes_only_absent_logical_keys(evaluate, order):
    parents = (record("old", 1, 9), record("new", 2, 3))
    collections = (collection(parents[0], ("a", "b")), collection(parents[1], ("a", "c")))
    children = (
        child(parents[0], "a"),
        child(parents[0], "b"),
        child(parents[1], "a"),
        child(parents[1], "c"),
    )
    result = evaluate(
        tuple(parents[index] for index in order),
        tuple(collections[index] for index in order),
        tuple(reversed(children)),
    )
    rows = by_child(result["v2_children"])
    assert_flags(rows[("old", "a")], superseded=True)
    assert not rows[("old", "a")]["is_removed"]
    assert rows[("old", "a")]["record_state"] == "superseded"
    removed = rows[("old", "b")]
    assert_flags(removed, superseded=True, deleted=True)
    assert removed["is_removed"]
    assert not removed["source_deleted"]
    assert removed["deletion_reason"] == "missing_from_complete_parent"
    assert removed["record_state"] == "removed"
    assert {(row["version_id"], row["child_key"]) for row in result["v2_children_current"]} == {
        ("new", "a"),
        ("new", "c"),
    }
    assert len(result["v2_children"]) == len(children)


def test_complete_empty_array_keeps_header_and_history_but_clears_child_current(evaluate):
    old, empty = record("old", 1), record("empty", 2)
    result = evaluate(
        (old, empty),
        (collection(old, ("a",)), collection(empty)),
        (child(old),),
    )
    headers = by_version(result["v2_collections"])
    assert len(headers) == 2
    assert headers["empty"]["child_count"] == 0
    assert_flags(headers["empty"], current=True)
    assert_flags(headers["old"], superseded=True)
    assert headers["old"]["superseded_by"] == "empty"
    assert headers["old"]["superseded_at"] == timestamp(2)
    assert result["v2_children_current"] == ()
    assert_flags(result["v2_children"][0], superseded=True, deleted=True)
    assert result["v2_children"][0]["record_state"] == "removed"


def test_omitted_or_null_collections_leave_known_arrays_current(evaluate):
    old, omitted = record("old", 1), record("omitted", 2, payload_json='{"LineItems":null}')
    result = evaluate((old, omitted), (collection(old, ("a",)),), (child(old),))
    assert_flags(result["v2_collections"][0], current=True)
    assert_flags(result["v2_children"][0], current=True)
    assert not result["v2_children"][0]["is_removed"]
    assert result["v2_records_current"][0]["version_id"] == "omitted"
    assert result["v2_children_current"][0]["version_id"] == "old"


def test_collection_order_uses_observation_and_version_tiebreak_like_records(evaluate):
    parents = (
        record("old", 1, 1),
        record("a", 1, 2, payload_hash="same"),
        record("z", 1, 2, payload_hash="same"),
    )
    result = evaluate(
        parents,
        tuple(collection(parent, ("a",)) for parent in parents),
        tuple(child(parent) for parent in parents),
    )
    headers = by_version(result["v2_collections"])
    assert_flags(headers["z"], current=True)
    assert headers["old"]["superseded_by"] == "a"
    assert headers["a"]["superseded_by"] == "z"
    assert result["v2_children_current"][0]["version_id"] == "z"


def test_collection_order_falls_back_to_observation_for_null_source_revision(evaluate):
    parents = (record("known", 2, 8), record("unknown", None, 3))
    result = evaluate(
        parents,
        tuple(collection(parent, ("a",)) for parent in parents),
        tuple(child(parent) for parent in parents),
    )
    assert result["v2_children_current"][0]["version_id"] == "unknown"


def test_collection_names_and_membership_do_not_cross_contaminate(evaluate):
    old, new = record("old", 1), record("new", 2)
    result = evaluate(
        (old, new),
        (collection(old, ("a",)), collection(old, ("a",), "Addresses"), collection(new)),
        (child(old), child(old, name="Addresses")),
    )
    rows = {row["collection_name"]: row for row in result["v2_children"]}
    assert rows["LineItems"]["is_removed"]
    assert_flags(rows["Addresses"], current=True)
    assert result["v2_children_current"][0]["collection_name"] == "Addresses"


def test_removal_uses_child_key_not_position_native_id_or_payload(evaluate):
    old, new = record("old", 1), record("new", 2)
    result = evaluate(
        (old, new),
        (collection(old, ("a",)), collection(new, ("a",))),
        (
            child(old, native_id="before", line_position=4),
            child(new, native_id="after", line_position=0),
        ),
    )
    historical = by_child(result["v2_children"])[("old", "a")]
    assert_flags(historical, superseded=True)
    assert not historical["is_removed"]
    assert historical["deletion_reason"] is None


@pytest.mark.parametrize("has_deleted_collection", [False, True])
def test_latest_explicit_parent_deletion_suppresses_children_without_dropping_history(
    evaluate,
    has_deleted_collection,
):
    old = record("old", 1)
    deleted = record("deleted", 2, source_deleted=True, source_status="DELETED")
    collections = (collection(old, ("a",)),) + (
        (collection(deleted),) if has_deleted_collection else ()
    )
    result = evaluate((old, deleted), collections, (child(old),))
    assert result["v2_children_current"] == ()
    assert len(result["v2_collections"]) == len(collections)
    assert all(row["is_deleted"] for row in result["v2_collections"])
    (row,) = result["v2_children"]
    assert_flags(row, superseded=has_deleted_collection, deleted=True)
    assert row["source_deleted"]
    assert row["record_state"] == "source_deleted"
    assert row["deletion_reason"] == "source_deleted"


def test_restored_parent_can_reactivate_known_array_after_omitted_headers(evaluate):
    old = record("old", 1)
    deleted = record("deleted", 2, source_deleted=True, source_status="DELETED")
    restored = record("restored", 3)
    result = evaluate((old, deleted, restored), (collection(old, ("a",)),), (child(old),))
    assert_flags(result["v2_children_current"][0], current=True)
    assert result["v2_children_current"][0]["deletion_reason"] is None
    assert by_version(result["v2_records"])["deleted"]["is_deleted"]


def test_removed_key_reappearing_is_superseded_not_still_removed(evaluate):
    old, empty, restored = record("old", 1), record("empty", 2), record("restored", 3)
    result = evaluate(
        (old, empty, restored),
        (collection(old, ("a",)), collection(empty), collection(restored, ("a",))),
        (child(old), child(restored)),
    )
    historical = by_child(result["v2_children"])[("old", "a")]
    assert_flags(historical, superseded=True)
    assert not historical["is_removed"]
    assert historical["deletion_reason"] is None
    assert result["v2_children_current"][0]["version_id"] == "restored"


@pytest.mark.parametrize("deleted_tie", ["a", "z", None])
def test_conflicting_latest_parent_cannot_promote_remove_or_arbitrarily_delete_children(
    evaluate,
    deleted_tie,
):
    old = record("old", 1)
    ties = tuple(
        record(
            version,
            2,
            source_deleted=version == deleted_tie,
            source_status="DELETED" if version == deleted_tie else "AUTHORISED",
        )
        for version in ("a", "z")
    )
    result = evaluate((old, *ties), (collection(old, ("a",)),), (child(old),))
    assert result["v2_records_current"] == result["v2_children_current"] == ()
    assert_flags(result["v2_collections"][0], conflict=True)
    (row,) = result["v2_children"]
    assert_flags(row, conflict=True)
    assert not row["is_removed"]
    assert not row["source_deleted"]
    assert row["deletion_reason"] is None
    assert row["record_state"] == "conflict"


@pytest.mark.parametrize("later_header", [False, True])
def test_conflicting_complete_collection_never_clears_or_promotes_known_children(
    evaluate,
    later_header,
):
    old, a, z = record("old", 1), record("a", 2), record("z", 2)
    parents = (old, a, z) + ((record("later", 3),) if later_header else ())
    result = evaluate(
        parents,
        (collection(old, ("a",)), collection(a, ("b",)), collection(z)),
        (child(old), child(a, "b")),
    )
    assert result["v2_children_current"] == ()
    assert len(result["v2_children"]) == 2
    assert not any(row["is_removed"] or row["is_deleted"] for row in result["v2_children"])
    assert all(row["record_state"] == "conflict" for row in result["v2_children"])
    assert not any(row["is_current"] for row in result["v2_collections"])


def test_later_complete_collection_resolves_conflict_without_erasing_historical_flags(evaluate):
    a, z, later = record("a", 1), record("z", 1), record("later", 2)
    result = evaluate(
        (a, z, later),
        tuple(collection(parent, ("a",)) for parent in (a, z, later)),
        tuple(child(parent) for parent in (a, z, later)),
    )
    assert_flags(result["v2_children_current"][0], current=True)
    assert result["v2_children_current"][0]["version_id"] == "later"
    rows = by_child(result["v2_children"])
    assert rows[("a", "a")]["record_state"] == "conflict"
    assert rows[("z", "a")]["record_state"] == "conflict"


def test_orphan_history_is_retained_but_never_promoted_or_inferred_deleted(evaluate):
    orphan = record("orphan")
    result = evaluate((), (collection(orphan, ("a",)),), (child(orphan),))
    assert len(result["v2_collections"]) == len(result["v2_children"]) == 1
    assert result["v2_children_current"] == ()
    assert not result["v2_children"][0]["is_deleted"]


def test_child_without_own_collection_is_retained_but_cannot_be_current(evaluate):
    parent = record("v")
    result = evaluate((parent,), (), (child(parent),))
    assert len(result["v2_children"]) == 1
    assert result["v2_children_current"] == ()
    assert not result["v2_children"][0]["is_removed"]


def test_different_version_same_hash_is_still_distinct_child_history(evaluate):
    old, new = record("old", 1), record("new", 2)
    result = evaluate(
        (old, new),
        (collection(old, ("a",)), collection(new, ("a",))),
        (child(old, payload_hash="same"), child(new, payload_hash="same")),
    )
    assert len(result["v2_children"]) == 2
    assert result["v2_children_current"][0]["version_id"] == "new"


def test_late_arriving_old_tombstone_and_empty_array_do_not_delete_newer_children(evaluate):
    deleted = record("deleted", 1, 9, source_deleted=True, source_status="DELETED")
    active = record("active", 2, 3)
    result = evaluate(
        (active, deleted),
        (collection(active, ("a",)), collection(deleted)),
        (child(active),),
    )
    assert_flags(by_version(result["v2_records"])["deleted"], superseded=True, deleted=True)
    assert_flags(result["v2_records_current"][0], current=True)
    assert_flags(result["v2_children_current"][0], current=True)
    assert not result["v2_children_current"][0]["source_deleted"]


def test_timestamp_precision_is_used_for_ordering_and_conflict_detection(evaluate):
    early = record("z", 1, observed_at="2026-01-01T00:00:00.000001Z")
    later = record("a", 1, observed_at="2026-01-01T00:00:00.000002Z")
    result = evaluate((later, early))
    assert result["v2_records_current"][0]["version_id"] == "a"
    assert not any(row["revision_conflict"] for row in result["v2_records"])


def test_membership_join_does_not_multiply_history_when_keys_repeat(evaluate):
    old, new = record("old", 1), record("new", 2)
    children = (child(old), child(new), child(new, line_position=1))
    result = evaluate(
        (old, new),
        (collection(old, ("a",)), collection(new, ("a", "a"))),
        children,
    )
    assert len(result["v2_children"]) == len(children)
    assert len(result["v2_children_current"]) == 2
    assert not any(row["is_removed"] for row in result["v2_children"])


def test_duplicate_logical_headers_with_unique_occurrence_versions_do_not_multiply_joins(evaluate):
    chosen_version = "occurrence-0002"
    for divergent_hash in (False, True):
        parents = tuple(
            record(version, payload_hash=payload_hash, run_id="run", receipt_id="receipt")
            for version, payload_hash in (
                ("occurrence-0001", "same"),
                (chosen_version, "different" if divergent_hash else "same"),
            )
        )
        for order in ((0, 1), (1, 0)):
            ordered_parents = tuple(parents[index] for index in order)
            headers = tuple(collection(parent, ("a", "b")) for parent in ordered_parents)
            children = tuple(
                child(parent, key, line_position=position, payload_hash=f"child-{key}")
                for parent in ordered_parents
                for position, key in enumerate(("a", "b"))
            )
            result = evaluate(ordered_parents, headers, children)
            for name, raw_rows, fields in (
                ("v2_records", parents, SCHEMAS["v2_record_versions"]),
                ("v2_collections", headers, SCHEMAS["v2_collection_versions"]),
                ("v2_children", children, SCHEMAS["v2_child_versions"]),
            ):
                assert len(result[name]) == len(raw_rows)
                assert {tuple(row[field] for field in fields) for row in result[name]} == {
                    tuple(row[field] for field in fields) for row in raw_rows
                }
                for row in result[name]:
                    is_chosen = row["version_id"] == chosen_version
                    assert_flags(
                        row,
                        current=is_chosen and not divergent_hash,
                        superseded=not is_chosen,
                        conflict=divergent_hash,
                    )
                    assert row["record_state"] == (
                        "conflict" if divergent_hash else "current" if is_chosen else "superseded"
                    )
            for row in result["v2_children"]:
                assert not row["is_removed"]
                assert row["deletion_reason"] is None
            assert tuple(row["version_id"] for row in result["v2_records_current"]) == (
                () if divergent_hash else (chosen_version,)
            )
            assert {
                (row["version_id"], row["child_key"]) for row in result["v2_children_current"]
            } == (set() if divergent_hash else {(chosen_version, "a"), (chosen_version, "b")})
            assert len(result["v2_children_current"]) == (0 if divergent_hash else 2)


@pytest.mark.parametrize("stage", range(6))
def test_append_only_lifecycle_end_to_end_keeps_all_history(evaluate, stage):
    parents = tuple(
        record(
            f"v{index}",
            index + 1,
            source_deleted=index == 4,
            source_status="DELETED" if index == 4 else "AUTHORISED",
        )
        for index in range(6)
    )
    all_headers = (
        collection(parents[0], ("a",)),
        collection(parents[2]),
        collection(parents[3], ("a",)),
        collection(parents[5], ("b",)),
    )
    all_children = (child(parents[0]), child(parents[3]), child(parents[5], "b"))
    versions = {parent["version_id"] for parent in parents[: stage + 1]}
    headers = tuple(row for row in all_headers if row["version_id"] in versions)
    children = tuple(row for row in all_children if row["version_id"] in versions)
    result = evaluate(parents[: stage + 1], headers, children)
    assert len(result["v2_records"]) == stage + 1
    assert len(result["v2_collections"]) == len(headers)
    assert len(result["v2_children"]) == len(children)
    (current,) = result["v2_records_current"]
    assert current["version_id"] == parents[stage]["version_id"]
    assert bool(current["is_deleted"]) is (stage == 4)
    expected_children = (
        (("v0", "a"),),
        (("v0", "a"),),
        (),
        (("v3", "a"),),
        (),
        (("v5", "b"),),
    )
    assert (
        tuple((row["version_id"], row["child_key"]) for row in result["v2_children_current"])
        == expected_children[stage]
    )
    for name, rows in result.items():
        for row in rows:
            for flag in ("is_current", "is_superseded", "is_deleted", "revision_conflict"):
                assert row[flag] in (0, 1), (name, flag, row)
