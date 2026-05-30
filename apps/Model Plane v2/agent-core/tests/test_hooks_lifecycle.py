"""Tests for lifecycle hooks on session and subagent boundaries."""

from __future__ import annotations

from typing import Any
from unittest.mock import AsyncMock, patch

import pytest

from app.hooks.domain import (
    HookAction,
    HookConfig,
    HookType,
    LifecycleHookResult,
)
from app.hooks.executor import run_lifecycle_hooks


def _make_hook(
    hook_type: HookType,
    action: HookAction,
    pattern: str = "*",
    reason: str = "",
    modify_output: dict[str, Any] | None = None,
    priority: int = 0,
) -> HookConfig:
    return HookConfig(
        org_id="test-org",
        tool_name_pattern=pattern,
        hook_type=hook_type,
        action=action,
        reason=reason,
        modify_output=modify_output,
        priority=priority,
    )


def _patch_hooks(hooks: list[HookConfig]):
    return patch(
        "app.hooks.executor.load_hooks_for_org",
        new_callable=AsyncMock,
        return_value=hooks,
    )


class TestLifecycleHookTypes:
    def test_session_and_subagent_hook_types_exist(self) -> None:
        assert HookType.SESSION_START == "session_start"
        assert HookType.SESSION_END == "session_end"
        assert HookType.SUBAGENT_START == "subagent_start"
        assert HookType.SUBAGENT_STOP == "subagent_stop"


class TestLifecycleHookResult:
    def test_defaults_allow_and_preserve_payload(self) -> None:
        result = LifecycleHookResult()

        assert result.proceed is True
        assert result.modified_payload is None
        assert result.reason is None
        assert result.modified_by is None


class TestRunLifecycleHooks:
    @pytest.mark.asyncio
    async def test_no_org_skips_lifecycle_hooks(self) -> None:
        result = await run_lifecycle_hooks(
            None,
            HookType.SESSION_START,
            "session:start",
            {"session_id": "s1"},
        )

        assert result.proceed is True
        assert result.modified_payload is None

    @pytest.mark.asyncio
    async def test_wrong_hook_type_is_ignored(self) -> None:
        hooks = [_make_hook(HookType.PRE_TOOL_USE, HookAction.BLOCK)]

        with _patch_hooks(hooks):
            result = await run_lifecycle_hooks(
                "test-org",
                HookType.SESSION_START,
                "session:start",
                {"session_id": "s1"},
            )

        assert result.proceed is True
        assert result.modified_payload is None

    @pytest.mark.asyncio
    async def test_block_stops_session_lifecycle_event(self) -> None:
        hooks = [
            _make_hook(
                HookType.SESSION_START,
                HookAction.BLOCK,
                pattern="session:*",
                reason="session start disabled",
            )
        ]

        with _patch_hooks(hooks):
            result = await run_lifecycle_hooks(
                "test-org",
                HookType.SESSION_START,
                "session:start",
                {"session_id": "s1"},
            )

        assert result.proceed is False
        assert result.reason == "session start disabled"

    @pytest.mark.asyncio
    async def test_modify_merges_payload(self) -> None:
        hooks = [
            _make_hook(
                HookType.SUBAGENT_START,
                HookAction.MODIFY,
                pattern="subagent:start:*",
                modify_output={"trace_source": "hook", "priority": "high"},
            )
        ]

        with _patch_hooks(hooks):
            result = await run_lifecycle_hooks(
                "test-org",
                HookType.SUBAGENT_START,
                "subagent:start:researcher",
                {"child_agent_id": "researcher", "priority": "normal"},
            )

        assert result.proceed is True
        assert result.modified_payload == {
            "child_agent_id": "researcher",
            "priority": "high",
            "trace_source": "hook",
        }
        assert result.modified_by is not None

    @pytest.mark.asyncio
    async def test_approve_is_noop(self) -> None:
        hooks = [
            _make_hook(
                HookType.SESSION_END,
                HookAction.APPROVE,
                pattern="session:end",
            )
        ]

        with _patch_hooks(hooks):
            result = await run_lifecycle_hooks(
                "test-org",
                HookType.SESSION_END,
                "session:end",
                {"session_id": "s1", "total_runs": 2},
            )

        assert result.proceed is True
        assert result.modified_payload is None