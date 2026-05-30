"""Streaming turn loop — yields TurnEvent objects as an async generator.

Phase A1: wraps the existing run_turn_loop with a streaming interface,
adds circuit-breaker for compaction failures, prompt-cache-break
detection, and max_turns enforcement with StopReason tracking.

Mirrors CC's QueryEngine streaming pattern where consumers receive
events as they happen rather than waiting for the full result.
"""

from __future__ import annotations

import asyncio
import json
import logging
from dataclasses import dataclass, field
from typing import Any, AsyncIterator

from app.context.auto_compact import auto_compact
from app.context.injector import compose_system_prompt
from app.context.memory import load_memory_files
from app.cost_tracker import CostTracker, TurnUsage, parse_usage_from_response
from app.domain import (
    ActionKind,
    ActionStatus,
    CacheSafeParams,
    StopReason,
    TurnEvent,
    TurnEventKind,
)
from app.timeouts import RunClock, RunDeadlineExceeded, TurnTimeoutError, with_turn_timeout
from app.tool_cache import ToolResultCache
from app.turn_loop import (
    READONLY_TOOLS,
    REACTIVE_SYSTEM_PROMPT,
    TurnLoopResult,
    _execute_parallel,
    _format_action_result,
    _is_readonly,
    _make_action,
    _parse_actions,
)

logger = logging.getLogger(__name__)

# Circuit-breaker: max consecutive compaction failures before aborting
MAX_CONSECUTIVE_COMPACT_FAILURES = 3


@dataclass
class CompactCircuitBreaker:
    """Track consecutive compaction failures — trip after max threshold."""

    consecutive_failures: int = 0
    max_failures: int = MAX_CONSECUTIVE_COMPACT_FAILURES
    tripped: bool = False

    def record_success(self) -> None:
        self.consecutive_failures = 0

    def record_failure(self) -> None:
        self.consecutive_failures += 1
        if self.consecutive_failures >= self.max_failures:
            self.tripped = True
            logger.error(
                "compact_circuit_breaker_tripped",
                extra={"consecutive": self.consecutive_failures},
            )


@dataclass
class CacheBreakDetector:
    """Detect prompt-cache breaks by tracking cache_read_tokens across turns."""

    last_cache_read: int = 0
    breaks_detected: int = 0

    def check(self, usage: TurnUsage) -> bool:
        """Return True if a cache break was detected (significant drop)."""
        current = usage.cache_read_tokens
        had_cache = self.last_cache_read > 100
        lost_cache = current < self.last_cache_read * 0.5
        self.last_cache_read = current
        if had_cache and lost_cache:
            self.breaks_detected += 1
            logger.info(
                "prompt_cache_break_detected",
                extra={
                    "previous": self.last_cache_read,
                    "current": current,
                    "total_breaks": self.breaks_detected,
                },
            )
            return True
        return False


@dataclass
class StreamingTurnLoopState:
    """Mutable state for a streaming turn loop run."""

    history: list[dict[str, Any]] = field(default_factory=list)
    actions: list[Any] = field(default_factory=list)
    turns_used: int = 0
    stop_reason: StopReason = StopReason.MAX_TURNS
    cost_tracker: CostTracker = field(default_factory=lambda: CostTracker())
    tool_cache: ToolResultCache = field(default_factory=ToolResultCache)
    compact_breaker: CompactCircuitBreaker = field(default_factory=CompactCircuitBreaker)
    cache_break_detector: CacheBreakDetector = field(default_factory=CacheBreakDetector)
    seen_tool_calls: set[str] = field(default_factory=set)


