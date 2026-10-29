"""Contract tests for pure, append-only raw observation staging."""

from copy import deepcopy
from datetime import datetime, timezone
import hashlib
import json

import pytest

from etl.xero.fresh_staging import build_observation


GENERATION = "550e8400-e29b-41d4-a716-446655440000"
RUN_ID = "550e8400-e29b-41d4-a716-446655440001"
BUCKET = "raw-xero"
OBJECT = "raw/xero/tenant-1/2.0/invoices/page.json.meta.json"
OBSERVED_AT = datetime(2026, 10, 6, 10, tzinfo=timezone.utc)
DEFAULT_PAYLOAD = object()
TABLES = {
    "v2_record_versions",
    "v2_collection_versions",
    "v2_child_versions",
    "v2_ingest_receipts",
}
RECORD_FIELDS = set(
    (
        "generation tenant_id endpoint record_id version_id observed_at source_updated_at "
        "run_id receipt_id source_status source_deleted payload_hash payload_json"
    ).split()
)
COLLECTION_FIELDS = set(
    (
        "generation tenant_id endpoint record_id version_id collection_name observed_at "
        "source_updated_at child_count"
    ).split()
)
CHILD_FIELDS = set(
    (
        "generation tenant_id endpoint record_id version_id collection_name child_key "
        "line_position native_id payload_json payload_hash"
    ).split()
)
RECEIPT_FIELDS = set(
    (
        "receipt_id generation tenant_id endpoint run_id observed_at source_bucket source_object "
        "meta_generation data_generation http_status expected_records record_count child_count metadata_json"
    ).split()
)


def _meta(**overrides):
    return {
        "x-source-system": "xero_v2",
        "x-ingestion-generation": GENERATION,
        "x-tenant-id": "tenant-1",
        "x-endpoint": "invoices",
        "x-run-id": RUN_ID,
        "x-http-status": "200",
        "x-synced-at": "2026-10-06T12:00:00+02:00",
        "x-data-generation": "12345678901234567890",
        "x-record-count": "1",
    } | overrides


def _record(payload=DEFAULT_PAYLOAD, **overrides):
    return {
        "tenant_id": "tenant-1",
        "record_id": "invoice-1",
        "payload": {"InvoiceID": "invoice-1"} if payload is DEFAULT_PAYLOAD else payload,
    } | overrides


def _build(records=None, meta=None, **overrides):
    return build_observation(
        _meta() if meta is None else meta,
        [_record()] if records is None else records,
        **({"bucket": BUCKET, "object_name": OBJECT, "meta_generation": "99"} | overrides),
    )


def _json(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False)


def _hash(value):
    return hashlib.sha256(_json(value).encode("utf-8")).hexdigest()


def test_exact_schema_explicit_nulls_and_source_payload():
    payload = {"InvoiceID": "invoice-1", "LineItems": [{"Description": "raw"}], "Tax": None}
    result = _build([_record(payload)])
    assert set(result) == TABLES
    for table, fields in (
        ("v2_record_versions", RECORD_FIELDS),
        ("v2_collection_versions", COLLECTION_FIELDS),
        ("v2_child_versions", CHILD_FIELDS),
        ("v2_ingest_receipts", RECEIPT_FIELDS),
    ):
        assert len(result[table]) == 1
        assert set(result[table][0]) == fields
    row = result["v2_record_versions"][0]
    assert row["source_updated_at"] is None
    assert row["source_status"] is None
    assert row["source_deleted"] is False
    assert row["observed_at"] == OBSERVED_AT
    assert row["payload_json"] == _json(payload)
    assert row["payload_hash"] == _hash(payload)
    assert result["v2_collection_versions"][0]["source_updated_at"] is None
    assert result["v2_child_versions"][0]["native_id"] is None
    receipt = result["v2_ingest_receipts"][0]
    assert receipt["source_bucket"] == BUCKET
    assert receipt["source_object"] == OBJECT
    assert receipt["data_generation"] == _meta()["x-data-generation"]
    assert receipt["metadata_json"] == _json(_meta())
    assert (
        receipt["http_status"],
        receipt["expected_records"],
        receipt["record_count"],
        receipt["child_count"],
    ) == (200, 1, 1, 1)


def test_main_xero_source_system_is_accepted_for_a_fresh_warehouse_generation():
    result = _build(meta=_meta(**{"x-source-system": "xero"}))

    assert result["v2_ingest_receipts"][0]["tenant_id"] == "tenant-1"


