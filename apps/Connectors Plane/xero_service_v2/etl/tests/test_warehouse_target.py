import pytest


def test_ordinary_staging_dataset_defaults_to_active_gcp_target(monkeypatch):
    monkeypatch.delenv("XERO_STAGING_DATASET", raising=False)

    from etl.xero.warehouse import ordinary_staging_dataset

    assert ordinary_staging_dataset() == "dw_staging_xero"


def test_ordinary_staging_dataset_allows_valid_explicit_target(monkeypatch):
    monkeypatch.setenv("XERO_STAGING_DATASET", "custom_staging_xero")

    from etl.xero.warehouse import ordinary_staging_dataset

    assert ordinary_staging_dataset() == "custom_staging_xero"


@pytest.mark.parametrize("value", ["", "contains-dash", "dataset; DROP TABLE x"])
def test_ordinary_staging_dataset_rejects_invalid_identifier(monkeypatch, value):
    monkeypatch.setenv("XERO_STAGING_DATASET", value)

    from etl.xero.warehouse import ordinary_staging_dataset

    with pytest.raises(ValueError, match="XERO_STAGING_DATASET"):
        ordinary_staging_dataset()
