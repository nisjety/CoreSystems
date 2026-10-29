"""The live-stack suite: one pytest per YAML case (EVAL_LIVE=1 to run).

`make eval` sets EVAL_LIVE=1 and writes the scored report via conftest's
session hook; plain pytest (CI) skips these and runs only the self-tests.
"""

from __future__ import annotations

import pytest

from eval_lab.cases import CASES_DIR, load_cases
from eval_lab.live_state import RESULTS, live_enabled
from eval_lab.runner import run_case


@pytest.mark.skipif(not live_enabled(), reason="live eval disabled (set EVAL_LIVE=1 / make eval)")
@pytest.mark.parametrize("case", load_cases(CASES_DIR), ids=lambda c: c.id)
def test_case(client, case) -> None:
    result = run_case(client, case)
    RESULTS.append(result)
    if result.skipped:
        pytest.skip(result.skip_reason)
    failing = [
        f"{m.metric}: {m.detail}"
        for m in result.metrics
        if not m.passed and not m.skipped
    ]
    excerpt = (result.outcome.text[:300] + "…") if result.outcome and len(result.outcome.text) > 300 else (result.outcome.text if result.outcome else "")
    assert result.passed, "; ".join(failing) + f" | answer: {excerpt!r}"