def test_deterministic_replay_and_order_independent_json():
    payload = {"InvoiceID": "invoice-1", "Nested": {"z": 1, "a": "raw"}, "Lines": [2, 1]}
    first = _build([_record(payload)])
    reordered = {key: payload[key] for key in reversed(payload)}
    assert _build([_record(reordered)], dict(reversed(list(_meta().items())))) == first
    receipt_id = _hash([GENERATION, BUCKET, OBJECT, "99", _meta()["x-data-generation"]])
    assert first["v2_ingest_receipts"][0]["receipt_id"] == receipt_id
    assert first["v2_record_versions"][0]["version_id"] == _hash(
        [receipt_id, "invoices", "tenant-1", "invoice-1", 0]
    )


@pytest.mark.parametrize("change", ["generation", "bucket", "object", "meta", "data"])
def test_each_receipt_identity_component_creates_a_fresh_version(change):
    original = _build()
    meta = _meta(**({"x-ingestion-generation": RUN_ID} if change == "generation" else {}))
    if change == "data":
        meta = meta | {"x-data-generation": "12345678901234567891"}
    arguments = {
        "bucket": "another-bucket" if change == "bucket" else BUCKET,
        "object_name": OBJECT + ".copy" if change == "object" else OBJECT,
        "meta_generation": "100" if change == "meta" else "99",
    }
    changed = _build(meta=meta, **arguments)
    assert (
        changed["v2_ingest_receipts"][0]["receipt_id"]
        != original["v2_ingest_receipts"][0]["receipt_id"]
    )
    assert (
        changed["v2_record_versions"][0]["version_id"]
        != original["v2_record_versions"][0]["version_id"]
    )


def test_changed_payload_is_preserved_even_at_identical_effective_and_observed_time():
    first = _build([_record({"UpdatedDateUTC": "/Date(0)/", "Amount": 1})])
    second = _build([_record({"UpdatedDateUTC": "/Date(0)/", "Amount": 2})])
    a, b = first["v2_record_versions"][0], second["v2_record_versions"][0]
    assert a["version_id"] == b["version_id"]
    assert a["observed_at"] == b["observed_at"]
    assert a["source_updated_at"] == b["source_updated_at"]
    assert a["payload_hash"] != b["payload_hash"]
    assert json.loads(b["payload_json"])["Amount"] == 2


def test_same_record_id_in_multiple_tenants_has_distinct_versions():
    first = _build()
    second = _build([_record(tenant_id="tenant-2")], _meta(**{"x-tenant-id": "tenant-2"}))
    assert second["v2_record_versions"][0]["tenant_id"] == "tenant-2"
    assert (
        first["v2_record_versions"][0]["version_id"]
        != second["v2_record_versions"][0]["version_id"]
    )


@pytest.mark.parametrize("key", list(_meta()))
def test_all_required_metadata_is_required(key):
    with pytest.raises(ValueError, match=key):
        _build(meta={name: value for name, value in _meta().items() if name != key})


@pytest.mark.parametrize(
    "key,value",
    [
        (key, value)
        for key, values in {
            "x-source-system": ["other", None],
            "x-ingestion-generation": ["not-a-uuid", 1],
            "x-run-id": ["run-1", None],
            "x-tenant-id": ["", " ", None],
            "x-endpoint": ["", None],
            "x-http-status": ["201", "206", True, 200.0],
            "x-data-generation": ["0", "-1", "1.0", 123, " 1", "+1", "\u0661"],
            "x-record-count": ["-1", "1.5", "1e0", "NaN", " 1", True, None, "\u0661"],
        }.items()
        for value in values
    ],
)
def test_invalid_metadata_fails_without_silent_coercion(key, value):
    with pytest.raises(ValueError, match=key):
        _build(meta=_meta(**{key: value}))


@pytest.mark.parametrize(
    "timestamp",
    [
        None,
        "",
        "bad",
        "2026-10-06",
        "2026-10-06T10:00:00",
        123,
        "2026-10-06T25:00:00Z",
        "2026-10-06T10:00:00+99:00",
    ],
)
def test_observation_timestamp_is_strict_and_never_substituted(timestamp):
    with pytest.raises(ValueError, match="x-synced-at"):
        _build(meta=_meta(**{"x-synced-at": timestamp}))


