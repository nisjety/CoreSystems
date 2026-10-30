"""Unit tests for the bronze schema compatibility assertion."""

from __future__ import annotations

from pathlib import Path

import pytest

from vsv2_tooling.dataform.bronze_assertions import (
    ORCHESTRATOR_FIELDS,
    SYNC_INFRASTRUCTURE_FIELDS,
    _parse_bronze_sqlx_schema,
    assert_bronze_compat,
    assert_bronze_schema_compatibility,
)

_BRONZE_DIR = Path(
    "/Volumes/Lagring/Aquatiq/Aquatiq integrasjonen "
    "/apps/visma_service/Dataform/definitions/bronze/visma"
)


def _write_sqlx(tmp_path: Path, name: str, fields: list[dict]) -> Path:
    import json as _json

    path = tmp_path / f"{name}.sqlx"
    body = (
        "config {\n"
        '  type: "declaration",\n'
        f'  schema: "dw_1_bronze_visma",\n'
        f'  name: "{name}"\n'
        "}\n\n"
        "//schema\n"
        f"{_json.dumps(fields, indent=2)}\n"
    )
    path.write_text(body, encoding="utf-8")
    return path


@pytest.mark.unit
def test_parse_bronze_sqlx_schema_finds_fields(tmp_path: Path) -> None:
    path = _write_sqlx(
        tmp_path,
        "visma_widgets",
        [
            {"name": "id", "type": "STRING", "mode": ""},
            {"name": "name", "type": "STRING", "mode": ""},
        ],
    )
    schema = _parse_bronze_sqlx_schema(path)
    assert [f["name"] for f in schema] == ["id", "name"]


@pytest.mark.unit
def test_parse_bronze_sqlx_schema_raises_on_missing_block(tmp_path: Path) -> None:
    path = tmp_path / "visma_nope.sqlx"
    path.write_text('config { type: "declaration" }\n', encoding="utf-8")
    with pytest.raises(ValueError, match="no '//schema"):
        _parse_bronze_sqlx_schema(path)


@pytest.mark.unit
def test_parse_bronze_sqlx_schema_raises_on_bad_json(tmp_path: Path) -> None:
    path = tmp_path / "visma_bad.sqlx"
    path.write_text("config { }\n//schema\n[ { not valid json } ]\n", encoding="utf-8")
    with pytest.raises(ValueError, match="invalid JSON"):
        _parse_bronze_sqlx_schema(path)


@pytest.mark.unit
def test_assert_bronze_compat_unknown_normalizer(tmp_path: Path) -> None:
    path = _write_sqlx(tmp_path, "visma_x", [{"name": "id", "type": "STRING", "mode": ""}])
    with pytest.raises(ValueError, match="Unknown normalizer"):
        assert_bronze_compat(
            normalizer_name="normalize_nope",
            sample_dm_record={"id": "1"},
            bronze_sqlx_path=path,
        )


@pytest.mark.unit
def test_assert_bronze_compat_clean_match_returns_empty(tmp_path: Path) -> None:
    """Customer normalizer + the real customers bronze sqlx should match."""
    pytest.importorskip("vsv2_tooling.normalizers.customer")
    bronze_path = _BRONZE_DIR / "visma_customers.sqlx"
    if not bronze_path.exists():
        pytest.skip(f"bronze sqlx not available at {bronze_path}")

    sample = {
        "id": "C-1",
        "number": "1001",
        "name": "Acme Foods AS",
        "mainLocationId": "L-1",
        "updatedInDataMartDateTime": "2026-05-20T10:00:00Z",
    }
    mismatches = assert_bronze_compat(
        normalizer_name="normalize_customer_with_locations",
        sample_dm_record=sample,
        bronze_sqlx_path=bronze_path,
    )
    extras = [m for m in mismatches if m.startswith("extra:")]
    assert extras == [], f"customer normalizer drift vs bronze: {extras}"


@pytest.mark.unit
def test_assert_bronze_compat_detects_missing_field(tmp_path: Path) -> None:
    path = _write_sqlx(
        tmp_path,
        "visma_widgets",
        [
            {"name": "id", "type": "STRING", "mode": ""},
            {"name": "name", "type": "STRING", "mode": ""},
            {"name": "price", "type": "FLOAT", "mode": ""},
        ],
    )
    from vsv2_tooling.normalizers import REGISTRY

    def fake_normalizer(_dm: dict) -> dict:
        return {"id": "1", "name": "x"}  # missing 'price'

    REGISTRY["__test_widget"] = fake_normalizer
    try:
        mismatches = assert_bronze_compat(
            normalizer_name="__test_widget",
            sample_dm_record={},
            bronze_sqlx_path=path,
        )
        assert any("'price'" in m and m.startswith("missing:") for m in mismatches)
    finally:
        del REGISTRY["__test_widget"]


