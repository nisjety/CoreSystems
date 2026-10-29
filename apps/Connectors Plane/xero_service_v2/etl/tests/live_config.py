"""Explicit live-test target, never an ordinary or legacy staging dataset."""

import os
import re


def test_target():
    if os.environ.get("XERO_RUN_LIVE_ETL_TESTS") != "1":
        raise RuntimeError("Live ETL tests require explicit XERO_RUN_LIVE_ETL_TESTS=1")
    project = os.environ.get("XERO_ETL_TEST_PROJECT", "")
    dataset = os.environ.get("XERO_ETL_TEST_DATASET", "")
    if (not re.fullmatch(r"[A-Za-z][A-Za-z0-9-]*", project)
            or not re.fullmatch(r"xero_test_[A-Za-z0-9_]+", dataset)):
        raise RuntimeError("Live ETL tests require an isolated xero_test_ dataset and explicit project")
    return project, dataset