def test_generation_and_run_ids_are_canonical_uuids_and_observation_is_utc():
    result = _build(
        meta=_meta(
            **{
                "x-ingestion-generation": "{" + GENERATION.upper() + "}",
                "x-run-id": RUN_ID.replace("-", ""),
                "x-synced-at": "2026-10-06T10:00:00.123456789Z",
            }
        )
    )
    row = result["v2_record_versions"][0]
    assert (row["generation"], row["run_id"]) == (GENERATION, RUN_ID)
    assert row["observed_at"] == OBSERVED_AT.replace(microsecond=123456)


@pytest.mark.parametrize("count", ["0", "2"])
def test_ordinary_record_count_mismatch_fails(count):
    with pytest.raises(ValueError, match="x-record-count"):
        _build(meta=_meta(**{"x-record-count": count}))


def test_empty_page_still_has_receipt_and_no_business_rows():
    result = _build([], _meta(**{"x-record-count": "0"}))
    assert all(result[table] == [] for table in TABLES - {"v2_ingest_receipts"})
    assert result["v2_ingest_receipts"][0]["record_count"] == 0


@pytest.mark.parametrize(
    "record",
    [
        {"tenant_id": "tenant-1", "payload": {}},
        _record(record_id=None),
        _record(record_id=""),
        _record(record_id=" "),
        _record(record_id=123),
        _record(tenant_id="tenant-2"),
        _record(tenant_id=None),
        _record({"TenantID": "tenant-2"}),
        _record(meta={"x-tenant-id": "tenant-2"}),
        {"tenant_id": "tenant-1", "record_id": "invoice-1"},
        _record(payload=None),
        _record(payload="not-an-object"),
        "not-a-record",
    ],
)
def test_invalid_record_or_tenant_fails_instead_of_dropping(record):
    with pytest.raises(ValueError):
        _build([record])


def test_later_invalid_record_rejects_whole_observation():
    with pytest.raises(ValueError):
        _build([_record(), _record(record_id=None)], _meta(**{"x-record-count": "2"}))


@pytest.mark.parametrize(
    "value,expected",
    [
        (None, None),
        ("/Date(0)/", datetime(1970, 1, 1, tzinfo=timezone.utc)),
        ("/Date(-1+0200)/", datetime(1969, 12, 31, 23, 59, 59, 999000, tzinfo=timezone.utc)),
        ("2026-10-06T12:00:00+02:00", OBSERVED_AT),
        ("2026-10-06T10:00:00Z", OBSERVED_AT),
        ("2026-10-06T10:00:00", OBSERVED_AT),
    ],
)
def test_source_updated_at_is_optional_and_parsed_on_records_and_collections(value, expected):
    result = _build([_record({"UpdatedDateUTC": value, "Lines": []})])
    assert result["v2_record_versions"][0]["source_updated_at"] == expected
    assert result["v2_collection_versions"][0]["source_updated_at"] == expected
    assert json.loads(result["v2_record_versions"][0]["payload_json"])["UpdatedDateUTC"] == value


@pytest.mark.parametrize("value", ["bad", "", 0, "prefix /Date(0)/ suffix", "/Date(1e500)/"])
def test_malformed_present_source_timestamp_is_not_silently_discarded(value):
    with pytest.raises(ValueError, match="UpdatedDateUTC"):
        _build([_record({"UpdatedDateUTC": value})])


def test_every_explicit_list_creates_collection_and_every_position_creates_child():
    payload = {
        "LineItems": [{"IsBlank": True}, None, {}, ["nested"], "raw", False, 0],
        "Other": [],
        "Absent": None,
        "Nested": {"NotTopLevel": []},
    }
    result = _build([_record(payload)])
    collections = {row["collection_name"]: row for row in result["v2_collection_versions"]}
    assert set(collections) == {"LineItems", "Other"}
    assert collections["LineItems"]["child_count"] == 7
    assert collections["Other"]["child_count"] == 0
    children = result["v2_child_versions"]
    assert [row["line_position"] for row in children] == list(range(7))
    assert [row["child_key"] for row in children] == [f"position:{i}" for i in range(7)]
    assert [json.loads(row["payload_json"]) for row in children] == payload["LineItems"]
    assert all(
        row["payload_hash"] == _hash(value) for row, value in zip(children, payload["LineItems"])
    )
    assert result["v2_ingest_receipts"][0]["child_count"] == 7


