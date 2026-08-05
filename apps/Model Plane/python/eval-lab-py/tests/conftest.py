"""Shared fixtures + the end-of-session report writer.

pytest only honors pytest_sessionfinish in conftest.py — defining it in a
test module silently does nothing (empirically confirmed on the first live
run: no report was written).
"""

from __future__ import annotations

import os
from pathlib import Path

import pytest

from eval_lab.client import EvalEnv, VerevonClient
from eval_lab.live_state import RESULTS, live_enabled
from eval_lab.report import write_report


@pytest.fixture(scope="session")
def client():
    env = EvalEnv()
    ready, reason = env.ready()
    if not ready:
        pytest.skip(f"live stack not configured: {reason}")
    instance = VerevonClient(env)
    yield instance
    instance.close()


def pytest_sessionfinish(session, exitstatus):  # noqa: ANN001, D103
    if live_enabled() and RESULTS:
        # parents[5] is the repo root, not parents[4]. This file lives at
        # apps/Model Plane/python/eval-lab-py/tests/conftest.py, so [4] is
        # `apps/` — which has no docs/eval-reports, meaning every live report
        # was written into a directory nobody reads.
        repo_root = Path(__file__).resolve().parents[5]
        reports_dir = Path(
            os.environ.get(
                "EVAL_REPORTS_DIR",
                str(repo_root / "docs" / "eval-reports"),
            )
        )
        path = write_report(RESULTS, reports_dir)
        print(f"\neval report written: {path}")
