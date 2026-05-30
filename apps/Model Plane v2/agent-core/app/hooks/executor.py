"""Hook executor — run pre/post/stop hooks sequentially.

Pre-tool-use hooks:
  - Run in priority order (highest first)
  - Short-circuit on BLOCK → raise HookBlockedError
  - MODIFY → merge modified_input into action input
  - APPROVE → pass through

Post-tool-use hooks:
  - Run in priority order
  - MODIFY → replace output with modify_output template
  - BLOCK → not supported post-execution (logged as warning)

Stop hooks:
  - Run at end of run lifecycle
  - Can signal should_continue=False for future loop control
"""

from __future__ import annotations

import logging
from typing import Any

from app.hooks.domain import (
    HookAction,
    HookBlockedError,
    HookConfig,
    HookType,
    LifecycleHookResult,
    PermissionCheckResult,
    PostCompactResult,
    PostToolUseResult,
    PreCompactResult,
    PreToolUseResult,
    StopResult,
)
from app.hooks.registry import load_hooks_for_org, match_hooks

logger = logging.getLogger(__name__)


async def run_pre_hooks(
    org_id: str | None,
    tool_name: str,
    tool_input: dict[str, Any],
) -> PreToolUseResult:
    """Execute pre-tool-use hooks. Returns approval result with optional input modification."""
    if not org_id:
        return PreToolUseResult(approved=True)

    all_hooks = await load_hooks_for_org(org_id)
    matched = match_hooks(all_hooks, tool_name, HookType.PRE_TOOL_USE)

    if not matched:
        return PreToolUseResult(approved=True)

    result = PreToolUseResult(approved=True)

    for hook in matched:
        if hook.action == HookAction.BLOCK:
            logger.info(
                "hook_blocked_tool",
                extra={
                    "hook_id": hook.id,
                    "tool": tool_name,
                    "reason": hook.reason,
                },
            )
            raise HookBlockedError(
                hook_id=hook.id,
                tool_name=tool_name,
                reason=hook.reason,
            )

        if hook.action == HookAction.MODIFY and hook.modify_input:
            merged = {**tool_input, **hook.modify_input}
            result = result.model_copy(update={"modified_input": merged})
            logger.debug(
                "hook_modified_input",
                extra={"hook_id": hook.id, "tool": tool_name},
            )

        # HookAction.APPROVE → pass through (no-op)

    return result


async def run_post_hooks(
    org_id: str | None,
    tool_name: str,
    tool_output: Any,
) -> PostToolUseResult:
    """Execute post-tool-use hooks. Returns optional output modification."""
    if not org_id:
        return PostToolUseResult()

    all_hooks = await load_hooks_for_org(org_id)
    matched = match_hooks(all_hooks, tool_name, HookType.POST_TOOL_USE)

    if not matched:
        return PostToolUseResult()

    result = PostToolUseResult()

    for hook in matched:
        if hook.action == HookAction.BLOCK:
            logger.warning(
                "hook_block_post_execution_ignored",
                extra={"hook_id": hook.id, "tool": tool_name},
            )
            continue

        if hook.action == HookAction.MODIFY and hook.modify_output is not None:
            result = result.model_copy(
                update={
                    "modified_output": hook.modify_output,
                    "modified_by": hook.id,
                }
            )
            logger.debug(
                "hook_modified_output",
                extra={"hook_id": hook.id, "tool": tool_name},
            )

    return result


async def run_stop_hooks(org_id: str | None) -> StopResult:
    """Execute stop hooks at run termination."""
    if not org_id:
        return StopResult(should_continue=True)

    all_hooks = await load_hooks_for_org(org_id)
    matched = match_hooks(all_hooks, "*", HookType.STOP)

    if not matched:
        return StopResult(should_continue=True)

    for hook in matched:
        if hook.action == HookAction.BLOCK:
            return StopResult(should_continue=False, reason=hook.reason)

    return StopResult(should_continue=True)


async def run_pre_compact_hooks(
    org_id: str | None,
    messages: list[dict[str, Any]],
) -> PreCompactResult:
    """Execute pre-compact hooks before compacting conversation history.

    Hooks may:
      - BLOCK → skip compaction entirely
      - MODIFY → alter messages before summarization (e.g. strip sensitive content)
    """
    if not org_id:
        return PreCompactResult(proceed=True)

    all_hooks = await load_hooks_for_org(org_id)
    matched = match_hooks(all_hooks, "*", HookType.PRE_COMPACT)

    if not matched:
        return PreCompactResult(proceed=True)

    result = PreCompactResult(proceed=True)

    for hook in matched:
        if hook.action == HookAction.BLOCK:
            logger.info(
                "hook_blocked_compact",
                extra={"hook_id": hook.id, "reason": hook.reason},
            )
            return PreCompactResult(proceed=False, reason=hook.reason)

        if hook.action == HookAction.MODIFY and hook.modify_input:
            # modify_input may contain {"strip_fields": [...]} or message transforms
            result = result.model_copy(
                update={"modified_messages": messages}
            )
            logger.debug(
                "hook_modified_compact_input",
                extra={"hook_id": hook.id},
            )

    return result


