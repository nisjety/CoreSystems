"""Rules enforced by ``vsv2-validate-config`` on config/endpoints/*.json."""

from __future__ import annotations

import json
import shutil
from pathlib import Path

import pytest

from vsv2_tooling import config_validate
from vsv2_tooling.config_validate import validate_configs

REPO = Path(config_validate.__file__).resolve().parents[3]


def erp(name: str = "accounts", **overrides) -> dict:
    cfg = {
        "endpoint_name": name,
        "bigquery_table": f"visma_{name}",
        "primary_key": "accountCD",
        "source": "erp_rest",
        "api_method": "get_accounts",
        "api_endpoint": "/v1/account",
        "pagination_profile": "finance_page_number",
        "modified_field": None,
        "merge_type": "upsert",
        "sync_pattern": "full_refresh_upsert",
        "validation_fields": ["accountCD"],
    }
    cfg.update(overrides)
    return cfg


@pytest.fixture
def endpoints(tmp_path: Path):
    """A throwaway config/endpoints dir with the real schema; returns (dir, repo_root, write)."""
    root = tmp_path / "repo"
    directory = root / "config" / "endpoints"
    directory.mkdir(parents=True)
    shutil.copy(REPO / "config/endpoints/_schema.json", directory / "_schema.json")

    def write(cfg: dict, filename: str | None = None) -> None:
        (directory / f"{filename or cfg['endpoint_name']}.json").write_text(json.dumps(cfg))

    return directory, root, write


def messages(endpoints_fixture) -> list[str]:
    directory, root, _ = endpoints_fixture
    failures, _ = validate_configs(directory, root)
    return [f"{name}: {msg}" for name, msg in failures]


@pytest.mark.unit
def test_a_valid_config_passes(endpoints) -> None:
    endpoints[2](erp())

    failures, checked = validate_configs(endpoints[0], endpoints[1])

    assert failures == []
    assert checked == 1


@pytest.mark.unit
def test_files_starting_with_underscore_are_not_configs(endpoints) -> None:
    (endpoints[0] / "_notes.json").write_text("not json at all")
    endpoints[2](erp())

    assert validate_configs(endpoints[0], endpoints[1]) == ([], 1)


@pytest.mark.unit
def test_invalid_json_is_reported(endpoints) -> None:
    (endpoints[0] / "broken.json").write_text("{nope")

    assert any("invalid JSON" in m for m in messages(endpoints))


@pytest.mark.unit
def test_endpoint_name_must_match_the_filename(endpoints) -> None:
    endpoints[2](erp("accounts"), filename="other_name")

    assert any("must match filename stem 'other_name'" in m for m in messages(endpoints))


@pytest.mark.unit
def test_erp_rest_requires_a_real_http_path(endpoints) -> None:
    endpoints[2](erp("a", api_endpoint=""))
    endpoints[2](erp("b", api_endpoint="get_accounts"))
    endpoints[2](erp("c", api_endpoint="account"))

    found = messages(endpoints)
    assert any(m.startswith("a.json") and "requires api_endpoint" in m for m in found)
    assert any(m.startswith("b.json") and "not an executable HTTP path" in m for m in found)
    assert any(m.startswith("c.json") and "not an executable HTTP path" in m for m in found)


@pytest.mark.unit
def test_erp_rest_requires_an_explicit_pagination_profile(endpoints) -> None:
    cfg = erp()
    del cfg["pagination_profile"]
    endpoints[2](cfg)

    assert any("requires explicit pagination_profile" in m for m in messages(endpoints))


@pytest.mark.unit
def test_salesorder_v3_must_use_the_page_index_profile(endpoints) -> None:
    endpoints[2](
        erp("sales_orders_v3", api_endpoint="https://salesorder.visma.net/api/v3/SalesOrders")
    )

    assert any("salesorder_page_index" in m for m in messages(endpoints))


@pytest.mark.unit
def test_a_salesorder_child_list_may_be_unpaged_but_a_listing_may_not(endpoints) -> None:
    endpoints[2](path_erp("child_ok"))
    endpoints[2](
        erp(
            "listing_unpaged",
            api_endpoint="https://salesorder.visma.net/api/v3/Customers",
            pagination_profile="none",
        )
    )

    found = messages(endpoints)
    assert not any(m.startswith("child_ok.json") for m in found), found
    assert any(
        m.startswith("listing_unpaged.json") and "salesorder_page_index" in m for m in found
    ), found


