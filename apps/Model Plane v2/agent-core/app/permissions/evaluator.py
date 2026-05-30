"""Permission evaluator — decides whether a tool call is allowed.

Mirrors CC's toolPermission evaluator chain:
1. Check if tool was previously denied in this session → DENY
2. Check permission mode (trust/interactive/coordinator/swarm)
3. Check per-tool risk level against mode thresholds
4. Return decision: ALLOW, DENY, or ASK_USER

In COORDINATOR mode, workers inherit the parent's allowed_tools list.
In SWARM mode, the coordinator pre-approves a tool allowlist.
"""

from __future__ import annotations

import fnmatch
import logging
from typing import Any

from app.permissions.denial_tracker import is_denied
from app.permissions.domain import (
    DenialRecord,
    PermissionDecision,
    PermissionEvalResult,
    PermissionMode,
    ToolPermissionRule,
    ToolRiskLevel,
)

logger = logging.getLogger(__name__)

# Default tool risk classifications
DEFAULT_RISK_RULES: list[ToolPermissionRule] = [
    # Dangerous: destructive file/system operations
    ToolPermissionRule(tool_pattern="bash:*", risk_level=ToolRiskLevel.DANGEROUS),
    ToolPermissionRule(tool_pattern="delete_file", risk_level=ToolRiskLevel.DANGEROUS),
    ToolPermissionRule(tool_pattern="git_push*", risk_level=ToolRiskLevel.DANGEROUS),
    ToolPermissionRule(tool_pattern="deploy*", risk_level=ToolRiskLevel.DANGEROUS),
    ToolPermissionRule(tool_pattern="drop_*", risk_level=ToolRiskLevel.DANGEROUS),
    # Moderate: mutations that are reversible
    ToolPermissionRule(tool_pattern="write_file", risk_level=ToolRiskLevel.MODERATE),
    ToolPermissionRule(tool_pattern="edit_file", risk_level=ToolRiskLevel.MODERATE),
    ToolPermissionRule(tool_pattern="create_*", risk_level=ToolRiskLevel.MODERATE),
    ToolPermissionRule(tool_pattern="mcp:*", risk_level=ToolRiskLevel.MODERATE),
    # Safe: read-only
    ToolPermissionRule(tool_pattern="read_file", risk_level=ToolRiskLevel.SAFE, auto_approve=True),
    ToolPermissionRule(tool_pattern="search_*", risk_level=ToolRiskLevel.SAFE, auto_approve=True),
    ToolPermissionRule(tool_pattern="list_*", risk_level=ToolRiskLevel.SAFE, auto_approve=True),
    ToolPermissionRule(tool_pattern="get_*", risk_level=ToolRiskLevel.SAFE, auto_approve=True),
]


async def evaluate_permission(
    session_id: str,
    tool_name: str,
    permission_mode: PermissionMode,
    allowed_tools: list[str] | None = None,
    custom_rules: list[ToolPermissionRule] | None = None,
) -> PermissionEvalResult:
    """Evaluate whether a tool call should be allowed.

    Args:
        session_id: Current session for denial tracking.
        tool_name: Name of the tool being called.
        permission_mode: The active permission mode.
        allowed_tools: Explicit allowlist (for COORDINATOR/SWARM modes).
        custom_rules: Per-org tool risk overrides.

    Returns:
        PermissionEvalResult with the decision.
    """
    # Step 1: Check denial tracker
    if await is_denied(session_id, tool_name):
        return PermissionEvalResult(
            decision=PermissionDecision.DENY,
            tool_name=tool_name,
            reason="Previously denied in this session",
            was_previously_denied=True,
        )

    # Step 2: TRUST mode — everything allowed
    if permission_mode == PermissionMode.TRUST:
        return PermissionEvalResult(
            decision=PermissionDecision.ALLOW,
            tool_name=tool_name,
            reason="Trust mode: all tools allowed",
        )

    # Step 3: COORDINATOR/SWARM — check explicit allowlist
    if permission_mode in (PermissionMode.COORDINATOR, PermissionMode.SWARM):
        if allowed_tools is not None:
            if _tool_matches_allowlist(tool_name, allowed_tools):
                return PermissionEvalResult(
                    decision=PermissionDecision.ALLOW,
                    tool_name=tool_name,
                    reason=f"{permission_mode.value}: tool in allowlist",
                )
            return PermissionEvalResult(
                decision=PermissionDecision.DENY,
                tool_name=tool_name,
                reason=f"{permission_mode.value}: tool not in allowlist",
            )
        # No allowlist → fall through to risk evaluation
        # (coordinator inherits parent's rules)

    # Step 4: INTERACTIVE — check tool risk level
    risk_level = _classify_risk(tool_name, custom_rules)

    if risk_level == ToolRiskLevel.SAFE:
        return PermissionEvalResult(
            decision=PermissionDecision.ALLOW,
            tool_name=tool_name,
            risk_level=risk_level,
            reason="Safe tool: auto-approved",
        )

    if risk_level == ToolRiskLevel.DANGEROUS:
        return PermissionEvalResult(
            decision=PermissionDecision.ASK_USER,
            tool_name=tool_name,
            risk_level=risk_level,
            reason="Dangerous tool: requires user confirmation",
        )

    # Moderate risk in interactive mode — allow but log
    if permission_mode == PermissionMode.INTERACTIVE:
        return PermissionEvalResult(
            decision=PermissionDecision.ALLOW,
            tool_name=tool_name,
            risk_level=risk_level,
            reason="Moderate risk: allowed in interactive mode",
        )

    # Default: allow
    return PermissionEvalResult(
        decision=PermissionDecision.ALLOW,
        tool_name=tool_name,
        risk_level=risk_level,
        reason="Default: allowed",
    )


def _tool_matches_allowlist(tool_name: str, allowlist: list[str]) -> bool:
    """Check if a tool matches any pattern in the allowlist."""
    for pattern in allowlist:
        if fnmatch.fnmatch(tool_name, pattern):
            return True
    return False


def _classify_risk(
    tool_name: str,
    custom_rules: list[ToolPermissionRule] | None = None,
) -> ToolRiskLevel:
    """Classify a tool's risk level using custom rules then defaults."""
    rules = (custom_rules or []) + DEFAULT_RISK_RULES

    for rule in rules:
        if fnmatch.fnmatch(tool_name, rule.tool_pattern):
            return rule.risk_level

    return ToolRiskLevel.MODERATE  # Default to moderate
