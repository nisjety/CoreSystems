"""Tests for Phase B1: Hooks — Pre/Post Compact + Permission Hooks."""

from __future__ import annotations

import pytest
from unittest.mock import AsyncMock, patch
from typing import Any

from app.hooks.domain import (
    HookAction,
    HookConfig,
    HookType,
    PreCompactResult,
    PostCompactResult,
    PermissionCheckResult,
)
from app.hooks.executor import (
    run_pre_compact_hooks,
    run_post_compact_hooks,
    run_permission_hooks,
)


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def _make_hook(
    hook_type: HookType,
    action: HookAction,
    tool_pattern: str = "*",
    reason: str = "",
    modify_input: dict[str, Any] | None = None,
    modify_output: dict[str, Any] | None = None,
    priority: int = 0,
) -> HookConfig:
    return HookConfig(
        org_id="test-org",
        tool_name_pattern=tool_pattern,
        hook_type=hook_type,
        action=action,
        reason=reason,
        modify_input=modify_input,
        modify_output=modify_output,
        priority=priority,
    )


def _patch_hooks(hooks: list[HookConfig]):
    """Patch load_hooks_for_org to return the given hooks."""
    return patch(
        "app.hooks.executor.load_hooks_for_org",
        new_callable=AsyncMock,
        return_value=hooks,
    )


# ===========================================================================
# HookType enum tests
# ===========================================================================

class TestHookTypeEnum:
    def test_new_hook_types_exist(self):
        assert HookType.PRE_COMPACT == "pre_compact"
        assert HookType.POST_COMPACT == "post_compact"
        assert HookType.PERMISSION_CHECK == "permission_check"

    def test_original_types_still_exist(self):
        assert HookType.PRE_TOOL_USE == "pre_tool_use"
        assert HookType.POST_TOOL_USE == "post_tool_use"
        assert HookType.STOP == "stop"


# ===========================================================================
# Domain model tests
# ===========================================================================

class TestDomainModels:
    def test_pre_compact_result_defaults(self):
        r = PreCompactResult()
        assert r.proceed is True
        assert r.modified_messages is None
        assert r.reason is None

    def test_post_compact_result_defaults(self):
        r = PostCompactResult()
        assert r.modified_summary is None
        assert r.inject_messages is None

    def test_permission_result_defaults(self):
        r = PermissionCheckResult()
        assert r.allowed is True
        assert r.requires_confirmation is False
        assert r.blocked_by is None


# ===========================================================================
# Pre-compact hook tests
# ===========================================================================

class TestPreCompactHooks:
    @pytest.mark.asyncio
    async def test_no_org_skips_hooks(self):
        result = await run_pre_compact_hooks(None, [{"role": "user", "content": "hi"}])
        assert result.proceed is True

    @pytest.mark.asyncio
    async def test_no_matching_hooks(self):
        hooks = [_make_hook(HookType.PRE_TOOL_USE, HookAction.BLOCK)]  # wrong type
        with _patch_hooks(hooks):
            result = await run_pre_compact_hooks("test-org", [])
        assert result.proceed is True

    @pytest.mark.asyncio
    async def test_block_prevents_compaction(self):
        hooks = [_make_hook(HookType.PRE_COMPACT, HookAction.BLOCK, reason="data retention")]
        with _patch_hooks(hooks):
            result = await run_pre_compact_hooks("test-org", [{"role": "user", "content": "hi"}])
        assert result.proceed is False
        assert result.reason == "data retention"

    @pytest.mark.asyncio
    async def test_modify_passes_messages(self):
        hooks = [_make_hook(
            HookType.PRE_COMPACT,
            HookAction.MODIFY,
            modify_input={"strip_fields": ["sensitive"]},
        )]
        messages = [{"role": "user", "content": "secret"}]
        with _patch_hooks(hooks):
            result = await run_pre_compact_hooks("test-org", messages)
        assert result.proceed is True
        assert result.modified_messages is not None

    @pytest.mark.asyncio
    async def test_approve_is_noop(self):
        hooks = [_make_hook(HookType.PRE_COMPACT, HookAction.APPROVE)]
        with _patch_hooks(hooks):
            result = await run_pre_compact_hooks("test-org", [])
        assert result.proceed is True
        assert result.modified_messages is None


# ===========================================================================
# Post-compact hook tests
# ===========================================================================

class TestPostCompactHooks:
    @pytest.mark.asyncio
    async def test_no_org_skips_hooks(self):
        result = await run_post_compact_hooks(None, "summary text")
        assert result.modified_summary is None

    @pytest.mark.asyncio
    async def test_modify_summary(self):
        hooks = [_make_hook(
            HookType.POST_COMPACT,
            HookAction.MODIFY,
            modify_output={"summary": "redacted summary"},
        )]
        with _patch_hooks(hooks):
            result = await run_post_compact_hooks("test-org", "original")
        assert result.modified_summary == "redacted summary"
        assert result.modified_by is not None

    @pytest.mark.asyncio
    async def test_inject_messages(self):
        hooks = [_make_hook(
            HookType.POST_COMPACT,
            HookAction.MODIFY,
            modify_output={
                "inject": [{"role": "system", "content": "Remember: security policy X"}]
            },
        )]
        with _patch_hooks(hooks):
            result = await run_post_compact_hooks("test-org", "summary")
        assert result.inject_messages is not None
        assert len(result.inject_messages) == 1

    @pytest.mark.asyncio
    async def test_no_matching_hooks(self):
        hooks = [_make_hook(HookType.PRE_TOOL_USE, HookAction.APPROVE)]
        with _patch_hooks(hooks):
            result = await run_post_compact_hooks("test-org", "summary")
        assert result.modified_summary is None


# ===========================================================================
# Permission hook tests
# ===========================================================================

class TestPermissionHooks:
    @pytest.mark.asyncio
    async def test_no_org_allows(self):
        result = await run_permission_hooks(None, "bash", {"command": "ls"})
        assert result.allowed is True

    @pytest.mark.asyncio
    async def test_block_denies(self):
        hooks = [_make_hook(
            HookType.PERMISSION_CHECK,
            HookAction.BLOCK,
            tool_pattern="bash",
            reason="bash disabled for this org",
        )]
        with _patch_hooks(hooks):
            result = await run_permission_hooks("test-org", "bash", {"command": "rm -rf"})
        assert result.allowed is False
        assert "disabled" in result.reason
        assert result.blocked_by is not None

    @pytest.mark.asyncio
    async def test_modify_requires_confirmation(self):
        hooks = [_make_hook(
            HookType.PERMISSION_CHECK,
            HookAction.MODIFY,
            tool_pattern="file_write",
            reason="destructive action",
        )]
        with _patch_hooks(hooks):
            result = await run_permission_hooks("test-org", "file_write", {"path": "/etc/x"})
        assert result.allowed is True
        assert result.requires_confirmation is True

    @pytest.mark.asyncio
    async def test_approve_allows(self):
        hooks = [_make_hook(
            HookType.PERMISSION_CHECK,
            HookAction.APPROVE,
            tool_pattern="file_read",
        )]
        with _patch_hooks(hooks):
            result = await run_permission_hooks("test-org", "file_read", {"path": "/tmp/x"})
        assert result.allowed is True
        assert result.requires_confirmation is False

    @pytest.mark.asyncio
    async def test_no_matching_hooks_allows(self):
        hooks = [_make_hook(HookType.PERMISSION_CHECK, HookAction.BLOCK, tool_pattern="bash")]
        with _patch_hooks(hooks):
            result = await run_permission_hooks("test-org", "file_read", {"path": "/tmp/x"})
        assert result.allowed is True