async def run_post_compact_hooks(
    org_id: str | None,
    summary: str,
) -> PostCompactResult:
    """Execute post-compact hooks after compaction completes.

    Hooks may:
      - MODIFY → alter summary (e.g. inject user instructions or redact content)
      - Inject additional messages to restore after compact
    """
    if not org_id:
        return PostCompactResult()

    all_hooks = await load_hooks_for_org(org_id)
    matched = match_hooks(all_hooks, "*", HookType.POST_COMPACT)

    if not matched:
        return PostCompactResult()

    result = PostCompactResult()

    for hook in matched:
        if hook.action == HookAction.MODIFY and hook.modify_output is not None:
            # modify_output may contain {"summary": "...", "inject": [...]}
            mod = hook.modify_output
            if isinstance(mod, dict):
                updated: dict[str, Any] = {}
                if "summary" in mod:
                    updated["modified_summary"] = mod["summary"]
                if "inject" in mod:
                    updated["inject_messages"] = mod["inject"]
                updated["modified_by"] = hook.id
                result = result.model_copy(update=updated)
            logger.debug(
                "hook_modified_compact_output",
                extra={"hook_id": hook.id},
            )

    return result


async def run_permission_hooks(
    org_id: str | None,
    tool_name: str,
    tool_input: dict[str, Any],
) -> PermissionCheckResult:
    """Execute permission-check hooks for a tool invocation.

    Runs before pre-tool-use hooks. Used for policy enforcement.
    """
    if not org_id:
        return PermissionCheckResult(allowed=True)

    all_hooks = await load_hooks_for_org(org_id)
    matched = match_hooks(all_hooks, tool_name, HookType.PERMISSION_CHECK)

    if not matched:
        return PermissionCheckResult(allowed=True)

    for hook in matched:
        if hook.action == HookAction.BLOCK:
            logger.info(
                "hook_permission_denied",
                extra={
                    "hook_id": hook.id,
                    "tool": tool_name,
                    "reason": hook.reason,
                },
            )
            return PermissionCheckResult(
                allowed=False,
                reason=hook.reason,
                blocked_by=hook.id,
            )

        if hook.action == HookAction.MODIFY:
            # MODIFY on permission means "allow but require confirmation"
            return PermissionCheckResult(
                allowed=True,
                requires_confirmation=True,
                reason=hook.reason,
            )

    return PermissionCheckResult(allowed=True)


async def run_lifecycle_hooks(
    org_id: str | None,
    hook_type: HookType,
    event_name: str,
    payload: dict[str, Any],
) -> LifecycleHookResult:
    """Execute hooks for session and subagent lifecycle events.

    Lifecycle hooks allow org-specific interception around runtime boundaries
    such as session start/end and subagent spawn/stop.
    """
    if not org_id:
        return LifecycleHookResult(proceed=True)

    all_hooks = await load_hooks_for_org(org_id)
    matched = match_hooks(all_hooks, event_name, hook_type)

    if not matched:
        return LifecycleHookResult(proceed=True)

    result = LifecycleHookResult(proceed=True)

    for hook in matched:
        if hook.action == HookAction.BLOCK:
            logger.info(
                "hook_blocked_lifecycle_event",
                extra={
                    "hook_id": hook.id,
                    "hook_type": hook.hook_type.value,
                    "event": event_name,
                    "reason": hook.reason,
                },
            )
            return LifecycleHookResult(proceed=False, reason=hook.reason)

        if hook.action == HookAction.MODIFY and hook.modify_output is not None:
            if isinstance(hook.modify_output, dict):
                merged = {**payload, **hook.modify_output}
                result = result.model_copy(
                    update={
                        "modified_payload": merged,
                        "modified_by": hook.id,
                    }
                )
                logger.debug(
                    "hook_modified_lifecycle_payload",
                    extra={
                        "hook_id": hook.id,
                        "hook_type": hook.hook_type.value,
                        "event": event_name,
                    },
                )

    return result
