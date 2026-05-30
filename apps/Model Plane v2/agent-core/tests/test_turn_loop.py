"""Tests for Phase G — Reactive turn loop."""

from __future__ import annotations

import json
import pytest

from app.domain import ActionKind, ActionStatus, ActionTarget
from app.turn_loop import (
    _format_action_result,
    _parse_single_action,
)


# ---------------------------------------------------------------------------
# _parse_single_action
# ---------------------------------------------------------------------------


class TestParseSingleAction:
    def test_tool_call(self) -> None:
        raw = json.dumps({"kind": "tool_call", "name": "read_file", "input": {"path": "/a.py"}})
        action = _parse_single_action(raw, 0)
        assert action is not None
        assert action.kind == ActionKind.TOOL_CALL
        assert action.name == "read_file"
        assert action.target == ActionTarget.AI_CORE

    def test_reasoning(self) -> None:
        raw = json.dumps({"kind": "reasoning", "input": {"query": "think about it"}})
        action = _parse_single_action(raw, 1)
        assert action is not None
        assert action.kind == ActionKind.REASONING
        assert action.target == ActionTarget.INTERNAL

    def test_final_response(self) -> None:
        raw = json.dumps({"kind": "final_response", "input": {"content": "done!"}})
        action = _parse_single_action(raw, 2)
        assert action is not None
        assert action.kind == ActionKind.FINAL_RESPONSE

    def test_mcp_tool_targets_agent_core(self) -> None:
        raw = json.dumps({"kind": "tool_call", "name": "mcp:server:tool"})
        action = _parse_single_action(raw, 0)
        assert action is not None
        assert action.target == ActionTarget.AGENT_CORE

    def test_invalid_json_returns_none(self) -> None:
        assert _parse_single_action("not json", 0) is None

    def test_code_block_wrapper(self) -> None:
        raw = '```json\n{"kind": "reasoning", "input": {"query": "test"}}\n```'
        action = _parse_single_action(raw, 0)
        assert action is not None
        assert action.kind == ActionKind.REASONING

    def test_invalid_kind_returns_none(self) -> None:
        raw = json.dumps({"kind": "nonexistent", "name": "x"})
        assert _parse_single_action(raw, 0) is None

    def test_default_name(self) -> None:
        raw = json.dumps({"kind": "reasoning"})
        action = _parse_single_action(raw, 5)
        assert action is not None
        assert action.name == "turn_5"


# ---------------------------------------------------------------------------
# _format_action_result
# ---------------------------------------------------------------------------


class TestFormatActionResult:
    def test_completed_dict(self) -> None:
        from app.domain import AgentAction

        action = AgentAction(
            kind=ActionKind.TOOL_CALL,
            target=ActionTarget.AI_CORE,
            name="test",
            status=ActionStatus.COMPLETED,
            output={"key": "value"},
        )
        result = _format_action_result(action)
        assert '"key"' in result
        assert '"value"' in result

    def test_completed_string(self) -> None:
        from app.domain import AgentAction

        action = AgentAction(
            kind=ActionKind.TOOL_CALL,
            target=ActionTarget.AI_CORE,
            name="test",
            status=ActionStatus.COMPLETED,
            output="hello",
        )
        assert _format_action_result(action) == "hello"

    def test_failed(self) -> None:
        from app.domain import AgentAction

        action = AgentAction(
            kind=ActionKind.TOOL_CALL,
            target=ActionTarget.AI_CORE,
            name="test",
            status=ActionStatus.FAILED,
            error="timeout",
        )
        result = _format_action_result(action)
        assert "failed" in result.lower()
        assert "timeout" in result

    def test_skipped(self) -> None:
        from app.domain import AgentAction

        action = AgentAction(
            kind=ActionKind.TOOL_CALL,
            target=ActionTarget.AI_CORE,
            name="test",
            status=ActionStatus.SKIPPED,
            error="blocked by hook",
        )
        result = _format_action_result(action)
        assert "skipped" in result.lower()
