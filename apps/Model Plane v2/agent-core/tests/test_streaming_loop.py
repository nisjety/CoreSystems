"""Tests for Phase A1: Streaming turn loop, StopReason, CacheSafeParams, circuit breaker."""

from __future__ import annotations

import json
import pytest
from dataclasses import dataclass, field
from typing import Any
from unittest.mock import AsyncMock, MagicMock, patch

from app.domain import (
    ActionKind,
    ActionStatus,
    ActionTarget,
    AgentAction,
    AgentType,
    CacheSafeParams,
    ExecutionPolicy,
    RunMode,
    RunRecord,
    RunStatus,
    StopReason,
    TurnEvent,
    TurnEventKind,
)
from app.streaming_loop import (
    CacheBreakDetector,
    CompactCircuitBreaker,
    MAX_CONSECUTIVE_COMPACT_FAILURES,
    StreamingTurnLoopState,
    run_streaming_turn_loop,
    streaming_result_from_events,
)
from app.cost_tracker import TurnUsage


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _make_run(**overrides: Any) -> RunRecord:
    defaults = {
        "session_id": "sess-1",
        "user_id": "u-1",
        "org_id": "org-1",
        "goal": "test goal",
        "status": RunStatus.RUNNING,
        "policy": ExecutionPolicy(max_turns=5, token_budget=10_000),
        "loaded_tool_names": ["search", "file_read"],
    }
    defaults.update(overrides)
    return RunRecord(**defaults)


class FakeLLMClient:
    """Fake LLM client returning canned responses."""

    def __init__(self, responses: list[str] | None = None) -> None:
        self._responses = list(responses or [])
        self._call_idx = 0
        self.last_response_data: dict[str, Any] | None = None
        self.calls: list[dict[str, Any]] = []

    async def planner_complete(
        self,
        messages: list[dict[str, str]],
        model: str | None = None,
        temperature: float | None = None,
        cache_params: CacheSafeParams | None = None,
    ) -> str:
        self.calls.append({"messages": messages, "cache_params": cache_params})
        if self._call_idx < len(self._responses):
            resp = self._responses[self._call_idx]
        else:
            resp = json.dumps({"kind": "final_response", "input": {"content": "done"}})
        self._call_idx += 1
        self.last_response_data = {"usage": {"input_tokens": 100, "output_tokens": 50}}
        return resp


class FakePublisher:
    async def action_completed(self, *args: Any) -> None:
        pass


async def _noop_execute(run: Any, action: Any) -> Any:
    action.status = ActionStatus.COMPLETED
    action.output = f"result-{action.name}"
    return action


# Patches to avoid real DB / compaction calls
_PATCHES = [
    patch("app.streaming_loop.load_memory_files", new_callable=AsyncMock, return_value=[]),
    patch("app.streaming_loop.auto_compact", new_callable=AsyncMock, side_effect=lambda msgs, **kw: (msgs, False)),
]


def _apply_patches():
    """Start all patches and return list of mocks."""
    return [p.start() for p in _PATCHES]


def _stop_patches():
    for p in _PATCHES:
        p.stop()


# ---------------------------------------------------------------------------
# StopReason enum tests
# ---------------------------------------------------------------------------


class TestStopReason:
    def test_all_values(self) -> None:
        assert len(StopReason) == 8
        expected = {
            "end_turn", "max_turns", "tool_use", "interrupt",
            "error", "timeout", "budget_exceeded", "final_response",
        }
        assert {s.value for s in StopReason} == expected

    def test_serialization(self) -> None:
        assert StopReason.FINAL_RESPONSE.value == "final_response"
        assert StopReason("max_turns") == StopReason.MAX_TURNS


# ---------------------------------------------------------------------------
# TurnEvent / TurnEventKind tests
# ---------------------------------------------------------------------------


class TestTurnEvent:
    def test_kinds(self) -> None:
        assert len(TurnEventKind) == 8

    def test_create(self) -> None:
        ev = TurnEvent(kind=TurnEventKind.TOOL_CALL_START, turn_index=2, data={"name": "bash"})
        assert ev.kind == TurnEventKind.TOOL_CALL_START
        assert ev.turn_index == 2
        assert ev.data["name"] == "bash"

    def test_default_empty_data(self) -> None:
        ev = TurnEvent(kind=TurnEventKind.PROGRESS)
        assert ev.data == {}
        assert ev.turn_index == 0


# ---------------------------------------------------------------------------
# CacheSafeParams tests
# ---------------------------------------------------------------------------