def test_out_of_scope_branding_theme_payment_services_are_removed_from_staged_parent():
    payload = {
        "BrandingThemeID": "theme-1",
        "PaymentServices": [{"PaymentServiceID": "service-1"}],
    }
    result = _build(
        [_record(payload, record_id="theme-1")],
        _meta(**{"x-endpoint": "branding_themes"}),
    )
    assert payload["PaymentServices"] == [{"PaymentServiceID": "service-1"}]
    assert json.loads(result["v2_record_versions"][0]["payload_json"]) == {
        "BrandingThemeID": "theme-1",
    }
    assert result["v2_collection_versions"] == []
    assert result["v2_child_versions"] == []
    assert result["v2_ingest_receipts"][0]["child_count"] == 0


def test_shrink_empty_omitted_and_null_collections_are_distinct_immutable_observations():
    full = _build([_record({"Lines": [{"LineItemID": "a"}, {"LineItemID": "b"}]})])
    shrunk = _build([_record({"Lines": [{"LineItemID": "a"}]})], meta_generation="100")
    empty = _build([_record({"Lines": []})], meta_generation="101")
    omitted = _build([_record({})], meta_generation="102")
    null = _build([_record({"Lines": None})], meta_generation="103")
    assert [
        row["child_count"]
        for result in (full, shrunk, empty)
        for row in result["v2_collection_versions"]
    ] == [2, 1, 0]
    assert len(full["v2_child_versions"]) == 2
    assert empty["v2_child_versions"] == []
    assert omitted["v2_collection_versions"] == null["v2_collection_versions"] == []
    assert all(
        not result["v2_record_versions"][0]["source_deleted"]
        for result in (full, shrunk, empty, omitted, null)
    )


@pytest.mark.parametrize(
    "field",
    "LineItemID PaymentID JournalLineID ContactID AttachmentID TrackingOptionID AccountID taskId id".split(),
)
def test_unique_native_ids_are_used_for_child_keys(field):
    children = _build([_record({"Children": [{field: "native-1"}]})])["v2_child_versions"]
    assert children[0]["native_id"] == "native-1"
    assert children[0]["child_key"] == "native-1"


def test_duplicate_null_and_missing_ids_use_positions_without_losing_native_id():
    items = [
        {"LineItemID": "duplicate"},
        {"LineItemID": "duplicate"},
        {"LineItemID": None},
        {},
        {"LineItemID": ""},
        {"LineItemID": "unique"},
    ]
    children = _build([_record({"LineItems": items})])["v2_child_versions"]
    assert [row["child_key"] for row in children] == [
        "position:0",
        "position:1",
        "position:2",
        "position:3",
        "position:4",
        "unique",
    ]
    assert [row["native_id"] for row in children] == [
        "duplicate",
        "duplicate",
        None,
        None,
        "",
        "unique",
    ]


def test_manual_journal_blank_lines_retain_original_indexes_and_count():
    lines = [
        {"IsBlank": True},
        {"Description": "debit", "LineAmount": 10},
        {"IsBlank": True},
        {"Description": "credit", "LineAmount": -10},
    ]
    result = _build(
        [_record({"JournalLines": lines}, record_id="manual-1")],
        _meta(**{"x-endpoint": "manual_journals"}),
    )
    assert [row["line_position"] for row in result["v2_child_versions"]] == [0, 1, 2, 3]
    assert result["v2_collection_versions"][0]["child_count"] == 4
    assert result["v2_ingest_receipts"][0]["expected_records"] == 1


def test_http_304_produces_only_receipt_regardless_of_cached_records():
    result = _build([_record()], _meta(**{"x-http-status": "304", "x-record-count": "0"}))
    assert all(result[table] == [] for table in TABLES - {"v2_ingest_receipts"})
    assert result["v2_ingest_receipts"][0]["http_status"] == 304
    assert result["v2_ingest_receipts"][0]["record_count"] == 0
    assert result["v2_ingest_receipts"][0]["child_count"] == 0