@pytest.mark.unit
def test_assert_bronze_compat_detects_extra_field_pre_phase8b(tmp_path: Path) -> None:
    path = _write_sqlx(tmp_path, "visma_widgets", [{"name": "id", "type": "STRING", "mode": ""}])
    from vsv2_tooling.normalizers import REGISTRY

    def fake_normalizer(_dm: dict) -> dict:
        return {"id": "1", "shiny_new_field": "boom"}

    REGISTRY["__test_widget"] = fake_normalizer
    try:
        mismatches = assert_bronze_compat(
            normalizer_name="__test_widget",
            sample_dm_record={},
            bronze_sqlx_path=path,
        )
        assert any("'shiny_new_field'" in m and m.startswith("extra:") for m in mismatches)
    finally:
        del REGISTRY["__test_widget"]


@pytest.mark.unit
def test_assert_bronze_compat_allow_additive_passes_extras(tmp_path: Path) -> None:
    path = _write_sqlx(tmp_path, "visma_widgets", [{"name": "id", "type": "STRING", "mode": ""}])
    from vsv2_tooling.normalizers import REGISTRY

    def fake_normalizer(_dm: dict) -> dict:
        return {"id": "1", "phase8b_new": "ok"}

    REGISTRY["__test_widget"] = fake_normalizer
    try:
        mismatches = assert_bronze_compat(
            normalizer_name="__test_widget",
            sample_dm_record={},
            bronze_sqlx_path=path,
            allow_additive=True,
        )
        assert all(not m.startswith("extra:") for m in mismatches)
    finally:
        del REGISTRY["__test_widget"]


@pytest.mark.unit
def test_orchestrator_fields_are_ignored(tmp_path: Path) -> None:
    """tenant_id / _source / _synced_at should never trigger 'extra' flags."""
    path = _write_sqlx(tmp_path, "visma_widgets", [{"name": "id", "type": "STRING", "mode": ""}])
    from vsv2_tooling.normalizers import REGISTRY

    def fake_normalizer(_dm: dict) -> dict:
        return {"id": "1", "tenant_id": "t-1", "_source": "erp_rest", "_synced_at": "now"}

    REGISTRY["__test_widget"] = fake_normalizer
    try:
        mismatches = assert_bronze_compat(
            normalizer_name="__test_widget",
            sample_dm_record={},
            bronze_sqlx_path=path,
        )
        assert mismatches == []
    finally:
        del REGISTRY["__test_widget"]


@pytest.mark.unit
def test_sync_infrastructure_fields_are_ignored(tmp_path: Path) -> None:
    """The sync layer adds syncId/syncedAt/etc. — normalizer not responsible."""
    path = _write_sqlx(
        tmp_path,
        "visma_widgets",
        [
            {"name": "syncId", "type": "STRING", "mode": ""},
            {"name": "syncedAt", "type": "TIMESTAMP", "mode": ""},
            {"name": "tenantId", "type": "STRING", "mode": ""},
            {"name": "id", "type": "STRING", "mode": ""},
        ],
    )
    from vsv2_tooling.normalizers import REGISTRY

    def fake_normalizer(_dm: dict) -> dict:
        return {"id": "1"}  # does not emit syncId/syncedAt/tenantId — that's fine

    REGISTRY["__test_widget"] = fake_normalizer
    try:
        mismatches = assert_bronze_compat(
            normalizer_name="__test_widget",
            sample_dm_record={},
            bronze_sqlx_path=path,
        )
        assert mismatches == []
    finally:
        del REGISTRY["__test_widget"]


@pytest.mark.unit
def test_assert_bronze_schema_compatibility_alias(tmp_path: Path) -> None:
    path = _write_sqlx(tmp_path, "visma_widgets", [{"name": "id", "type": "STRING", "mode": ""}])
    from vsv2_tooling.normalizers import REGISTRY

    def fake_normalizer(_dm: dict) -> dict:
        return {"id": "1"}

    REGISTRY["__test_widget"] = fake_normalizer
    try:
        mismatches = assert_bronze_schema_compatibility(
            normalizer_name="__test_widget",
            sample_dm_record={},
            bronze_sqlx_path=path,
        )
        assert mismatches == []
    finally:
        del REGISTRY["__test_widget"]


@pytest.mark.unit
def test_assert_bronze_compat_non_dict_output(tmp_path: Path) -> None:
    path = _write_sqlx(tmp_path, "visma_widgets", [{"name": "id", "type": "STRING", "mode": ""}])
    from vsv2_tooling.normalizers import REGISTRY

    def fake_normalizer(_dm: dict) -> list:  # type: ignore[return-type]
        return ["not", "a", "dict"]  # type: ignore[return-value]

    REGISTRY["__test_widget"] = fake_normalizer  # type: ignore[assignment]
    try:
        mismatches = assert_bronze_compat(
            normalizer_name="__test_widget",
            sample_dm_record={},
            bronze_sqlx_path=path,
        )
        assert any("expected dict" in m for m in mismatches)
    finally:
        del REGISTRY["__test_widget"]


@pytest.mark.unit
def test_field_set_constants_are_disjoint() -> None:
    """Orchestrator fields and sync infrastructure fields should not overlap."""
    assert set() == ORCHESTRATOR_FIELDS & SYNC_INFRASTRUCTURE_FIELDS
