import pytest

from etl.tests.live_config import test_target as live_target


def test_live_target_requires_explicit_opt_in(monkeypatch):
    monkeypatch.delenv("XERO_RUN_LIVE_ETL_TESTS", raising=False)
    with pytest.raises(RuntimeError, match="explicit"):
        live_target()


@pytest.mark.parametrize("dataset", ["staging_xero", "dw_staging_xero", "", "xero_test_x;DROP"])
def test_live_target_rejects_business_datasets(monkeypatch, dataset):
    monkeypatch.setenv("XERO_RUN_LIVE_ETL_TESTS", "1")
    monkeypatch.setenv("XERO_ETL_TEST_PROJECT", "prj-dw-dev")
    monkeypatch.setenv("XERO_ETL_TEST_DATASET", dataset)
    with pytest.raises(RuntimeError, match="isolated"):
        live_target()


def test_live_target_accepts_only_named_test_dataset(monkeypatch):
    monkeypatch.setenv("XERO_RUN_LIVE_ETL_TESTS", "1")
    monkeypatch.setenv("XERO_ETL_TEST_PROJECT", "prj-dw-dev")
    monkeypatch.setenv("XERO_ETL_TEST_DATASET", "xero_test_fresh")
    assert live_target() == ("prj-dw-dev", "xero_test_fresh")


def test_repository_root_discovery_installs_guard_before_nested_module_import(tmp_path):
    import os
    from pathlib import Path
    import subprocess
    import sys

    root = Path(__file__).resolve().parents[2]
    nested = tmp_path / "nested"
    nested.mkdir()
    (nested / "conftest.py").write_text((root / "etl/tests/conftest.py").read_text())
    (nested / "test_guard_probe.py").write_text(
        "from google.cloud import bigquery\n"
        "assert bigquery.Client.__name__ == 'forbidden'\n"
        "def test_guard_is_installed():\n"
        "    assert bigquery.Client.__name__ == 'forbidden'\n"
    )
    env = {**os.environ, "PYTHONPATH": str(root)}
    env.pop("XERO_RUN_LIVE_ETL_TESTS", None)
    result = subprocess.run(
        [sys.executable, "-m", "pytest", "-q", "-o", "addopts=", str(tmp_path)],
        cwd=tmp_path, env=env, capture_output=True, text=True, timeout=30,
    )
    assert result.returncode == 0, result.stdout + result.stderr
