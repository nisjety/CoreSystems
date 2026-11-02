from types import SimpleNamespace
from unittest.mock import MagicMock
from uuid import uuid4

import pytest

from etl.xero.fresh_backfill import (
    backfill_manifests,
    backfill_raw_observations,
    canonical_run_id,
    committed_sidecars,
    manifest_sidecars,
    raw_sidecars,
    replay_current_projections,
)
from etl.xero.fresh_ingestion import PreparedFreshObservation


GENERATION = "a3e6e7e5-04c1-4574-a63f-e0863ea9cef1"
PREFIX = f"xero/{GENERATION}/raw/xero/"
MANIFEST_PREFIX = f"xero/{GENERATION}/_manifests/xero/"


def _prepared(receipt_id, *, layout="warehouse", dataset="dw_staging_xero", is_manifest=False):
    return PreparedFreshObservation(
        layout=layout,
        target_project="aquatiq-datavarehus",
        dataset=dataset,
        batches={
            "v2_record_versions": [{"receipt_id": receipt_id}],
            "v2_ingest_receipts": [{"receipt_id": receipt_id}],
        },
        is_manifest=is_manifest,
    )


def test_backfill_groups_pinned_raw_sidecars_into_bounded_idempotent_batches(monkeypatch):
    client = MagicMock()
    client.list_blobs.return_value = [
        SimpleNamespace(name=f"{PREFIX}tenant/2.0/assets_asset_details/2026-10-16/a.json.meta.json", generation=11),
        SimpleNamespace(name=f"{PREFIX}tenant/2.0/assets_asset_details/2026-10-16/a.json", generation=12),
        SimpleNamespace(name=f"{PREFIX}tenant/2.0/assets_asset_details/2026-10-16/b.json.meta.json", generation=13),
    ]
    prepared = iter([_prepared("one"), _prepared("two")])
    monkeypatch.setattr(
        "etl.xero.fresh_backfill.prepare_fresh_observation",
        lambda *_: next(prepared),
    )
    writer = MagicMock()

    result = backfill_raw_observations(
        client, writer, object(), bucket="aquatiq-datavarehus-xero-raw",
        project="aquatiq-datavarehus", prefix=PREFIX, batch_size=2,
    )

    assert result == {"objects": 2, "batches": 1}
    writer.publish_observations_many.assert_called_once()
    published = writer.publish_observations_many.call_args.args[0]
    assert [item["v2_ingest_receipts"][0]["receipt_id"] for item in published] == ["one", "two"]


def test_backfill_rejects_a_nonwarehouse_or_manifest_observation(monkeypatch):
    client = MagicMock()
    client.list_blobs.return_value = [
        SimpleNamespace(name=f"{PREFIX}tenant/2.0/assets_asset_details/2026-10-16/a.json.meta.json", generation=11),
    ]
    monkeypatch.setattr(
        "etl.xero.fresh_backfill.prepare_fresh_observation",
        lambda *_: _prepared("one", layout="legacy"),
    )

    with pytest.raises(ValueError, match="warehouse raw layout"):
        backfill_raw_observations(
            client, MagicMock(), object(), bucket="aquatiq-datavarehus-xero-raw",
            project="aquatiq-datavarehus", prefix=PREFIX, batch_size=1,
        )


def test_raw_backfill_can_replay_one_exact_run_without_touching_other_sidecars(monkeypatch):
    requested_run_id = str(uuid4())
    other_run_id = str(uuid4())
    requested_tag = requested_run_id.replace("-", "")[:6]
    other_tag = other_run_id.replace("-", "")[:6]
    client = MagicMock()
    client.list_blobs.return_value = [
        SimpleNamespace(
            name=f"{PREFIX}tenant/2.0/assets_asset_details/2026-10-16/a_{requested_tag}_p001.json.meta.json",
            generation=11,
        ),
        SimpleNamespace(
            name=f"{PREFIX}tenant/2.0/assets_asset_details/2026-10-16/a_{other_tag}_p001.json.meta.json",
            generation=12,
        ),
    ]
    prepared = _prepared("one")
    prepared.batches["v2_ingest_receipts"][0]["run_id"] = requested_run_id
    prepare = MagicMock(return_value=prepared)
    monkeypatch.setattr("etl.xero.fresh_backfill.prepare_fresh_observation", prepare)
    writer = MagicMock()

    result = backfill_raw_observations(
        client, writer, object(), bucket="aquatiq-datavarehus-xero-raw",
        project="aquatiq-datavarehus", prefix=PREFIX, batch_size=1,
        run_id=requested_run_id,
    )

    assert result == {"objects": 1, "batches": 1}
    assert prepare.call_count == 1
    writer.publish_observations_many.assert_called_once()