class TestCacheSafeParams:
    def test_defaults(self) -> None:
        p = CacheSafeParams()
        assert p.enabled is True
        assert p.cache_type == "ephemeral"
        assert p.ttl is None
        assert p.disabled_for_models == []

    def test_with_ttl(self) -> None:
        p = CacheSafeParams(ttl="1h", scope="global")
        assert p.ttl == "1h"
        assert p.scope == "global"

    def test_disable_for_model(self) -> None:
        p = CacheSafeParams(disabled_for_models=["claude-3-haiku-20240307"])
        assert "claude-3-haiku-20240307" in p.disabled_for_models


# ---------------------------------------------------------------------------
# CompactCircuitBreaker tests
# ---------------------------------------------------------------------------


class TestCompactCircuitBreaker:
    def test_initial_state(self) -> None:
        cb = CompactCircuitBreaker()
        assert cb.consecutive_failures == 0
        assert cb.tripped is False

    def test_success_resets(self) -> None:
        cb = CompactCircuitBreaker()
        cb.record_failure()
        cb.record_failure()
        cb.record_success()
        assert cb.consecutive_failures == 0
        assert cb.tripped is False

    def test_trips_at_threshold(self) -> None:
        cb = CompactCircuitBreaker()
        for _ in range(MAX_CONSECUTIVE_COMPACT_FAILURES):
            cb.record_failure()
        assert cb.tripped is True

    def test_custom_max(self) -> None:
        cb = CompactCircuitBreaker(max_failures=2)
        cb.record_failure()
        assert cb.tripped is False
        cb.record_failure()
        assert cb.tripped is True


# ---------------------------------------------------------------------------
# CacheBreakDetector tests
# ---------------------------------------------------------------------------


class TestCacheBreakDetector:
    def test_no_break_first_turn(self) -> None:
        d = CacheBreakDetector()
        usage = TurnUsage(cache_read_tokens=500)
        assert d.check(usage) is False

    def test_detects_significant_drop(self) -> None:
        d = CacheBreakDetector()
        d.check(TurnUsage(cache_read_tokens=1000))
        assert d.check(TurnUsage(cache_read_tokens=100)) is True
        assert d.breaks_detected == 1

    def test_no_break_small_decrease(self) -> None:
        d = CacheBreakDetector()
        d.check(TurnUsage(cache_read_tokens=1000))
        assert d.check(TurnUsage(cache_read_tokens=800)) is False

    def test_no_break_when_previous_low(self) -> None:
        d = CacheBreakDetector()
        d.check(TurnUsage(cache_read_tokens=50))
        assert d.check(TurnUsage(cache_read_tokens=10)) is False


# ---------------------------------------------------------------------------
# Streaming turn loop integration tests
# ---------------------------------------------------------------------------