async def run_streaming_turn_loop(
    run: Any,
    llm_client: Any,
    capability_client: Any,
    execute_action_fn: Any,
    publisher: Any,
    cache_params: CacheSafeParams | None = None,
) -> AsyncIterator[TurnEvent]:
    """Async generator that yields TurnEvent objects during execution.

    This is the streaming equivalent of run_turn_loop — same logic,
    but emits events as they happen for real-time consumption.
    """
    max_turns = run.policy.max_turns
    token_budget = run.policy.token_budget
    tool_names = ", ".join(run.loaded_tool_names) if run.loaded_tool_names else "none"

    run_clock = RunClock(deadline=run.policy.run_timeout)
    state = StreamingTurnLoopState(
        cost_tracker=CostTracker(budget=token_budget),
    )

    # Load memory
    if run.policy.memory_isolation:
        memory_snippets: list[str] = []
    else:
        memory_snippets = await load_memory_files(
            org_id=run.org_id,
            session_id=run.session_id,
        )

    for turn_idx in range(max_turns):
        # Check run deadline
        try:
            run_clock.check()
        except RunDeadlineExceeded:
            state.stop_reason = StopReason.TIMEOUT
            yield TurnEvent(
                kind=TurnEventKind.LOOP_FINISHED,
                turn_index=turn_idx,
                data={"stop_reason": StopReason.TIMEOUT.value},
            )
            break

        # Check budget
        try:
            state.cost_tracker.check_budget()
        except Exception:
            state.stop_reason = StopReason.BUDGET_EXCEEDED
            yield TurnEvent(
                kind=TurnEventKind.LOOP_FINISHED,
                turn_index=turn_idx,
                data={"stop_reason": StopReason.BUDGET_EXCEEDED.value},
            )
            break

        remaining = max_turns - turn_idx

        # Build system prompt
        base_prompt = REACTIVE_SYSTEM_PROMPT.format(
            tool_names=tool_names,
            remaining_turns=remaining,
        )
        system_prompt = compose_system_prompt(
            base_prompt=base_prompt,
            memory_snippets=memory_snippets,
            session_id=run.session_id or "",
        )

        messages: list[dict[str, Any]] = [
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": run.goal},
        ]
        messages.extend(state.history)

        # Auto-compact with circuit breaker
        if not state.compact_breaker.tripped:
            try:
                messages, compacted = await auto_compact(
                    messages, token_budget=token_budget, llm_client=llm_client
                )
                if compacted:
                    state.compact_breaker.record_success()
                    yield TurnEvent(
                        kind=TurnEventKind.COMPACT_BOUNDARY,
                        turn_index=turn_idx,
                        data={"compacted": True},
                    )
            except Exception as exc:
                state.compact_breaker.record_failure()
                logger.warning(
                    "compact_failed",
                    extra={
                        "turn": turn_idx,
                        "consecutive": state.compact_breaker.consecutive_failures,
                        "error": str(exc),
                    },
                )
                if state.compact_breaker.tripped:
                    state.stop_reason = StopReason.ERROR
                    yield TurnEvent(
                        kind=TurnEventKind.LOOP_FINISHED,
                        turn_index=turn_idx,
                        data={
                            "stop_reason": StopReason.ERROR.value,
                            "error": "compact_circuit_breaker_tripped",
                        },
                    )
                    break

        # LLM call with timeout
        turn_timeout = run_clock.turn_timeout(default=run.policy.turn_timeout)
        try:
            raw = await with_turn_timeout(
                llm_client.planner_complete(messages, cache_params=cache_params),
                turn_index=turn_idx,
                timeout=turn_timeout,
            )
        except (TurnTimeoutError, RunDeadlineExceeded):
            state.stop_reason = StopReason.TIMEOUT
            yield TurnEvent(
                kind=TurnEventKind.LOOP_FINISHED,
                turn_index=turn_idx,
                data={"stop_reason": StopReason.TIMEOUT.value},
            )
            break

        # Record usage + detect cache breaks
        if hasattr(llm_client, "last_response_data") and llm_client.last_response_data:
            usage = parse_usage_from_response(llm_client.last_response_data)
            state.cost_tracker.record(usage)
            cache_break = state.cache_break_detector.check(usage)
            yield TurnEvent(
                kind=TurnEventKind.USAGE_DELTA,
                turn_index=turn_idx,
                data={
                    **usage.to_dict(),
                    "cache_break": cache_break,
                },
            )

        # Parse actions
        action_batch = _parse_actions(raw, turn_idx)
        if not action_batch:
            state.stop_reason = StopReason.ERROR
            yield TurnEvent(
                kind=TurnEventKind.LOOP_FINISHED,
                turn_index=turn_idx,
                data={"stop_reason": StopReason.ERROR.value, "error": "parse_failed"},
            )
            break

        # Parallel read-only batch
        if len(action_batch) > 1:
            for act in action_batch:
                yield TurnEvent(
                    kind=TurnEventKind.TOOL_CALL_START,
                    turn_index=turn_idx,
                    data={"name": act.name, "input": act.input},
                )
            executed = await _execute_parallel(action_batch, run, execute_action_fn)
            state.actions.extend(executed)
            for act in executed:
                state.history.append({
                    "role": "assistant",
                    "content": json.dumps({"kind": act.kind.value, "name": act.name, "input": act.input}),
                })
                state.history.append({
                    "role": "user",
                    "content": _format_action_result(act),
                })
                yield TurnEvent(
                    kind=TurnEventKind.TOOL_RESULT,
                    turn_index=turn_idx,
                    data={"name": act.name, "status": act.status.value},
                )
            state.turns_used = turn_idx + 1
            yield TurnEvent(
                kind=TurnEventKind.TURN_COMPLETE,
                turn_index=turn_idx,
                data={"turns_used": state.turns_used},
            )
            state.seen_tool_calls.clear()
            continue

        action = action_batch[0]
        state.actions.append(action)

        # Final response
        if action.kind == ActionKind.FINAL_RESPONSE:
            action.status = ActionStatus.COMPLETED
            action.output = action.input.get("content", "")
            state.stop_reason = StopReason.FINAL_RESPONSE
            await publisher.action_completed(
                run.id, run.session_id, action.id, action.output, None
            )
            yield TurnEvent(
                kind=TurnEventKind.LOOP_FINISHED,
                turn_index=turn_idx,
                data={
                    "stop_reason": StopReason.FINAL_RESPONSE.value,
                    "final_output": action.output,
                },
            )
            break

        # Tool call with dedup + caching
        if action.kind == ActionKind.TOOL_CALL:
            import hashlib as _hl

            _dedup_key = f"{action.name}:{_hl.sha256(json.dumps(action.input, sort_keys=True, default=str).encode()).hexdigest()[:16]}"

            if _dedup_key in state.seen_tool_calls:
                action.status = ActionStatus.SKIPPED
                action.error = "Duplicate tool call within turn"
                state.history.append({
                    "role": "assistant",
                    "content": json.dumps({"kind": action.kind.value, "name": action.name, "input": action.input}),
                })
                state.history.append({"role": "user", "content": "[Duplicate tool call skipped]"})
                state.turns_used = turn_idx + 1
                yield TurnEvent(
                    kind=TurnEventKind.TURN_COMPLETE,
                    turn_index=turn_idx,
                    data={"skipped": True},
                )
                continue

            state.seen_tool_calls.add(_dedup_key)

            cached = state.tool_cache.get(action.name, action.input)
            if cached is not None:
                action.status = ActionStatus.COMPLETED
                action.output = cached
                state.history.append({
                    "role": "assistant",
                    "content": json.dumps({"kind": action.kind.value, "name": action.name, "input": action.input}),
                })
                state.history.append({
                    "role": "user",
                    "content": _format_action_result(action),
                })
                state.turns_used = turn_idx + 1
                await publisher.action_completed(
                    run.id, run.session_id, action.id, action.output, None
                )
                yield TurnEvent(
                    kind=TurnEventKind.TOOL_RESULT,
                    turn_index=turn_idx,
                    data={"name": action.name, "status": "completed", "cached": True},
                )
                yield TurnEvent(
                    kind=TurnEventKind.TURN_COMPLETE,
                    turn_index=turn_idx,
                    data={"turns_used": state.turns_used},
                )
                continue

            yield TurnEvent(
                kind=TurnEventKind.TOOL_CALL_START,
                turn_index=turn_idx,
                data={"name": action.name, "input": action.input},
            )

        # Execute the action
        action = await execute_action_fn(run, action)

        # Cache successful tool results
        if action.kind == ActionKind.TOOL_CALL and action.status == ActionStatus.COMPLETED:
            state.tool_cache.put(action.name, action.input, action.output)

        state.history.append({
            "role": "assistant",
            "content": json.dumps({
                "kind": action.kind.value,
                "name": action.name,
                "input": action.input,
            }),
        })
        state.history.append({
            "role": "user",
            "content": _format_action_result(action),
        })

        state.turns_used = turn_idx + 1
        state.seen_tool_calls.clear()

        yield TurnEvent(
            kind=TurnEventKind.TOOL_RESULT,
            turn_index=turn_idx,
            data={"name": action.name, "status": action.status.value},
        )
        yield TurnEvent(
            kind=TurnEventKind.TURN_COMPLETE,
            turn_index=turn_idx,
            data={"turns_used": state.turns_used},
        )

        if action.status == ActionStatus.FAILED:
            state.stop_reason = StopReason.ERROR
            yield TurnEvent(
                kind=TurnEventKind.LOOP_FINISHED,
                turn_index=turn_idx,
                data={"stop_reason": StopReason.ERROR.value},
            )
            break
    else:
        state.stop_reason = StopReason.MAX_TURNS
        yield TurnEvent(
            kind=TurnEventKind.LOOP_FINISHED,
            turn_index=max_turns - 1,
            data={"stop_reason": StopReason.MAX_TURNS.value},
        )

    yield TurnEvent(
        kind=TurnEventKind.PROGRESS,
        turn_index=state.turns_used,
        data={
            "total_actions": len(state.actions),
            "cost_summary": state.cost_tracker.summary(),
            "cache_stats": state.tool_cache.stats(),
            "compact_failures": state.compact_breaker.consecutive_failures,
            "cache_breaks": state.cache_break_detector.breaks_detected,
        },
    )


def streaming_result_from_events(events: list[TurnEvent]) -> TurnLoopResult:
    """Build a TurnLoopResult from collected streaming events (for compat)."""
    result = TurnLoopResult()
    for ev in events:
        if ev.kind == TurnEventKind.LOOP_FINISHED:
            result.stopped_reason = ev.data.get("stop_reason", "max_turns")
            result.final_output = ev.data.get("final_output")
        elif ev.kind == TurnEventKind.PROGRESS:
            result.cost_summary = ev.data.get("cost_summary", {})
            result.cache_stats = ev.data.get("cache_stats", {})
    return result