def test_raw_backfill_skips_only_the_exact_committed_sidecar_generation(monkeypatch):
    run_id = str(uuid4())
    run_tag = run_id.replace("-", "")[:6]
    name = f"{PREFIX}tenant/2.0/invoices/2026-10-18/a_{run_tag}_p001.json.meta.json"
    client = MagicMock()
    client.list_blobs.return_value = [
        SimpleNamespace(name=name, generation=11),
        SimpleNamespace(name=name, generation=12),
    ]
    prepared = _prepared("new-generation")
    prepared.batches["v2_ingest_receipts"][0]["run_id"] = run_id
    prepare = MagicMock(return_value=prepared)
    monkeypatch.setattr("etl.xero.fresh_backfill.prepare_fresh_observation", prepare)
    writer = MagicMock()

    result = backfill_raw_observations(
        client, writer, object(), bucket="aquatiq-datavarehus-xero-raw",
        project="aquatiq-datavarehus", prefix=PREFIX, batch_size=50,
        run_id=run_id, received_sidecars=frozenset({(name, "11")}),
    )

    assert result == {"objects": 1, "batches": 1}
    assert prepare.call_count == 1
    assert prepare.call_args.args[3] == "12"
    writer.publish_observations_many.assert_called_once()


def test_committed_sidecars_uses_bound_run_id():
    run_id = str(uuid4())
    writer = MagicMock()
    writer.client.query.return_value.result.return_value = [
        {"source_object": "raw/page.meta.json", "meta_generation": "123"},
    ]

    assert committed_sidecars(writer, "aquatiq-datavarehus", run_id) == frozenset({
        ("raw/page.meta.json", "123"),
    })
    job_config = writer.client.query.call_args.kwargs["job_config"]
    assert job_config.query_parameters[0].value == run_id
    assert "@run_id" in writer.client.query.call_args.args[0]


def test_current_projection_replay_uses_pinned_records_for_one_exact_run(monkeypatch):
    requested_run_id = str(uuid4())
    run_tag = requested_run_id.replace("-", "")[:6]
    client = MagicMock()
    object_name = (
        f"{PREFIX}tenant/2.0/accounting_get_getaccount/2026-10-16/"
        f"account_{run_tag}_p001.json.meta.json"
    )
    client.list_blobs.return_value = [SimpleNamespace(name=object_name, generation=11)]
    prepared = _prepared("one")
    prepared.batches["v2_ingest_receipts"][0]["run_id"] = requested_run_id
    prepared = PreparedFreshObservation(
        layout=prepared.layout,
        target_project=prepared.target_project,
        dataset=prepared.dataset,
        batches=prepared.batches,
        is_manifest=False,
        records=({"record_id": "account-1"},),
    )
    projection = SimpleNamespace(batch_key=(
        "aquatiq-datavarehus", "dw_staging_xero", "xero", "accounting_get_getaccount",
    ))
    monkeypatch.setattr(
        "etl.xero.fresh_backfill.prepare_fresh_observation",
        lambda *_: prepared,
    )
    build_projection = MagicMock(return_value=projection)
    publish = MagicMock()
    monkeypatch.setattr(
        "etl.xero.fresh_backfill.current_projection_from_records", build_projection,
    )
    monkeypatch.setattr(
        "etl.xero.fresh_backfill.publish_current_projections", publish,
    )

    result = replay_current_projections(
        client, object(), bucket="aquatiq-datavarehus-xero-raw",
        project="aquatiq-datavarehus", prefix=PREFIX, batch_size=2,
        run_id=requested_run_id,
    )

    assert result == {"objects": 1, "projection_batches": 1}
    build_projection.assert_called_once_with(
        "aquatiq-datavarehus-xero-raw", object_name,
        object_name.removeprefix(f"xero/{GENERATION}/"), prepared.records,
    )
    publish.assert_called_once_with((projection,))


