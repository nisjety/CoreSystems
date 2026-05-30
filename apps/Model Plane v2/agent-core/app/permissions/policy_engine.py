"""Policy engine — unified per-tool, per-org, per-path permission evaluation.

Combines the permission evaluator, classifier approvals, destructive
warnings, and rate-limit checks into a single interface that the
turn loop calls before every tool execution.

Mirrors CC's permission chain: deny tracking → mode check → risk level
→ classifier auto-approve → destructive warning → rate limit.
"""

from __future__ import annotations

import logging
from dataclasses import dataclass, field
from typing import Any

from app.domain import ExecutionPolicy, PermissionMode as DomainPermMode
from app.messages.types import MessageType, RunEvent, build_event
from app.permissions.classifier import (
    ClassifierApproval,
    build_classifier_event,
    build_destructive_warning_event,
    classify_bash_command,
    detect_destructive_warnings,
)
from app.permissions.domain import (
    PermissionDecision,
    PermissionEvalResult,
    PermissionMode,
    ToolRiskLevel,
)
from app.permissions.evaluator import evaluate_permission

logger = logging.getLogger(__name__)


@dataclass
class PolicyVerdict:
    """Complete result of a policy evaluation for a single tool call."""

    allowed: bool
    decision: PermissionDecision
    tool_name: str
    risk_level: ToolRiskLevel = ToolRiskLevel.MODERATE
    reason: str = ""

    # Classifier info (if auto-approved)
    classifier_approval: ClassifierApproval | None = None

    # Destructive warnings (informational)
    destructive_warnings: list[str] = field(default_factory=list)

    # Rate limit info
    rate_limited: bool = False
    rate_limit_remaining: int | None = None

    # Events to emit
    events: list[RunEvent] = field(default_factory=list)


class PolicyEngine:
    """Evaluates all policy constraints for a tool call.

    Usage:
        engine = PolicyEngine(session_id, run_id, org_id, policy)
        verdict = await engine.check("bash", {"command": "rm -rf /"})
        if not verdict.allowed:
            # emit events, skip tool
    """

    def __init__(
        self,
        session_id: str,
        run_id: str,
        org_id: str | None,
        policy: ExecutionPolicy,
    ) -> None:
        self._session_id = session_id
        self._run_id = run_id
        self._org_id = org_id
        self._policy = policy
        self._call_counts: dict[str, int] = {}  # tool → invocation count for rate limiting

    async def check(
        self,
        tool_name: str,
        tool_input: dict[str, Any] | None = None,
        tool_use_id: str = "",
        turn_index: int = 0,
    ) -> PolicyVerdict:
        """Run the full policy chain for a tool call.

        Steps:
        1. Permission evaluator (mode, risk, denial tracking)
        2. Bash classifier (auto-approve safe commands)
        3. Destructive warning detection  
        4. Rate limit check
        """
        events: list[RunEvent] = []
        tool_input = tool_input or {}

        # Step 1: Core permission evaluation
        perm_mode = PermissionMode(self._policy.permission_mode.value)
        eval_result = await evaluate_permission(
            session_id=self._session_id,
            tool_name=tool_name,
            permission_mode=perm_mode,
            allowed_tools=self._policy.allowed_tools or None,
        )

        # Step 2: If INTERACTIVE and not auto-approved, try classifier
        classifier_approval: ClassifierApproval | None = None
        if eval_result.decision == PermissionDecision.ASK_USER:
            # For bash tools, try classifying the command
            if tool_name.startswith("bash") and "command" in tool_input:
                classifier_approval = classify_bash_command(
                    tool_use_id, tool_input["command"]
                )
                if classifier_approval is not None:
                    # Classifier says it's safe → override to ALLOW
                    eval_result = PermissionEvalResult(
                        decision=PermissionDecision.ALLOW,
                        tool_name=tool_name,
                        risk_level=ToolRiskLevel.SAFE,
                        reason=f"Classifier auto-approved: {classifier_approval.matched_rule}",
                    )
                    # Emit classifier event
                    classifier_evt = build_classifier_event(
                        self._run_id, self._session_id, tool_use_id, turn_index
                    )
                    if classifier_evt:
                        events.append(classifier_evt)

        # Step 3: Destructive warning detection (informational, even if allowed)
        destructive_warnings: list[str] = []
        if "command" in tool_input:
            warnings = detect_destructive_warnings(tool_input["command"])
            destructive_warnings = [w.warning for w in warnings]
            if warnings:
                events.extend(
                    build_destructive_warning_event(
                        self._run_id, self._session_id, tool_input["command"], turn_index
                    )
                )

        # Step 4: Rate limit check (simple per-tool counter)
        rate_limited = False
        rate_limit_remaining: int | None = None
        self._call_counts[tool_name] = self._call_counts.get(tool_name, 0) + 1

        # Per-tool rate limit: 50 calls per tool per run
        max_per_tool = 50
        if self._call_counts[tool_name] > max_per_tool:
            rate_limited = True
            eval_result = PermissionEvalResult(
                decision=PermissionDecision.DENY,
                tool_name=tool_name,
                risk_level=eval_result.risk_level,
                reason=f"Rate limit exceeded: {max_per_tool} calls per tool per run",
            )
            events.append(
                build_event(
                    run_id=self._run_id,
                    session_id=self._session_id,
                    msg_type=MessageType.RATE_LIMIT_HIT,
                    turn_index=turn_index,
                    data={"tool_name": tool_name, "count": self._call_counts[tool_name], "limit": max_per_tool},
                )
            )
        else:
            rate_limit_remaining = max_per_tool - self._call_counts[tool_name]

        allowed = eval_result.decision == PermissionDecision.ALLOW

        if not allowed and eval_result.decision == PermissionDecision.DENY:
            events.append(
                build_event(
                    run_id=self._run_id,
                    session_id=self._session_id,
                    msg_type=MessageType.PERMISSION_DENIED,
                    turn_index=turn_index,
                    data={"tool_name": tool_name, "reason": eval_result.reason},
                )
            )

        return PolicyVerdict(
            allowed=allowed,
            decision=eval_result.decision,
            tool_name=tool_name,
            risk_level=eval_result.risk_level,
            reason=eval_result.reason,
            classifier_approval=classifier_approval,
            destructive_warnings=destructive_warnings,
            rate_limited=rate_limited,
            rate_limit_remaining=rate_limit_remaining,
            events=events,
        )