def test_direct_binary_none_is_receipt_only_with_raw_pointer():
    meta = _meta(**{"x-endpoint": "accounting_get_getInvoiceAsPdf"})
    records = [_record(payload=None, record_id=RUN_ID)]
    result = _build(records, meta, object_name="raw/xero/tenant-1/invoice.pdf.meta.json")
    assert all(result[table] == [] for table in TABLES - {"v2_ingest_receipts"})
    receipt = result["v2_ingest_receipts"][0]
    assert (receipt["expected_records"], receipt["record_count"]) == (1, 0)
    assert receipt["source_object"] == "raw/xero/tenant-1/invoice.pdf.meta.json"
    assert json.loads(receipt["metadata_json"]) == meta


def test_direct_json_response_is_a_request_snapshot_not_normal_entity_rows():
    payload = {"Invoices": [{"InvoiceID": "invoice-1"}, {"InvoiceID": "invoice-2"}]}
    meta = _meta(**{"x-endpoint": "accounting_get_getInvoice", "x-record-count": "2"})
    result = _build([_record(payload, record_id=RUN_ID)], meta)
    assert len(result["v2_record_versions"]) == 1
    row = result["v2_record_versions"][0]
    assert row["record_id"] == RUN_ID
    assert row["endpoint"] == "accounting_get_getInvoice"
    assert json.loads(row["payload_json"]) == payload
    assert len(result["v2_child_versions"]) == 2
    assert (
        result["v2_ingest_receipts"][0]["expected_records"],
        result["v2_ingest_receipts"][0]["record_count"],
    ) == (2, 1)


def test_direct_detail_response_uses_the_parent_identity_not_the_run_identity():
    parent_id = "5f3c9a2e-0000-0000-0000-000000000000"
    meta = _meta(**{
        "x-endpoint": "accounting_get_getAccount",
        "x-operation-id": "getAccount",
        "x-parent-record-id": parent_id,
    })
    result = _build([_record({"Accounts": {"AccountID": parent_id}}, record_id=RUN_ID)], meta)

    row = result["v2_record_versions"][0]
    assert row["record_id"] == f"getAccount:{parent_id}"
    assert row["endpoint"] == "accounting_get_getAccount"


def test_attachment_wrapper_counts_are_explicit_without_false_equality():
    payload = {"Attachments": [{"AttachmentID": "a"}, {"AttachmentID": "b"}]}
    result = _build(
        [_record(payload)],
        _meta(
            **{
                "x-endpoint": "attachment_metadata",
                "x-record-count": "2",
            }
        ),
    )
    receipt = result["v2_ingest_receipts"][0]
    assert (receipt["expected_records"], receipt["record_count"], receipt["child_count"]) == (
        2,
        1,
        2,
    )


def test_extended_snapshot_wrapper_without_reader_record_id_is_preserved():
    payload = {
        "source_endpoint": "projects_projects",
        "request_key": "",
        "record_ids": ["project-1", "project-2"],
        "record_count": 2,
    }
    meta = _meta(
        **{
            "x-endpoint": "projects_list_snapshots",
            "x-record-count": "2",
            "x-source-endpoint": "projects_projects",
            "x-request-key": "",
        }
    )
    record = {"tenant_id": "tenant-1", "payload": payload}
    result = _build([record], meta)
    assert len(result["v2_record_versions"]) == 1
    assert result["v2_record_versions"][0]["record_id"]
    assert json.loads(result["v2_record_versions"][0]["payload_json"]) == payload
    assert result["v2_ingest_receipts"][0]["record_count"] == 1
    assert result["v2_ingest_receipts"][0]["child_count"] == 2


def test_inputs_are_never_mutated_and_replays_do_not_share_rows():
    meta = _meta()
    records = [_record({"Lines": [{"IsBlank": True}, None], "Nested": {"a": 1}})]
    before = deepcopy((meta, records))
    first = _build(records, meta)
    second = _build(records, meta)
    assert (meta, records) == before
    for table in TABLES:
        assert first[table] is not second[table]
        assert all(a is not b for a, b in zip(first[table], second[table]))


@pytest.mark.parametrize(
    "payload",
    [{"bad": float("nan")}, {"bad": float("inf")}, {"bad": object()}, {1: "not-a-json-key"}],
)
def test_non_json_values_fail_instead_of_lossy_stringification(payload):
    with pytest.raises(ValueError, match="JSON"):
        _build([_record(payload)])