@pytest.mark.unit
def test_deprecated_salesorder_v2_is_rejected(endpoints) -> None:
    endpoints[2](erp("salesorder", api_endpoint="/v2/salesorder"))

    assert any("deprecated/EOL" in m for m in messages(endpoints))


@pytest.mark.unit
def test_journal_transactions_must_use_finance_page_number(endpoints) -> None:
    endpoints[2](
        erp(
            "journal_transactions",
            api_endpoint="/v2/journaltransaction",
            pagination_profile="journal_tx_page_index",
        )
    )

    assert any("finance_page_number" in m for m in messages(endpoints))


@pytest.mark.unit
def test_background_mode_needs_a_relative_finance_path(endpoints) -> None:
    endpoints[2](erp("ok", api_endpoint="/v1/account", background_mode="subscription"))
    endpoints[2](
        erp("absolute", api_endpoint="https://example.test/v1/x", background_mode="subscription")
    )

    found = messages(endpoints)
    assert not any(m.startswith("ok.json") for m in found)
    assert any(m.startswith("absolute.json") and "background_mode" in m for m in found)


@pytest.mark.unit
def test_required_query_must_come_from_somewhere(endpoints) -> None:
    endpoints[2](erp("missing", required_query=["ledger"]))
    endpoints[2](erp("from_default", required_query=["ledger"], default_query={"ledger": "1"}))
    endpoints[2](
        erp(
            "from_partition",
            required_query=["ledger"],
            partitioning={
                "mode": "cartesian",
                "dimensions": [
                    {"query_param": "ledger", "source_endpoint": "/v1/ledger", "source_path": "n"}
                ],
            },
        )
    )

    found = messages(endpoints)
    assert any(m.startswith("missing.json") and "required_query 'ledger'" in m for m in found)
    assert not any(m.startswith(("from_default", "from_partition")) for m in found)


@pytest.mark.unit
def test_partition_dimensions_must_be_unique_and_executable(endpoints) -> None:
    dim = {"query_param": "ledger", "source_endpoint": "/v1/ledger", "source_path": "n"}
    endpoints[2](erp("dup", partitioning={"mode": "cartesian", "dimensions": [dim, dim]}))
    endpoints[2](
        erp(
            "bad",
            partitioning={
                "mode": "cartesian",
                "dimensions": [{**dim, "source_endpoint": "ledgers"}],
            },
        )
    )

    found = messages(endpoints)
    assert any(m.startswith("dup.json") and "duplicated" in m for m in found)
    assert any(m.startswith("bad.json") and "not an executable HTTP path" in m for m in found)


PATH_URL = "https://salesorder.visma.net/api/v3/Customers/{customerId}/locations"
PATH_DIM = {
    "query_param": "customerId",
    "in_path": True,
    "source_endpoint": "https://salesorder.visma.net/api/v3/Customers",
    "source_pagination_profile": "salesorder_page_index",
    "source_path": "id",
    "carry": {"defaultLocationId": "defaultLocation.id"},
}


def path_erp(name: str = "customer_locations", **overrides) -> dict:
    cfg = erp(
        name,
        api_endpoint=PATH_URL,
        pagination_profile="none",
        primary_key=["customerId", "locationId"],
        validation_fields=["customerId", "locationId"],
        partitioning={
            "mode": "cartesian",
            "dimensions": [PATH_DIM],
            "row_copies": {"locationId": "id"},
            "max_partitions": 20000,
        },
    )
    cfg.update(overrides)
    return cfg


@pytest.mark.unit
def test_a_url_template_filled_by_a_path_dimension_is_valid(endpoints) -> None:
    endpoints[2](path_erp())

    assert messages(endpoints) == []


