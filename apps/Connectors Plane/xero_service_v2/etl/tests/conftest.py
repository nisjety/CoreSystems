"""Offline by default: local ADC must never turn unit tests into GCP writes."""

import os
from urllib.parse import urlparse

import pytest


_LIVE_MODULES = frozenset({
    "test_bq_reader.py", "test_bq_writer.py", "test_bank_transactions.py", "test_gcs_reader.py",
})
_GUARD = pytest.MonkeyPatch()


def pytest_configure(config):
    if os.environ.get("XERO_RUN_LIVE_ETL_TESTS") == "1":
        from etl.tests.live_config import test_target
        test_target()
        return

    def forbidden(*args, **kwargs):
        raise AssertionError("Live Google clients/ADC are disabled in offline ETL tests")

    import requests
    original_request = requests.sessions.Session.request

    def guarded_request(self, method, url, *args, **kwargs):
        host = urlparse(url).hostname or ""
        if host == "googleapis.com" or host.endswith(".googleapis.com"):
            raise AssertionError("Google API networking is disabled in offline ETL tests")
        return original_request(self, method, url, *args, **kwargs)

    _GUARD.setattr("google.auth.default", forbidden)
    _GUARD.setattr("google.cloud.bigquery.Client", forbidden)
    _GUARD.setattr("google.cloud.storage.Client", forbidden)
    _GUARD.setattr("requests.sessions.Session.request", guarded_request)


def pytest_sessionfinish(session, exitstatus):
    _GUARD.undo()


def pytest_ignore_collect(collection_path, config):
    return (collection_path.name in _LIVE_MODULES
            and os.environ.get("XERO_RUN_LIVE_ETL_TESTS") != "1")


@pytest.fixture(autouse=True)
def prevent_implicit_live_credentials(monkeypatch):
    if os.environ.get("XERO_RUN_LIVE_ETL_TESTS") != "1":
        def forbidden(*args, **kwargs):
            raise AssertionError("Live ADC is disabled in offline ETL tests")
        monkeypatch.setattr("google.auth.default", forbidden)
