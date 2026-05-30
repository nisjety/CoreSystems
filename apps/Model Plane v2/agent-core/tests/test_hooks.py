"""Unit tests for the hook system (Phase D)."""

from __future__ import annotations

import pytest

from app.hooks.domain import (
    HookAction,
    HookBlockedError,
    HookConfig,
    HookType,
    PostToolUseResult,
    PreToolUseResult,
)
from app.hooks.registry import _pattern_matches, match_hooks


# ---------------------------------------------------------------------------
# Pattern matching
# ---------------------------------------------------------------------------


class TestPatternMatching:
    def test_wildcard_star_matches_everything(self) -> None:
        assert _pattern_matches("*", "bash:run")
        assert _pattern_matches("*", "mcp:github:search")

    def test_prefix_wildcard(self) -> None:
        assert _pattern_matches("bash:*", "bash:run")
        assert _pattern_matches("bash:*", "bash:exec")
        assert not _pattern_matches("bash:*", "mcp:github:search")

    def test_mcp_wildcard(self) -> None:
        assert _pattern_matches("mcp:*", "mcp:github:search")
        assert _pattern_matches("mcp:*", "mcp:stripe:create_payment")
        assert not _pattern_matches("mcp:*", "bash:run")

    def test_exact_match(self) -> None:
        assert _pattern_matches("file_read", "file_read")
        assert not _pattern_matches("file_read", "file_write")

    def test_nested_wildcard(self) -> None:
        assert _pattern_matches("mcp:github:*", "mcp:github:search")
        assert not _pattern_matches("mcp:github:*", "mcp:stripe:search")


# ---------------------------------------------------------------------------
# Hook matching
# ---------------------------------------------------------------------------


def _make_hook(
    pattern: str = "*",
    hook_type: HookType = HookType.PRE_TOOL_USE,
    action: HookAction = HookAction.APPROVE,
    priority: int = 0,
) -> HookConfig:
    return HookConfig(
        org_id="test-org",
        tool_name_pattern=pattern,
        hook_type=hook_type,
        action=action,
        priority=priority,
    )


class TestMatchHooks:
    def test_matches_by_type_and_pattern(self) -> None:
        hooks = [
            _make_hook("bash:*", HookType.PRE_TOOL_USE, HookAction.BLOCK),
            _make_hook("*", HookType.POST_TOOL_USE, HookAction.APPROVE),
            _make_hook("*", HookType.PRE_TOOL_USE, HookAction.APPROVE),
        ]
        matched = match_hooks(hooks, "bash:run", HookType.PRE_TOOL_USE)
        assert len(matched) == 2
        # bash:* hook should match + "*" hook should match
        patterns = {h.tool_name_pattern for h in matched}
        assert "bash:*" in patterns
        assert "*" in patterns

    def test_no_match(self) -> None:
        hooks = [_make_hook("bash:*", HookType.PRE_TOOL_USE)]
        matched = match_hooks(hooks, "mcp:github:search", HookType.PRE_TOOL_USE)
        assert len(matched) == 0

    def test_sorted_by_priority_desc(self) -> None:
        hooks = [
            _make_hook("*", priority=10),
            _make_hook("*", priority=50),
            _make_hook("*", priority=1),
        ]
        matched = match_hooks(hooks, "anything", HookType.PRE_TOOL_USE)
        priorities = [h.priority for h in matched]
        assert priorities == [50, 10, 1]

    def test_type_filter(self) -> None:
        hooks = [
            _make_hook("*", HookType.PRE_TOOL_USE),
            _make_hook("*", HookType.POST_TOOL_USE),
            _make_hook("*", HookType.STOP),
        ]
        matched = match_hooks(hooks, "tool", HookType.STOP)
        assert len(matched) == 1
        assert matched[0].hook_type == HookType.STOP


# ---------------------------------------------------------------------------
# Domain models
# ---------------------------------------------------------------------------


class TestHookBlockedError:
    def test_message_format(self) -> None:
        err = HookBlockedError(hook_id="h1", tool_name="bash:rm", reason="dangerous")
        assert "h1" in str(err)
        assert "bash:rm" in str(err)
        assert "dangerous" in str(err)

    def test_attributes(self) -> None:
        err = HookBlockedError(hook_id="h2", tool_name="file_delete", reason="blocked")
        assert err.hook_id == "h2"
        assert err.tool_name == "file_delete"
        assert err.reason == "blocked"


class TestPreToolUseResult:
    def test_defaults_to_approved(self) -> None:
        result = PreToolUseResult()
        assert result.approved is True
        assert result.modified_input is None
        assert result.blocked_by is None

    def test_with_modified_input(self) -> None:
        result = PreToolUseResult(
            approved=True,
            modified_input={"safe": True},
        )
        assert result.modified_input == {"safe": True}


class TestPostToolUseResult:
    def test_defaults_to_no_modification(self) -> None:
        result = PostToolUseResult()
        assert result.modified_output is None
        assert result.modified_by is None