def test_manifest_backfill_can_replay_one_exact_run(monkeypatch):
    requested_run_id = str(uuid4())
    other_run_id = str(uuid4())
    client = MagicMock()
    client.list_blobs.return_value = [
        SimpleNamespace(
            name=f"{MANIFEST_PREFIX}2026-10-16/{requested_run_id}.json.meta.json",
            generation=11,
        ),
        SimpleNamespace(
            name=f"{MANIFEST_PREFIX}2026-10-16/{other_run_id}.json.meta.json",
            generation=12,
        ),
    ]
    manifest = _prepared("manifest-one", is_manifest=True)
    manifest.batches["v2_ingest_receipts"][0]["run_id"] = requested_run_id
    prepare = MagicMock(return_value=manifest)
    monkeypatch.setattr("etl.xero.fresh_backfill.prepare_fresh_observation", prepare)
    writer = MagicMock()

    result = backfill_manifests(
        client, writer, object(), bucket="aquatiq-datavarehus-xero-raw",
        project="aquatiq-datavarehus", prefix=MANIFEST_PREFIX, run_id=requested_run_id,
    )

    assert result == {"objects": 1}
    assert prepare.call_count == 1
    writer.publish_observation.assert_called_once_with(manifest.batches)


@pytest.mark.parametrize("value", ["not-a-uuid", "{" + str(uuid4()) + "}", 1])
def test_canonical_run_id_rejects_noncanonical_values(value):
    with pytest.raises(ValueError, match="canonical run ID"):
        canonical_run_id(value)


def test_backfill_replays_each_verified_manifest_in_its_own_receipt_transaction(monkeypatch):
    client = MagicMock()
    client.list_blobs.return_value = [
        SimpleNamespace(
            name=f"{MANIFEST_PREFIX}2026-10-16/11111111-1111-4111-8111-111111111111.json.meta.json",
            generation=11,
        ),
        SimpleNamespace(
            name=f"{MANIFEST_PREFIX}2026-10-16/11111111-1111-4111-8111-111111111111.json",
            generation=12,
        ),
    ]
    manifest = _prepared("manifest-one", is_manifest=True)
    monkeypatch.setattr(
        "etl.xero.fresh_backfill.prepare_fresh_observation",
        lambda *_: manifest,
    )
    writer = MagicMock()

    result = backfill_manifests(
        client, writer, object(), bucket="aquatiq-datavarehus-xero-raw",
        project="aquatiq-datavarehus", prefix=MANIFEST_PREFIX,
    )

    assert result == {"objects": 1}
    writer.publish_observation.assert_called_once_with(manifest.batches)


def test_manifest_sidecars_streams_instead_of_materializing_the_generation():
    first = SimpleNamespace(
        name=f"{MANIFEST_PREFIX}2026-10-16/11111111-1111-4111-8111-111111111111.json.meta.json",
        generation=11,
    )

    def blobs():
        yield first
        raise AssertionError("manifest sidecars must not exhaust the bucket listing before yielding")

    client = MagicMock()
    client.list_blobs.return_value = blobs()

    assert next(manifest_sidecars(client, "aquatiq-datavarehus-xero-raw", MANIFEST_PREFIX)) == (
        first.name,
        "11",
    )


def test_raw_sidecars_streams_instead_of_materializing_the_generation():
    first = SimpleNamespace(name=f"{PREFIX}tenant/2.0/assets_asset_details/2026-10-16/a.json.meta.json", generation=11)

    def blobs():
        yield first
        raise AssertionError("raw sidecars must not exhaust the bucket listing before yielding")

    client = MagicMock()
    client.list_blobs.return_value = blobs()

    assert next(raw_sidecars(client, "aquatiq-datavarehus-xero-raw", PREFIX)) == (first.name, "11")


@pytest.mark.parametrize("batch_size", [0, -1, 251])
def test_backfill_rejects_unsafe_batch_sizes(batch_size):
    with pytest.raises(ValueError, match="batch size"):
        backfill_raw_observations(
            MagicMock(), MagicMock(), object(), bucket="aquatiq-datavarehus-xero-raw",
            project="aquatiq-datavarehus", prefix=PREFIX, batch_size=batch_size,
        )
