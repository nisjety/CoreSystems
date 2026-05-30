"""Tests for Phase K — Permission system (evaluator, denial tracker, domain)."""

from __future__ import annotations

import pytest

from app.permissions.domain import (
    DenialRecord,
    PermissionDecision,
    PermissionEvalResult,
    PermissionMode,
    ToolPermissionRule,
    ToolRiskLevel,
)
from app.permissions.evaluator import (
    _classify_risk,
    _tool_matches_allowlist,
    evaluate_permission,
)
from app.permissions.denial_tracker import (
    _denial_cache,
    clear_session_denials,
    is_denied,
    record_denial,
)


# ---------------------------------------------------------------------------
# Domain types
# ---------------------------------------------------------------------------


class TestPermissionDomain:
    def test_permission_modes(self) -> None:
        assert PermissionMode.TRUST.value == "trust"
        assert PermissionMode.INTERACTIVE.value == "interactive"
        assert PermissionMode.COORDINATOR.value == "coordinator"
        assert PermissionMode.SWARM.value == "swarm"

    def test_risk_levels(self) -> None:
        assert ToolRiskLevel.SAFE.value == "safe"
        assert ToolRiskLevel.DANGEROUS.value == "dangerous"

    def test_denial_record_defaults(self) -> None:
        record = DenialRecord(session_id="s1", tool_name="bash:rm")
        assert record.denial_count == 1
        assert record.reason == ""

    def test_eval_result(self) -> None:
        result = PermissionEvalResult(
            decision=PermissionDecision.ALLOW,
            tool_name="read_file",
        )
        assert result.was_previously_denied is False


# ---------------------------------------------------------------------------
# Risk classification
# ---------------------------------------------------------------------------


class TestClassifyRisk:
    def test_bash_is_dangerous(self) -> None:
        assert _classify_risk("bash:rm") == ToolRiskLevel.DANGEROUS

    def test_read_file_is_safe(self) -> None:
        assert _classify_risk("read_file") == ToolRiskLevel.SAFE

    def test_write_file_is_moderate(self) -> None:
        assert _classify_risk("write_file") == ToolRiskLevel.MODERATE

    def test_mcp_is_moderate(self) -> None:
        assert _classify_risk("mcp:server:tool") == ToolRiskLevel.MODERATE

    def test_unknown_tool_is_moderate(self) -> None:
        assert _classify_risk("unknown_tool_xyz") == ToolRiskLevel.MODERATE

    def test_custom_rule_overrides_default(self) -> None:
        custom = [
            ToolPermissionRule(
                tool_pattern="read_file",
                risk_level=ToolRiskLevel.DANGEROUS,
            )
        ]
        assert _classify_risk("read_file", custom) == ToolRiskLevel.DANGEROUS

    def test_search_is_safe(self) -> None:
        assert _classify_risk("search_code") == ToolRiskLevel.SAFE

    def test_list_is_safe(self) -> None:
        assert _classify_risk("list_files") == ToolRiskLevel.SAFE


# ---------------------------------------------------------------------------
# Allowlist matching
# ---------------------------------------------------------------------------


class TestToolMatchesAllowlist:
    def test_exact_match(self) -> None:
        assert _tool_matches_allowlist("read_file", ["read_file"]) is True

    def test_glob_match(self) -> None:
        assert _tool_matches_allowlist("bash:ls", ["bash:*"]) is True

    def test_no_match(self) -> None:
        assert _tool_matches_allowlist("bash:rm", ["read_file"]) is False

    def test_empty_allowlist(self) -> None:
        assert _tool_matches_allowlist("read_file", []) is False


# ---------------------------------------------------------------------------
# Permission evaluator
# ---------------------------------------------------------------------------


@pytest.mark.asyncio
class TestEvaluatePermission:
    def setup_method(self) -> None:
        _denial_cache.clear()

    async def test_trust_mode_allows_all(self) -> None:
        result = await evaluate_permission(
            session_id="s1",
            tool_name="bash:rm -rf /",
            permission_mode=PermissionMode.TRUST,
        )
        assert result.decision == PermissionDecision.ALLOW

    async def test_interactive_dangerous_asks_user(self) -> None:
        result = await evaluate_permission(
            session_id="s1",
            tool_name="bash:rm",
            permission_mode=PermissionMode.INTERACTIVE,
        )
        assert result.decision == PermissionDecision.ASK_USER

    async def test_interactive_safe_allows(self) -> None:
        result = await evaluate_permission(
            session_id="s1",
            tool_name="read_file",
            permission_mode=PermissionMode.INTERACTIVE,
        )
        assert result.decision == PermissionDecision.ALLOW

    async def test_coordinator_with_allowlist_allows(self) -> None:
        result = await evaluate_permission(
            session_id="s1",
            tool_name="read_file",
            permission_mode=PermissionMode.COORDINATOR,
            allowed_tools=["read_file", "search_*"],
        )
        assert result.decision == PermissionDecision.ALLOW

    async def test_coordinator_with_allowlist_denies(self) -> None:
        result = await evaluate_permission(
            session_id="s1",
            tool_name="bash:rm",
            permission_mode=PermissionMode.COORDINATOR,
            allowed_tools=["read_file"],
        )
        assert result.decision == PermissionDecision.DENY

    async def test_previously_denied_blocked(self) -> None:
        # Record denial in memory cache
        _denial_cache["s1"] = {"bash:rm": DenialRecord(session_id="s1", tool_name="bash:rm")}
        result = await evaluate_permission(
            session_id="s1",
            tool_name="bash:rm",
            permission_mode=PermissionMode.INTERACTIVE,
        )
        assert result.decision == PermissionDecision.DENY
        assert result.was_previously_denied is True


# ---------------------------------------------------------------------------
# Denial tracker
# ---------------------------------------------------------------------------


@pytest.mark.asyncio
class TestDenialTracker:
    def setup_method(self) -> None:
        _denial_cache.clear()

    async def test_record_denial(self) -> None:
        record = await record_denial("s1", "bash:rm", "user declined")
        assert record.denial_count == 1
        assert record.reason == "user declined"

    async def test_is_denied_after_record(self) -> None:
        await record_denial("s1", "bash:rm")
        assert await is_denied("s1", "bash:rm") is True

    async def test_is_not_denied_without_record(self) -> None:
        assert await is_denied("s1", "bash:rm") is False

    async def test_clear_denials(self) -> None:
        await record_denial("s1", "bash:rm")
        await clear_session_denials("s1")
        assert await is_denied("s1", "bash:rm") is False

    async def test_repeated_denial_increments(self) -> None:
        await record_denial("s1", "bash:rm")
        record = await record_denial("s1", "bash:rm")
        assert record.denial_count == 2
