"""The sales-order count gate's rule. Pure: no Visma, no BigQuery."""

from __future__ import annotations

import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent))

from verify_sales_order_counts import judge


@pytest.mark.unit
def test_silver_behind_by_no_more_than_the_changes_since_the_sync_passes() -> None:
    assert judge(visma_total=22_314, changed=209, silver_total=22_105).word == "PASS"


@pytest.mark.unit
def test_more_missing_than_could_have_changed_fails() -> None:
    """The bug this gate exists for: silver held 21,544 of Aquatiq AS's orders, ~200 changed."""
    verdict = judge(visma_total=22_314, changed=209, silver_total=21_544)

    assert verdict.word == "FAIL"
    assert "770" in verdict.reason


@pytest.mark.unit
def test_silver_ahead_of_visma_is_a_warning_not_a_failure() -> None:
    verdict = judge(visma_total=100, changed=0, silver_total=103)

    assert verdict.word == "WARN"
    assert "3 more than Visma" in verdict.reason


@pytest.mark.unit
def test_exactly_equal_passes_with_nothing_missing() -> None:
    assert judge(visma_total=50, changed=0, silver_total=50).word == "PASS"
