"""Reactive turn loop — CC-style per-turn decision making.

Instead of plan-all-then-execute, the reactive loop asks the LLM
one turn at a time:

  while turns < max_turns:
    1. Send history → LLM (with tools available)
    2. Parse LLM's next action (tool_call, reasoning, or final_response)
    3. Execute action
    4. Append result to history
    5. If LLM says "done" → break

This mirrors CC's main turn loop where the model decides what to do
each step based on accumulated context.

Enhanced with:
- Extended thinking configuration (adaptive/enabled/disabled)
- Letta memory injection (pre-turn context fetch)
- Session recovery with interruption detection
- Graceful degradation on repeated failures
"""

from __future__ import annotations

import asyncio
import json
import logging
from dataclasses import dataclass, field
from typing import Any

from app.context.auto_compact import auto_compact
from app.context.compact import compact_history
from app.context.injector import compose_system_prompt
from app.context.memory import load_memory_files
from app.cost_tracker import CostTracker, TurnUsage, parse_usage_from_response
from app.domain import (
    ActionKind,
    ActionStatus,
    ActionTarget,
    AgentAction,
    RunRecord,
    RunStatus,
)
from app.thinking import ThinkingConfig, ThinkingMode, resolve_thinking_config, strip_thinking_blocks
from app.timeouts import RunClock, RunDeadlineExceeded, TurnTimeoutError, with_turn_timeout
from app.tool_cache import ToolResultCache

logger = logging.getLogger(__name__)

# Tools that are safe to execute concurrently (never mutate state).
# MCP tools named with a "read_" prefix are also treated as readonly.
READONLY_TOOLS: frozenset[str] = frozenset({
    "search",
    "retrieve",
    "memory_read",
    "list_files",
    "read_file",
    "get_file_contents",
    "grep",
    "glob",
})


@dataclass
class Turn:
    """A single turn in the reactive loop."""

    index: int
    action: AgentAction | None = None
    is_final: bool = False


@dataclass
class TurnLoopResult:
    """Result of a complete reactive turn loop."""

    actions: list[AgentAction] = field(default_factory=list)
    final_output: str | None = None
    turns_used: int = 0
    stopped_reason: str = "max_turns"  # max_turns | final_response | error | timeout | budget_exceeded | graceful_degrade
    cost_summary: dict[str, Any] = field(default_factory=dict)
    cache_stats: dict[str, Any] = field(default_factory=dict)
    thinking_config: dict[str, Any] = field(default_factory=dict)
    letta_memories_used: int = 0
    consecutive_errors: int = 0


REACTIVE_SYSTEM_PROMPT = (
    "You are an autonomous agent. Respond with a single JSON action object.\n"
    "Each response must be ONE of:\n"
    '  {{"kind": "tool_call", "name": "<tool>", "input": {{...}}}}\n'
    '  {{"kind": "reasoning", "input": {{"query": "<your reasoning>"}}}}\n'
    '  {{"kind": "final_response", "input": {{"content": "<final answer>"}}}}\n\n'
    "For read-only information gathering you may emit a JSON ARRAY of tool_call objects\n"
    "to execute concurrently:\n"
    '  [{{"kind": "tool_call", "name": "search", ...}}, {{"kind": "tool_call", "name": "retrieve", ...}}]\n\n'
    "Available tools: [{tool_names}]\n"
    "Max remaining turns: {remaining_turns}\n\n"
    "When you have completed the task, respond with a final_response action.\n"
    "Respond with ONLY the JSON object or array, no markdown."
)