def test_reserved_position_keys_cannot_collide_with_fallbacks_or_each_other():
    items = [None, {"LineItemID": "position:0"}, {"LineItemID": "position:1"}]
    children = _build([_record({"Lines": items})])["v2_child_versions"]
    assert len({row["child_key"] for row in children}) == 3
    assert [row["native_id"] for row in children] == [None, "position:0", "position:1"]


def test_blank_primary_native_id_does_not_hide_an_available_common_id():
    child = {"LineItemID": "", "PaymentID": "payment-1"}
    children = _build([_record({"Lines": [child]})])["v2_child_versions"]
    assert children[0]["native_id"] == children[0]["child_key"] == "payment-1"


def test_direct_response_id_must_be_its_request_run_not_an_entity_id():
    with pytest.raises(ValueError, match="record_id"):
        _build(
            [_record(record_id="invoice-1")], _meta(**{"x-endpoint": "accounting_get_getInvoice"})
        )


@pytest.mark.parametrize("field", ["x-source-endpoint", "x-request-key"])
def test_snapshot_identity_requires_source_and_request_scope(field):
    meta = _meta(
        **{
            "x-endpoint": "projects_list_snapshots",
            "x-source-endpoint": "projects_projects",
            "x-request-key": "",
        }
    )
    with pytest.raises(ValueError, match=field):
        _build(
            [{"tenant_id": "tenant-1", "payload": {}}],
            {key: value for key, value in meta.items() if key != field},
        )


@pytest.mark.parametrize("field", ["bucket", "object_name", "meta_generation"])
@pytest.mark.parametrize("value", [None, "", " ", 123])
def test_receipt_source_identity_fields_are_required_strings(field, value):
    with pytest.raises(ValueError, match=field):
        _build(**{field: value})


@pytest.mark.parametrize("value", [None, {}, (), "records"])
def test_record_container_must_be_a_list(value):
    with pytest.raises(ValueError, match="records"):
        build_observation(_meta(), value, BUCKET, OBJECT, "99")


@pytest.mark.parametrize("value", [None, [], "metadata"])
def test_meta_container_must_be_a_dictionary(value):
    with pytest.raises(ValueError, match="meta"):
        build_observation(value, [], BUCKET, OBJECT, "99")


@pytest.mark.parametrize(
    "field,value",
    [
        ("x-run-id", GENERATION),
        ("x-ingestion-generation", RUN_ID),
        ("x-endpoint", "contacts"),
        ("x-source-system", "xero"),
    ],
)
def test_embedded_reader_metadata_cannot_override_observation_identity(field, value):
    with pytest.raises(ValueError, match=field):
        _build([_record(meta={field: value})])


def test_matching_reader_metadata_accepts_canonical_uuid_variants():
    meta = _meta(**{"x-run-id": "{" + RUN_ID + "}"})
    assert _build([_record(meta=meta)])["v2_record_versions"][0]["run_id"] == RUN_ID


def test_matching_main_xero_reader_metadata_is_accepted_without_legacy_source_alias():
    meta = _meta(**{"x-source-system": "xero"})

    result = _build([_record(meta={"x-source-system": "xero"})], meta)

    assert result["v2_record_versions"][0]["tenant_id"] == "tenant-1"


def test_invalid_reader_metadata_is_rejected():
    with pytest.raises(ValueError, match="meta"):
        _build([_record(meta="invalid")])


def test_direct_binary_cannot_bypass_tenant_validation():
    with pytest.raises(ValueError, match="tenant"):
        _build(
            [_record(None, record_id=RUN_ID, tenant_id="tenant-2")],
            _meta(**{"x-endpoint": "accounting_get_getInvoiceAsPdf"}),
        )


def test_direct_json_list_and_missing_request_id_preserve_complete_response():
    payload = [{"id": "first"}, None]
    result = _build(
        [{"tenant_id": "tenant-1", "payload": payload}],
        _meta(**{"x-endpoint": "accounting_get_getSomething"}),
    )
    row = result["v2_record_versions"][0]
    assert row["record_id"] == RUN_ID
    assert json.loads(row["payload_json"]) == payload


def test_unknown_endpoint_and_duplicate_record_ids_are_retained_without_legacy_dispatch():
    result = _build(
        [_record({"IsBlank": True}), _record({"Other": 1})],
        _meta(**{"x-endpoint": "new_endpoint", "x-record-count": "2"}),
    )
    assert len(result["v2_record_versions"]) == 2
    assert json.loads(result["v2_record_versions"][0]["payload_json"]) == {"IsBlank": True}
    assert result["v2_ingest_receipts"][0]["record_count"] == 2