@pytest.mark.unit
def test_a_url_placeholder_needs_a_path_dimension_that_fills_it(endpoints) -> None:
    endpoints[2](path_erp("no_dim", partitioning=None))
    endpoints[2](
        path_erp(
            "as_query",
            partitioning={
                "mode": "cartesian",
                "dimensions": [{**PATH_DIM, "in_path": False}],
            },
        )
    )
    endpoints[2](
        path_erp(
            "wrong_name",
            partitioning={
                "mode": "cartesian",
                "dimensions": [{**PATH_DIM, "query_param": "supplierId"}],
            },
        )
    )

    found = messages(endpoints)
    for name in ("no_dim", "as_query", "wrong_name"):
        assert any(
            m.startswith(f"{name}.json") and "{customerId}" in m and "in_path" in m for m in found
        ), (name, found)


@pytest.mark.unit
def test_a_path_dimension_needs_its_placeholder_in_the_url(endpoints) -> None:
    endpoints[2](path_erp("orphan", api_endpoint="/api/v3/Customers"))

    found = messages(endpoints)

    assert any(
        m.startswith("orphan.json") and "in_path" in m and "{customerId}" in m for m in found
    ), found


@pytest.mark.unit
def test_an_unknown_source_pagination_profile_is_a_schema_error(endpoints) -> None:
    dim = {**PATH_DIM, "source_pagination_profile": "cursor"}
    endpoints[2](path_erp("bad_profile", partitioning={"mode": "cartesian", "dimensions": [dim]}))

    assert any(m.startswith("bad_profile.json") for m in messages(endpoints))


@pytest.mark.unit
def test_tier_one_endpoints_can_never_be_ready_on_datamart(endpoints) -> None:
    endpoints[2](erp("t1", tier=1, datamart={"ready": True}))

    assert any("Invariant #4" in m for m in messages(endpoints))


@pytest.mark.unit
def test_enabled_datamart_needs_its_query_file_and_a_registered_normalizer(endpoints) -> None:
    endpoints[2](
        erp(
            "dm",
            datamart={
                "ready": True,
                "query_file": "config/queries/nope.graphql",
                "join_normalizer": "normalize_nothing",
            },
        )
    )

    found = messages(endpoints)
    assert any("query_file does not exist" in m for m in found)
    assert any("join_normalizer is not registered: normalize_nothing" in m for m in found)


@pytest.mark.unit
def test_a_mapped_datamart_root_needs_its_query_file_even_when_not_ready(endpoints) -> None:
    # Three placeholders once pointed at query files that did not exist while ready=false, so
    # nothing complained until someone tried to shadow-compare them.
    endpoints[2](
        erp(
            "shadow",
            datamart={
                "ready": False,
                "root_field": "accounts",
                "query_file": "config/queries/missing.graphql",
            },
        )
    )

    assert any("root_field is set but query_file does not exist" in m for m in messages(endpoints))


@pytest.mark.unit
def test_a_mapped_datamart_root_with_its_query_file_passes(endpoints) -> None:
    _, root, write = endpoints
    (root / "config" / "queries").mkdir()
    (root / "config/queries/accounts.graphql").write_text("query { accounts { id } }")
    write(
        erp(
            "shadow",
            datamart={
                "ready": False,
                "root_field": "accounts",
                "query_file": "config/queries/accounts.graphql",
            },
        )
    )

    assert messages(endpoints) == []


@pytest.mark.unit
def test_an_unmapped_placeholder_may_lack_a_query_file(endpoints) -> None:
    endpoints[2](erp("placeholder", datamart={"ready": False, "query_file": "config/queries/x"}))

    assert messages(endpoints) == []


def period_scanned(**overrides) -> dict:
    cfg = erp(
        "project_transactions",
        api_endpoint="/v1/projecttransaction",
        primary_key=["module", "refNbr"],
        sync_pattern="scoped_wrapper_only",
        required_query=["fromPeriod", "toPeriod"],
        period_scan={"first_period": "202301", "query_params": ["fromPeriod", "toPeriod"]},
        validation_fields=["module", "refNbr"],
    )
    cfg.update(overrides)
    return cfg


@pytest.mark.unit
def test_a_period_scanned_endpoint_passes(endpoints) -> None:
    endpoints[2](period_scanned())

    assert messages(endpoints) == []


@pytest.mark.unit
def test_the_period_params_count_as_supplied_at_runtime(endpoints) -> None:
    endpoints[2](
        period_scanned(
            required_query=["periodId"],
            period_scan={"first_period": "202001", "query_params": ["periodId"]},
            sync_pattern="incremental_upsert",
        )
    )

    assert messages(endpoints) == []