class TestStreamingTurnLoop:
    def setup_method(self) -> None:
        _apply_patches()

    def teardown_method(self) -> None:
        _stop_patches()

    @pytest.mark.asyncio
    async def test_final_response_emits_loop_finished(self) -> None:
        llm = FakeLLMClient([
            json.dumps({"kind": "final_response", "input": {"content": "hello"}}),
        ])
        run = _make_run()
        events: list[TurnEvent] = []
        async for ev in run_streaming_turn_loop(
            run, llm, None, _noop_execute, FakePublisher()
        ):
            events.append(ev)

        kinds = [e.kind for e in events]
        assert TurnEventKind.LOOP_FINISHED in kinds
        finished = [e for e in events if e.kind == TurnEventKind.LOOP_FINISHED][0]
        assert finished.data["stop_reason"] == "final_response"
        assert finished.data["final_output"] == "hello"

    @pytest.mark.asyncio
    async def test_tool_calls_emit_events(self) -> None:
        llm = FakeLLMClient([
            json.dumps({"kind": "tool_call", "name": "search", "input": {"q": "test"}}),
            json.dumps({"kind": "final_response", "input": {"content": "done"}}),
        ])
        run = _make_run()
        events: list[TurnEvent] = []
        async for ev in run_streaming_turn_loop(
            run, llm, None, _noop_execute, FakePublisher()
        ):
            events.append(ev)

        kinds = [e.kind for e in events]
        assert TurnEventKind.TOOL_CALL_START in kinds
        assert TurnEventKind.TOOL_RESULT in kinds
        assert TurnEventKind.TURN_COMPLETE in kinds

    @pytest.mark.asyncio
    async def test_max_turns_stop_reason(self) -> None:
        responses = [
            json.dumps({"kind": "tool_call", "name": "search", "input": {"q": str(i)}})
            for i in range(10)
        ]
        llm = FakeLLMClient(responses)
        run = _make_run(policy=ExecutionPolicy(max_turns=3, token_budget=100_000))
        events: list[TurnEvent] = []
        async for ev in run_streaming_turn_loop(
            run, llm, None, _noop_execute, FakePublisher()
        ):
            events.append(ev)

        finished = [e for e in events if e.kind == TurnEventKind.LOOP_FINISHED]
        assert len(finished) >= 1
        assert finished[0].data["stop_reason"] == "max_turns"

    @pytest.mark.asyncio
    async def test_usage_delta_events(self) -> None:
        llm = FakeLLMClient([
            json.dumps({"kind": "final_response", "input": {"content": "ok"}}),
        ])
        run = _make_run()
        events: list[TurnEvent] = []
        async for ev in run_streaming_turn_loop(
            run, llm, None, _noop_execute, FakePublisher()
        ):
            events.append(ev)

        usage_evts = [e for e in events if e.kind == TurnEventKind.USAGE_DELTA]
        assert len(usage_evts) >= 1
        assert "cache_break" in usage_evts[0].data

    @pytest.mark.asyncio
    async def test_cache_params_passed_to_llm(self) -> None:
        llm = FakeLLMClient([
            json.dumps({"kind": "final_response", "input": {"content": "ok"}}),
        ])
        run = _make_run()
        cp = CacheSafeParams(ttl="1h")
        events: list[TurnEvent] = []
        async for ev in run_streaming_turn_loop(
            run, llm, None, _noop_execute, FakePublisher(), cache_params=cp
        ):
            events.append(ev)

        assert llm.calls[0]["cache_params"] is cp

    @pytest.mark.asyncio
    async def test_progress_event_at_end(self) -> None:
        llm = FakeLLMClient([
            json.dumps({"kind": "final_response", "input": {"content": "ok"}}),
        ])
        run = _make_run()
        events: list[TurnEvent] = []
        async for ev in run_streaming_turn_loop(
            run, llm, None, _noop_execute, FakePublisher()
        ):
            events.append(ev)

        progress = [e for e in events if e.kind == TurnEventKind.PROGRESS]
        assert len(progress) == 1
        assert "cost_summary" in progress[0].data
        assert "cache_breaks" in progress[0].data

    @pytest.mark.asyncio
    async def test_parse_error_stops_with_error_reason(self) -> None:
        llm = FakeLLMClient(["not valid json at all"])
        run = _make_run()
        events: list[TurnEvent] = []
        async for ev in run_streaming_turn_loop(
            run, llm, None, _noop_execute, FakePublisher()
        ):
            events.append(ev)

        finished = [e for e in events if e.kind == TurnEventKind.LOOP_FINISHED]
        assert finished[0].data["stop_reason"] == "error"

    @pytest.mark.asyncio
    async def test_failed_action_stops_loop(self) -> None:
        async def _fail_execute(run: Any, action: Any) -> Any:
            action.status = ActionStatus.FAILED
            action.error = "boom"
            return action

        llm = FakeLLMClient([
            json.dumps({"kind": "tool_call", "name": "bash", "input": {"cmd": "ls"}}),
        ])
        run = _make_run()
        events: list[TurnEvent] = []
        async for ev in run_streaming_turn_loop(
            run, llm, None, _fail_execute, FakePublisher()
        ):
            events.append(ev)

        finished = [e for e in events if e.kind == TurnEventKind.LOOP_FINISHED]
        assert finished[0].data["stop_reason"] == "error"


# ---------------------------------------------------------------------------
# streaming_result_from_events tests
# ---------------------------------------------------------------------------


class TestStreamingResultFromEvents:
    def test_builds_result(self) -> None:
        events = [
            TurnEvent(kind=TurnEventKind.LOOP_FINISHED, data={"stop_reason": "final_response", "final_output": "hi"}),
            TurnEvent(kind=TurnEventKind.PROGRESS, data={"cost_summary": {"total_tokens": 150}, "cache_stats": {}}),
        ]
        result = streaming_result_from_events(events)
        assert result.stopped_reason == "final_response"
        assert result.final_output == "hi"
        assert result.cost_summary["total_tokens"] == 150

    def test_empty_events(self) -> None:
        result = streaming_result_from_events([])
        assert result.stopped_reason == "max_turns"
        assert result.final_output is None


# ---------------------------------------------------------------------------
# LLM client cache_control injection test
# ---------------------------------------------------------------------------


class TestLLMClientCacheControl:
    @pytest.mark.asyncio
    async def test_cache_control_in_body(self) -> None:
        """Verify planner_complete accepts cache_params arg without error."""
        from app.llm_client import LLMClient

        client = LLMClient()
        # We can't call open() or actually POST — just verify signature works.
        import inspect
        sig = inspect.signature(client.planner_complete)
        assert "cache_params" in sig.parameters

    def test_cache_safe_params_model_blacklist(self) -> None:
        p = CacheSafeParams(disabled_for_models=["haiku"])
        assert "haiku" in p.disabled_for_models