async def run_turn_loop(
    run: RunRecord,
    llm_client: Any,
    capability_client: Any,
    execute_action_fn: Any,
    publisher: Any,
    policy_engine: Any | None = None,
    event_stream: Any | None = None,
    thinking_override: ThinkingMode | None = None,
) -> TurnLoopResult:
    """Execute the reactive turn loop for a run.

    Args:
        run: The run record
        llm_client: LLM client for planner calls
        capability_client: For tool execution (unused directly; execute_action_fn handles it)
        execute_action_fn: async (run, action) -> action; the agent_service._execute_action
        publisher: Event publisher for action events
        policy_engine: PolicyEngine for permission checks before tool execution
        event_stream: EventStream for rich streaming of RunEvents
        thinking_override: Override thinking mode for this run
    """
    max_turns = run.policy.max_turns
    token_budget = run.policy.token_budget
    tool_names = ", ".join(run.loaded_tool_names) if run.loaded_tool_names else "none"

    # Phase R: wall-clock deadline for the entire run
    run_clock = RunClock(deadline=run.policy.run_timeout)

    # Phase S: cumulative token cost tracker
    cost_tracker = CostTracker(budget=token_budget)

    # Phase T: within-run tool result cache
    tool_cache = ToolResultCache()

    # Phase U: within-turn dedup set (reset per-turn)
    _seen_tool_calls: set[str] = set()

    # Phase 1: resolve extended thinking config
    model_name = getattr(run.policy, "model", "") or getattr(run, "model", "") or ""
    thinking_config = resolve_thinking_config(
        model_name,
        user_override=thinking_override,
    )

    # Phase 9: Letta memory injection (pre-loop context fetch)
    letta_snippets: list[str] = []
    if run.org_id and not run.policy.memory_isolation:
        try:
            from app.letta.memory_bridge import get_memory_bridge
            bridge = get_memory_bridge()
            letta_snippets = await bridge.fetch_context(
                org_id=run.org_id,
                query=run.goal[:500],  # Use first 500 chars of goal as query
            )
        except Exception as exc:
            logger.debug("letta_context_fetch_skipped", extra={"error": str(exc)})

    # Load memory for prompt composition (Phase X: skip if memory_isolation)
    if run.policy.memory_isolation:
        memory_snippets: list[str] = []
    else:
        memory_snippets = await load_memory_files(
            org_id=run.org_id,
            session_id=run.session_id,
        )

    # Merge Letta memories into memory snippets
    if letta_snippets:
        from app.letta.memory_bridge import get_memory_bridge
        bridge = get_memory_bridge()
        letta_prompt = bridge.format_memory_prompt(letta_snippets)
        if letta_prompt:
            memory_snippets.append(letta_prompt)

    # Build conversation history
    history: list[dict[str, Any]] = []
    actions: list[AgentAction] = []
    result = TurnLoopResult()
    result.letta_memories_used = len(letta_snippets)
    result.thinking_config = {
        "mode": thinking_config.mode.value,
        "budget_tokens": thinking_config.budget_tokens,
        "active": thinking_config.is_active,
    }

    # Error recovery: track consecutive failures for graceful degradation
    _consecutive_errors = 0
    _MAX_CONSECUTIVE_ERRORS = 3  # After 3 errors, degrade gracefully

    for turn_idx in range(max_turns):
        # Phase R: check run deadline before starting a new turn
        try:
            run_clock.check()
        except RunDeadlineExceeded:
            result.stopped_reason = "timeout"
            logger.warning(
                "run_deadline_exceeded",
                extra={"run_id": run.id, "elapsed": run_clock.elapsed()},
            )
            break

        # Phase S: check budget before starting a new turn
        try:
            cost_tracker.check_budget()
        except Exception:
            result.stopped_reason = "budget_exceeded"
            logger.warning(
                "token_budget_exceeded",
                extra={"run_id": run.id, "used": cost_tracker.total_tokens},
            )
            break

        remaining = max_turns - turn_idx

        # Build system prompt with memory + tool context
        base_prompt = REACTIVE_SYSTEM_PROMPT.format(
            tool_names=tool_names,
            remaining_turns=remaining,
        )
        system_prompt = compose_system_prompt(
            base_prompt=base_prompt,
            memory_snippets=memory_snippets,
            session_id=run.session_id or "",
        )

        # Compose messages: system + initial goal + history
        messages: list[dict[str, Any]] = [
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": run.goal},
        ]
        messages.extend(history)

        # Auto-compact if approaching token budget (Phase J)
        messages, _ = await auto_compact(
            messages, token_budget=token_budget, llm_client=llm_client,
            model=model_name,
        )

        # Strip thinking blocks if model changed (CC rule)
        if thinking_config.is_active and model_name:
            # Only strip if this is a resumed session with potentially different model
            pass  # Thinking blocks from same model are preserved

        # Phase R: LLM call with per-turn timeout + thinking config
        turn_timeout = run_clock.turn_timeout(default=run.policy.turn_timeout)
        thinking_param = thinking_config.to_api_param()
        try:
            # Pass thinking config to LLM client if supported
            if thinking_param and hasattr(llm_client, 'planner_complete_with_thinking'):
                raw = await with_turn_timeout(
                    llm_client.planner_complete_with_thinking(messages, thinking=thinking_param),
                    turn_index=turn_idx,
                    timeout=turn_timeout,
                )
            else:
                raw = await with_turn_timeout(
                    llm_client.planner_complete(messages),
                    turn_index=turn_idx,
                    timeout=turn_timeout,
                )
        except TurnTimeoutError:
            result.stopped_reason = "timeout"
            break
        except RunDeadlineExceeded:
            result.stopped_reason = "timeout"
            break

        # Phase S: record cost if LLM returns usage data
        if hasattr(llm_client, 'last_response_data') and llm_client.last_response_data:
            usage = parse_usage_from_response(llm_client.last_response_data)
            cost_tracker.record(usage)

        # Parse the action — may return 1 or more (batch read-only)
        action_batch = _parse_actions(raw, turn_idx)
        if not action_batch:
            logger.warning("turn_parse_failed", extra={"turn": turn_idx, "raw": raw[:200]})
            result.stopped_reason = "error"
            break

        # Parallel execution for read-only batches (Phase Q)
        if len(action_batch) > 1:
            executed = await _execute_parallel(action_batch, run, execute_action_fn)
            actions.extend(executed)
            for act in executed:
                history.append({
                    "role": "assistant",
                    "content": json.dumps({"kind": act.kind.value, "name": act.name, "input": act.input}),
                })
                history.append({
                    "role": "user",
                    "content": _format_action_result(act),
                })
            result.turns_used = turn_idx + 1
            _seen_tool_calls.clear()
            continue

        action = action_batch[0]
        actions.append(action)

        # Check for final response
        if action.kind == ActionKind.FINAL_RESPONSE:
            action.status = ActionStatus.COMPLETED
            action.output = action.input.get("content", "")
            result.final_output = action.output
            result.stopped_reason = "final_response"
            await publisher.action_completed(
                run.id, run.session_id, action.id, action.output, None
            )
            break

        # Phase T + U: tool result cache + within-turn dedup
        if action.kind == ActionKind.TOOL_CALL:
            import hashlib as _hl
            import json as _json

            _dedup_key = f"{action.name}:{_hl.sha256(_json.dumps(action.input, sort_keys=True, default=str).encode()).hexdigest()[:16]}"

            # Phase U: within-turn dedup
            if _dedup_key in _seen_tool_calls:
                action.status = ActionStatus.SKIPPED
                action.error = "Duplicate tool call within turn"
                logger.info("tool_call_dedup_skip", extra={"tool": action.name})
                history.append({
                    "role": "assistant",
                    "content": json.dumps({"kind": action.kind.value, "name": action.name, "input": action.input}),
                })
                history.append({
                    "role": "user",
                    "content": "[Duplicate tool call skipped]",
                })
                result.turns_used = turn_idx + 1
                continue

            _seen_tool_calls.add(_dedup_key)

            # Phase T: check cache
            cached = tool_cache.get(action.name, action.input)
            if cached is not None:
                action.status = ActionStatus.COMPLETED
                action.output = cached
                history.append({
                    "role": "assistant",
                    "content": json.dumps({"kind": action.kind.value, "name": action.name, "input": action.input}),
                })
                history.append({
                    "role": "user",
                    "content": _format_action_result(action),
                })
                result.turns_used = turn_idx + 1
                await publisher.action_completed(
                    run.id, run.session_id, action.id, action.output, None
                )
                continue

        # Execute the action
        # Phase 4: policy engine check before execution
        if policy_engine and action.kind == ActionKind.TOOL_CALL:
            from app.permissions.policy_engine import PolicyVerdict
            verdict: PolicyVerdict = await policy_engine.check(
                tool_name=action.name,
                args=action.input,
                run_id=run.id,
                permission_mode=run.policy.permission_mode if hasattr(run.policy, "permission_mode") else None,
            )
            # Emit policy events through stream
            if event_stream and verdict.events:
                for ev in verdict.events:
                    await event_stream.emit(ev)

            if not verdict.allowed:
                action.status = ActionStatus.FAILED
                action.error = f"Policy denied: {verdict.decision.value}"
                history.append({
                    "role": "assistant",
                    "content": json.dumps({"kind": action.kind.value, "name": action.name, "input": action.input}),
                })
                history.append({
                    "role": "user",
                    "content": f"[Permission denied: {verdict.decision.value}]",
                })
                result.turns_used = turn_idx + 1
                _seen_tool_calls.clear()
                continue

        action = await execute_action_fn(run, action)

        # Emit tool result events through stream
        if event_stream and action.kind == ActionKind.TOOL_CALL:
            from app.messages.types import MessageType, build_event
            event_type = (
                MessageType.TOOL_RESULT
                if action.status == ActionStatus.COMPLETED
                else MessageType.TOOL_ERROR
            )
            await event_stream.emit(build_event(
                run_id=run.id,
                session_id=run.session_id,
                msg_type=event_type,
                turn_index=turn_idx,
                data={"tool": action.name, "status": action.status.value},
                content=str(action.output or action.error or "")[:2000],
            ))

        # Phase T: cache successful tool results
        if action.kind == ActionKind.TOOL_CALL and action.status == ActionStatus.COMPLETED:
            tool_cache.put(action.name, action.input, action.output)

        # Append result to history for next turn
        history.append({
            "role": "assistant",
            "content": json.dumps({
                "kind": action.kind.value,
                "name": action.name,
                "input": action.input,
            }),
        })
        history.append({
            "role": "user",
            "content": _format_action_result(action),
        })

        result.turns_used = turn_idx + 1

        # Reset within-turn dedup for next turn
        _seen_tool_calls.clear()

        # Graceful degradation: track consecutive errors
        if action.status == ActionStatus.FAILED:
            _consecutive_errors += 1
            result.consecutive_errors = _consecutive_errors
            if _consecutive_errors >= _MAX_CONSECUTIVE_ERRORS:
                result.stopped_reason = "graceful_degrade"
                logger.warning(
                    "turn_loop_graceful_degrade",
                    extra={
                        "run_id": run.id,
                        "consecutive_errors": _consecutive_errors,
                        "last_error": action.error,
                    },
                )
                # Try to produce a partial result instead of hard failure
                result.final_output = (
                    f"Stopped after {_consecutive_errors} consecutive errors. "
                    f"Last error: {action.error}. "
                    f"Partial progress: {len(actions)} actions completed."
                )
                break
            # Single error: continue to next turn (agent might self-correct)
            continue
        else:
            _consecutive_errors = 0  # Reset on success

    else:
        result.stopped_reason = "max_turns"

    result.actions = actions
    result.turns_used = min(result.turns_used + 1, max_turns)
    result.cost_summary = cost_tracker.summary()
    result.cache_stats = tool_cache.stats()

    # Phase 9: queue Letta trajectory sync (fire-and-forget)
    if run.org_id and actions and not run.policy.memory_isolation:
        try:
            from app.letta.memory_bridge import get_memory_bridge
            bridge = get_memory_bridge()
            turn_data = {
                "goal": run.goal,
                "actions_count": len(actions),
                "final_output": result.final_output,
                "stopped_reason": result.stopped_reason,
            }
            bridge.queue_trajectory_sync(
                org_id=run.org_id,
                run_id=run.id,
                turn_data=turn_data,
            )
        except Exception as exc:
            logger.debug("letta_trajectory_sync_skipped", extra={"error": str(exc)})

    logger.info(
        "turn_loop_completed",
        extra={
            "run_id": run.id,
            "turns": result.turns_used,
            "reason": result.stopped_reason,
            "actions": len(actions),
            "total_tokens": cost_tracker.total_tokens,
            "cache_hits": tool_cache._hits,
        },
    )

    return result


