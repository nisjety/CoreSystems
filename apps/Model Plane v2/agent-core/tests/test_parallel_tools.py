"""Tests for Phase Q — Parallel Read-Only Tool Execution."""

from __future__ import annotations

import asyncio
import json
from unittest.mock import AsyncMock, MagicMock

import pytest

from app.domain import ActionKind, ActionStatus, AgentAction, ActionTarget
from app.turn_loop import (
    READONLY_TOOLS,
    _execute_parallel,
    _is_readonly,
    _make_action,
    _parse_actions,
)


# ---------------------------------------------------------------------------
# _is_readonly
# ---------------------------------------------------------------------------


class TestIsReadonly:
    def test_known_readonly_tool(self) -> None:
        assert _is_readonly("search") is True
        assert _is_readonly("retrieve") is True
        assert _is_readonly("memory_read") is True

    def test_read_prefix(self) -> None:
        assert _is_readonly("read_file") is True
        assert _is_readonly("read_anything") is True

    def test_write_tool_not_readonly(self) -> None:
        assert _is_readonly("write_file") is False
        assert _is_readonly("bash") is False

    def test_all_readonly_tools_in_frozenset(self) -> None:
        for tool in READONLY_TOOLS:
            assert _is_readonly(tool) is True


# ---------------------------------------------------------------------------
# _parse_actions — single action
# ---------------------------------------------------------------------------


class TestParseActionsSingle:
    def test_single_tool_call(self) -> None:
        raw = json.dumps({"kind": "tool_call", "name": "bash", "input": {"cmd": "ls"}})
        actions = _parse_actions(raw, 0)
        assert len(actions) == 1
        assert actions[0].kind == ActionKind.TOOL_CALL
        assert actions[0].name == "bash"

    def test_single_reasoning(self) -> None:
        raw = json.dumps({"kind": "reasoning", "input": {"query": "think"}})
        actions = _parse_actions(raw, 0)
        assert len(actions) == 1
        assert actions[0].kind == ActionKind.REASONING
        assert actions[0].readonly is False

    def test_final_response(self) -> None:
        raw = json.dumps({"kind": "final_response", "input": {"content": "done"}})
        actions = _parse_actions(raw, 0)
        assert len(actions) == 1
        assert actions[0].kind == ActionKind.FINAL_RESPONSE

    def test_strips_markdown_fences(self) -> None:
        body = json.dumps({"kind": "reasoning", "input": {}})
        raw = f"```json\n{body}\n```"
        actions = _parse_actions(raw, 0)
        assert len(actions) == 1

    def test_invalid_json_returns_empty(self) -> None:
        assert _parse_actions("not json", 0) == []

    def test_readonly_set_for_known_tool(self) -> None:
        raw = json.dumps({"kind": "tool_call", "name": "search", "input": {"q": "x"}})
        actions = _parse_actions(raw, 0)
        assert actions[0].readonly is True

    def test_readonly_false_for_write_tool(self) -> None:
        raw = json.dumps({"kind": "tool_call", "name": "bash", "input": {}})
        actions = _parse_actions(raw, 0)
        assert actions[0].readonly is False


# ---------------------------------------------------------------------------
# _parse_actions — batch array
# ---------------------------------------------------------------------------


class TestParseActionsArray:
    def _make_tool(self, name: str) -> dict:
        return {"kind": "tool_call", "name": name, "input": {}}

    def test_all_readonly_returns_batch(self) -> None:
        raw = json.dumps([self._make_tool("search"), self._make_tool("retrieve")])
        actions = _parse_actions(raw, 0)
        assert len(actions) == 2
        assert all(a.kind == ActionKind.TOOL_CALL for a in actions)
        assert all(a.readonly for a in actions)

    def test_non_readonly_in_array_falls_back_to_first(self) -> None:
        """If any item in a batch is non-readonly, fall back to first item only."""
        raw = json.dumps([self._make_tool("search"), self._make_tool("bash")])
        actions = _parse_actions(raw, 0)
        # Falls back to single item
        assert len(actions) == 1
        assert actions[0].name == "search"

    def test_empty_array_returns_empty(self) -> None:
        assert _parse_actions("[]", 0) == []

    def test_preserves_order(self) -> None:
        names = ["search", "retrieve", "memory_read"]
        raw = json.dumps([self._make_tool(n) for n in names])
        actions = _parse_actions(raw, 0)
        assert [a.name for a in actions] == names


# ---------------------------------------------------------------------------
# _execute_parallel
# ---------------------------------------------------------------------------


class TestExecuteParallel:
    @pytest.mark.asyncio
    async def test_executes_all_actions(self) -> None:
        executed: list[str] = []

        async def fake_execute(run: object, action: AgentAction) -> AgentAction:
            executed.append(action.name)
            action.status = ActionStatus.COMPLETED
            action.output = f"result:{action.name}"
            return action

        run = MagicMock()
        actions = [
            AgentAction(
                kind=ActionKind.TOOL_CALL,
                target=ActionTarget.AI_CORE,
                name=n,
                readonly=True,
            )
            for n in ["search", "retrieve"]
        ]

        results = await _execute_parallel(actions, run, fake_execute)
        assert len(results) == 2
        assert set(executed) == {"search", "retrieve"}

    @pytest.mark.asyncio
    async def test_preserves_result_order(self) -> None:
        """asyncio.gather preserves order even if coroutines finish at different times."""

        async def fake_execute(run: object, action: AgentAction) -> AgentAction:
            if action.name == "slow":
                await asyncio.sleep(0.01)
            action.status = ActionStatus.COMPLETED
            action.output = action.name
            return action

        run = MagicMock()
        actions = [
            AgentAction(kind=ActionKind.TOOL_CALL, target=ActionTarget.AI_CORE, name="slow", readonly=True),
            AgentAction(kind=ActionKind.TOOL_CALL, target=ActionTarget.AI_CORE, name="fast", readonly=True),
        ]
        results = await _execute_parallel(actions, run, fake_execute)
        assert results[0].name == "slow"
        assert results[1].name == "fast"