@pytest.mark.parametrize("value", ["/Date(999999999999999999999999)/", "9999-12-31T23:59:59-01:00"])
def test_out_of_range_source_timestamp_raises_clear_error(value):
    with pytest.raises(ValueError, match="UpdatedDateUTC"):
        _build([_record({"UpdatedDateUTC": value})])


def test_integer_and_generic_native_ids_are_retained_without_generic_status_inference():
    payload = {"ProjectStatus": "ARCHIVED", "Lines": [{"JournalLineID": 0}, {"UnusualID": "other"}]}
    result = _build([_record(payload)])
    assert [row["native_id"] for row in result["v2_child_versions"]] == ["0", "other"]
    assert result["v2_record_versions"][0]["source_status"] is None
    assert result["v2_record_versions"][0]["source_deleted"] is False


def test_json_unicode_is_semantically_unchanged_and_unencodable_unicode_is_rejected():
    payload = {"Text": "\u00e6\u00f8\u00e5", "Lines": ["\u2603"]}
    assert (
        json.loads(_build([_record(payload)])["v2_record_versions"][0]["payload_json"]) == payload
    )
    with pytest.raises(ValueError, match="JSON"):
        _build([_record({"Text": "\ud800"})])


def _manifest_meta(**overrides):
    return _meta(
        **{
            "x-tenant-id": "run",
            "x-endpoint": "sync_manifest",
            "x-record-count": "0",
            "x-run-started-at": "2026-10-06T09:00:00Z",
            "x-sync-type": "manifest",
        }
        | overrides
    )


def test_manifest_sidecar_emits_only_receipt_with_finished_time_and_original_tags():
    meta = _manifest_meta()
    before = deepcopy(meta)
    object_name = f"manifests/xero/2026-10-06/{RUN_ID}.json.meta.json"
    result = _build([], meta, object_name=object_name)
    assert result == _build([], meta, object_name=object_name)
    assert all(result[table] == [] for table in TABLES - {"v2_ingest_receipts"})
    receipt = result["v2_ingest_receipts"][0]
    assert set(receipt) == RECEIPT_FIELDS
    assert (receipt["tenant_id"], receipt["endpoint"], receipt["run_id"]) == (
        "run",
        "sync_manifest",
        RUN_ID,
    )
    assert receipt["observed_at"] == OBSERVED_AT
    assert (receipt["expected_records"], receipt["record_count"], receipt["child_count"]) == (
        0,
        0,
        0,
    )
    assert receipt["source_object"] == object_name
    assert receipt["data_generation"] == meta["x-data-generation"]
    assert json.loads(receipt["metadata_json"]) == before == meta


@pytest.mark.parametrize(
    "endpoint", ["invoices", "accounting_get_getInvoice", "projects_list_snapshots"]
)
@pytest.mark.parametrize("status", ["200", "304"])
def test_reserved_run_tenant_cannot_be_used_for_non_manifest_receipts(endpoint, status):
    meta = _manifest_meta(**{"x-endpoint": endpoint, "x-http-status": status})
    with pytest.raises(ValueError, match="x-tenant-id"):
        _build([], meta)


@pytest.mark.parametrize("tenant", ["tenant-1", RUN_ID])
def test_manifest_requires_reserved_run_tenant(tenant):
    with pytest.raises(ValueError, match="x-tenant-id"):
        _build([], _manifest_meta(**{"x-tenant-id": tenant}))


@pytest.mark.parametrize("status", ["200", "304"])
@pytest.mark.parametrize(
    "count,records",
    [
        ("0", [_record(tenant_id="run")]),
        ("1", [_record(tenant_id="run")]),
        ("1", []),
    ],
)
def test_manifest_rejects_business_records_or_nonzero_header_count(status, count, records):
    meta = _manifest_meta(**{"x-record-count": count, "x-http-status": status})
    with pytest.raises(ValueError, match="sync_manifest"):
        _build(records, meta)


@pytest.mark.parametrize(
    "field", ["x-synced-at", "x-data-generation", "x-ingestion-generation", "x-run-id"]
)
def test_manifest_receipt_cannot_bypass_required_metadata_validation(field):
    meta = _manifest_meta()
    with pytest.raises(ValueError, match=field):
        _build([], {key: value for key, value in meta.items() if key != field})


