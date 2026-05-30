"""Tests for Phase V — Typed CoordinationResult."""

from __future__ import annotations

from unittest.mock import MagicMock

import pytest

from app.coordination_result import ChildRunSummary, CoordinationResult
from app.domain import RunRecord, RunStatus


def _make_child(
    *,
    status: RunStatus = RunStatus.COMPLETED,
    final_output: str | None = "output",
    error: str | None = None,
    agent_id: str = "worker",
) -> RunRecord:
    """Create a minimal RunRecord mock for testing."""
    rec = MagicMock(spec=RunRecord)
    rec.id = f"run-{agent_id}"
    rec.status = status
    rec.final_output = final_output
    rec.error = error
    rec.context = {"agent_id": agent_id}
    return rec


# ---------------------------------------------------------------------------
# ChildRunSummary
# ---------------------------------------------------------------------------


class TestChildRunSummary:
    def test_from_completed_run(self) -> None:
        child = _make_child(agent_id="sec", status=RunStatus.COMPLETED, final_output="ok")
        s = ChildRunSummary(
            run_id=child.id,
            agent_id="sec",
            status=child.status.value,
            final_output=child.final_output,
        )
        assert s.run_id == "run-sec"
        assert s.status == "completed"
        assert s.final_output == "ok"
        assert s.error is None

    def test_from_failed_run(self) -> None:
        s = ChildRunSummary(
            run_id="r1",
            agent_id="w1",
            status="failed",
            final_output=None,
            error="boom",
        )
        assert s.status == "failed"
        assert s.error == "boom"


# ---------------------------------------------------------------------------
# CoordinationResult
# ---------------------------------------------------------------------------


class TestCoordinationResult:
    def test_from_children_all_completed(self) -> None:
        children = [
            _make_child(agent_id="a", final_output="out-a"),
            _make_child(agent_id="b", final_output="out-b"),
        ]
        result = CoordinationResult.from_children(children)
        assert result.child_count == 2
        assert result.all_done is True
        assert result.outputs == ["out-a", "out-b"]
        assert len(result.children) == 2
        assert result.status_summary == {"completed": 2}

    def test_from_children_mixed_status(self) -> None:
        children = [
            _make_child(agent_id="a", status=RunStatus.COMPLETED, final_output="ok"),
            _make_child(agent_id="b", status=RunStatus.RUNNING, final_output=None),
        ]
        result = CoordinationResult.from_children(children)
        assert result.child_count == 2
        assert result.all_done is False
        assert result.outputs == ["ok"]
        assert result.status_summary == {"completed": 1, "running": 1}

    def test_from_children_empty(self) -> None:
        result = CoordinationResult.from_children([])
        assert result.child_count == 0
        assert result.all_done is True
        assert result.outputs == []

    def test_from_children_with_synthesis(self) -> None:
        children = [_make_child(agent_id="a", final_output="x")]
        result = CoordinationResult.from_children(children, synthesis="combined")
        assert result.synthesis == "combined"

    def test_model_dump(self) -> None:
        children = [_make_child(agent_id="a", final_output="res")]
        result = CoordinationResult.from_children(children)
        d = result.model_dump()
        assert isinstance(d, dict)
        assert d["child_count"] == 1
        assert d["all_done"] is True
        assert d["outputs"] == ["res"]
        assert len(d["children"]) == 1