@pytest.mark.unit
def test_scoped_wrapper_only_without_period_scan_is_rejected(endpoints) -> None:
    cfg = period_scanned()
    del cfg["period_scan"]
    endpoints[2](cfg)

    assert any("period_scan.first_period" in m for m in messages(endpoints))


@pytest.mark.unit
@pytest.mark.parametrize(
    "scan",
    [
        {"first_period": "2023", "query_params": ["periodId"]},
        {"first_period": "202313", "query_params": ["periodId"]},
        {"first_period": "202301", "query_params": []},
        {"first_period": "202301", "query_params": ["periodId", "periodId"]},
        {"first_period": "202301", "query_params": ["periodId"], "look_ahead_months": 99},
        {"query_params": ["periodId"]},
    ],
)
def test_a_malformed_period_scan_is_a_schema_violation(endpoints, scan) -> None:
    endpoints[2](period_scanned(period_scan=scan, required_query=[]))

    assert messages(endpoints) != []


@pytest.mark.unit
def test_a_period_field_in_the_primary_key_passes(endpoints) -> None:
    cfg = period_scanned(primary_key=["module", "refNbr", "_scan_period"])
    cfg["period_scan"]["period_field"] = "_scan_period"
    endpoints[2](cfg)

    assert messages(endpoints) == []


@pytest.mark.unit
def test_a_period_field_outside_the_primary_key_is_rejected(endpoints) -> None:
    cfg = period_scanned()
    cfg["period_scan"]["period_field"] = "_scan_period"
    endpoints[2](cfg)

    assert any("period_field" in m for m in messages(endpoints))


@pytest.mark.unit
def test_a_resuming_period_scan_needs_a_modified_field(endpoints) -> None:
    resuming = {"first_period": "202301", "query_params": ["periodId"], "resume": True}
    endpoints[2](
        period_scanned(
            required_query=["periodId"],
            period_scan=resuming,
            sync_pattern="incremental_upsert",
            modified_field="lastModifiedDateTime",
        )
    )
    assert messages(endpoints) == []

    endpoints[2](
        period_scanned(
            required_query=["periodId"], period_scan=resuming, sync_pattern="incremental_upsert"
        )
    )
    assert any("resume needs a modified_field" in m for m in messages(endpoints))


@pytest.mark.unit
def test_a_default_query_value_for_a_period_param_is_rejected(endpoints) -> None:
    endpoints[2](period_scanned(default_query={"fromPeriod": "202001"}))

    assert any("overwritten by period_scan" in m for m in messages(endpoints))


@pytest.mark.unit
def test_schema_violations_are_reported_with_their_location(endpoints) -> None:
    cfg = erp()
    del cfg["bigquery_table"]
    endpoints[2](cfg)

    assert any("bigquery_table" in m for m in messages(endpoints))


# --- main() -----------------------------------------------------------------------------


@pytest.mark.unit
def test_main_exits_zero_and_reports_the_count_when_everything_is_valid(
    endpoints, monkeypatch, capsys
) -> None:
    endpoints[2](erp())
    monkeypatch.setattr(config_validate, "_repo_root", lambda: endpoints[1])

    config_validate.main()

    assert "validated 1 endpoint config file(s)" in capsys.readouterr().out


@pytest.mark.unit
def test_main_exits_one_and_lists_every_failure(endpoints, monkeypatch, capsys) -> None:
    endpoints[2](erp("bad", api_endpoint="get_accounts"))
    monkeypatch.setattr(config_validate, "_repo_root", lambda: endpoints[1])

    with pytest.raises(SystemExit) as exit_info:
        config_validate.main()

    assert exit_info.value.code == 1
    assert "bad.json" in capsys.readouterr().out


@pytest.mark.unit
def test_main_exits_two_when_the_endpoints_dir_or_schema_is_missing(tmp_path, monkeypatch) -> None:
    monkeypatch.setattr(config_validate, "_repo_root", lambda: tmp_path)
    with pytest.raises(SystemExit) as no_dir:
        config_validate.main()
    assert no_dir.value.code == 2

    (tmp_path / "config" / "endpoints").mkdir(parents=True)
    with pytest.raises(SystemExit) as no_schema:
        config_validate.main()
    assert no_schema.value.code == 2