@pytest.mark.parametrize("divergent", [False, True])
def test_duplicate_headers_have_unique_occurrence_versions_and_nonmultiplying_child_joins(
    divergent,
):
    payload = {"UpdatedDateUTC": "/Date(0)/", "Lines": [{"LineItemID": "a"}, {"IsBlank": True}]}
    records = [
        _record(payload),
        _record(payload | {"Lines": payload["Lines"][:1]} if divergent else payload),
    ]
    meta = _meta(**{"x-record-count": "2"})
    before = deepcopy((records, meta))
    result = _build(records, meta)
    assert result == _build(records, meta)
    assert (records, meta) == before
    parents = result["v2_record_versions"]
    collections = result["v2_collection_versions"]
    children = result["v2_child_versions"]
    assert len(parents) == len(collections) == 2
    assert len({parent["version_id"] for parent in parents}) == 2
    assert len({parent["payload_hash"] for parent in parents}) == (2 if divergent else 1)
    assert len({parent["source_updated_at"] for parent in parents}) == 1
    assert len({parent["observed_at"] for parent in parents}) == 1
    for index, parent in enumerate(parents):
        assert parent["version_id"] == _hash(
            [parent["receipt_id"], "invoices", "tenant-1", "invoice-1", index]
        )
        assert json.loads(parent["payload_json"]) == records[index]["payload"]
        matching = [row for row in collections if row["version_id"] == parent["version_id"]]
        assert len(matching) == 1
        assert matching[0]["child_count"] == len(records[index]["payload"]["Lines"])
    joined = [
        child
        for parent in parents
        for collection in collections
        for child in children
        if parent["version_id"] == collection["version_id"] == child["version_id"]
        and collection["collection_name"] == child["collection_name"]
    ]
    assert len(joined) == len(children) == (3 if divergent else 4)
    assert result["v2_ingest_receipts"][0]["child_count"] == len(children)


@pytest.mark.parametrize(
    "endpoint",
    (
        "report_balance_sheet report_aged_payables_by_contact report_aged_receivables_by_contact "
        "report_bank_summary report_budget_summary report_executive_summary "
        "report_profit_and_loss report_trial_balance"
    ).split(),
)
def test_report_identity_is_query_scoped_with_unchanged_report_payload(endpoint):
    payload = {"ReportID": "constant-kind", "Rows": []}
    records = [_record(payload, record_id=payload["ReportID"])]
    before = deepcopy(records)
    signatures = [
        "contactID=a&date=2026-10-05",
        "contactID=b&date=2026-10-05",
        "contactID=a&date=2026-10-06",
        "fromDate=2026-09-01&toDate=2026-09-30",
    ]
    results = [
        _build(records, _meta(**{"x-endpoint": endpoint, "x-report-params": signature}))
        for signature in signatures
    ]
    parents = [result["v2_record_versions"][0] for result in results]
    assert len({row["record_id"] for row in parents}) == len(signatures)
    assert len({row["payload_hash"] for row in parents}) == 1
    for row, signature, result in zip(parents, signatures, results):
        assert row["record_id"] == _hash([payload["ReportID"], signature])
        assert json.loads(row["payload_json"]) == payload
        assert result["v2_collection_versions"][0]["record_id"] == row["record_id"]
        assert (
            json.loads(result["v2_ingest_receipts"][0]["metadata_json"])["x-report-params"]
            == signature
        )
        meta = _meta(**{"x-endpoint": endpoint, "x-report-params": signature})
        assert result == _build(records, meta)
    assert records == before


@pytest.mark.parametrize("params", [DEFAULT_PAYLOAD, None, "", " ", 1, {}, []])
@pytest.mark.parametrize("endpoint", ["report_balance_sheet", "invoices"])
@pytest.mark.parametrize("status", ["200", "304"])
def test_only_report_receipts_require_a_nonempty_query_signature(endpoint, params, status):
    meta = _meta(**{"x-endpoint": endpoint, "x-record-count": "0", "x-http-status": status})
    meta = meta if params is DEFAULT_PAYLOAD else meta | {"x-report-params": params}
    if endpoint.startswith("report_"):
        with pytest.raises(ValueError, match="x-report-params"):
            _build([], meta)
    else:
        assert _build([], meta)["v2_record_versions"] == []
