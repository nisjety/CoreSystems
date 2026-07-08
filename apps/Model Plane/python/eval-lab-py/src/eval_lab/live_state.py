"""Shared state between the live test module and conftest's session hook.

Lives in the package (not tests/) so both import it absolutely — tests/ is
not a package and relative imports there fail under pytest's rootdir logic.
"""

from __future__ import annotations

import os

from eval_lab.types import CaseResult

RESULTS: list[CaseResult] = []


def live_enabled() -> bool:
    return os.environ.get("EVAL_LIVE") == "1"