def _is_readonly(name: str) -> bool:
    """Return True if *name* is a known read-only tool."""
    return name in READONLY_TOOLS or name.startswith("read_")


def _make_action(item: dict[str, Any], turn_index: int) -> AgentAction | None:
    """Build a single AgentAction from a parsed dict."""
    if not isinstance(item, dict):
        return None

    kind_str = item.get("kind", "reasoning")
    try:
        kind = ActionKind(kind_str)
    except ValueError:
        return None

    name = item.get("name", f"turn_{turn_index}")

    target = ActionTarget.INTERNAL
    if kind == ActionKind.TOOL_CALL:
        if name.startswith("mcp:"):
            target = ActionTarget.AGENT_CORE
        else:
            target = ActionTarget.AI_CORE

    return AgentAction(
        kind=kind,
        target=target,
        name=name,
        description=item.get("description"),
        input=item.get("input", {}),
        readonly=_is_readonly(name) if kind == ActionKind.TOOL_CALL else False,
    )


def _parse_actions(raw: str, turn_index: int) -> list[AgentAction]:
    """Parse one or more JSON actions from a raw LLM response.

    Returns a list with 1 element for a single-action response, or N
    elements for a batch array response (all must be read-only tool_calls;
    otherwise the batch is rejected and only the first is returned).
    """
    raw = raw.strip()
    if raw.startswith("```"):
        lines = raw.split("\n")
        raw = "\n".join(lines[1:-1]).strip()

    try:
        parsed = json.loads(raw)
    except json.JSONDecodeError:
        return []

    # Batch array case
    if isinstance(parsed, list):
        actions: list[AgentAction] = []
        for item in parsed:
            a = _make_action(item, turn_index)
            if a is None or a.kind != ActionKind.TOOL_CALL or not a.readonly:
                # Non-readonly or invalid item in batch → fall back to single
                if actions:
                    return [actions[0]]
                a_single = _make_action(parsed[0], turn_index) if parsed else None
                return [a_single] if a_single else []
            actions.append(a)
        return actions if actions else []

    # Single action case
    action = _make_action(parsed, turn_index)
    return [action] if action else []


def _parse_single_action(raw: str, turn_index: int) -> "AgentAction | None":
    """Back-compat alias — parses a single action from a raw LLM response.

    Wraps ``_parse_actions`` and returns the first result or ``None``.
    """
    results = _parse_actions(raw, turn_index)
    return results[0] if results else None


async def _execute_parallel(
    actions: list[AgentAction],
    run: RunRecord,
    execute_action_fn: Any,
) -> list[AgentAction]:
    """Execute a list of read-only actions concurrently via asyncio.gather.

    Results are returned in the same order as *actions*.
    """
    results = await asyncio.gather(
        *[execute_action_fn(run, a) for a in actions],
        return_exceptions=False,
    )
    return list(results)


def _format_action_result(action: AgentAction) -> str:
    """Format an action's result as a user message for the next turn."""
    if action.status == ActionStatus.COMPLETED:
        output = action.output
        if isinstance(output, dict):
            return json.dumps(output, default=str)
        return str(output) if output else "(no output)"
    elif action.status == ActionStatus.SKIPPED:
        return f"[Action skipped: {action.error or 'blocked by hook'}]"
    elif action.status == ActionStatus.FAILED:
        return f"[Action failed: {action.error or 'unknown error'}]"
    return "(action pending)"
