"""Tests for Phase S: Policy Limits."""

from __future__ import annotations

import pytest

from app.policy import OrgPolicyLimits, PolicyLimitAction, PolicyViolation
from app.policy.service import PolicyService


# ────────────────────────────────────────────────────────────────────────────
# OrgPolicyLimits
# ────────────────────────────────────────────────────────────────────────────

class TestOrgPolicyLimits:
    def test_defaults_are_permissive(self) -> None:
        p = OrgPolicyLimits(org_id="org1")
        assert p.max_concurrent_runs == 10
        assert p.max_tokens_per_run == 200_000
        assert p.max_cost_usd_per_run == 5.0
        assert p.allowed_tools == []

    def test_is_unlimited_true(self) -> None:
        p = OrgPolicyLimits(org_id="org1", max_concurrent_runs=-1)
        assert p.is_unlimited("max_concurrent_runs") is True

    def test_is_unlimited_false(self) -> None:
        p = OrgPolicyLimits(org_id="org1", max_concurrent_runs=5)
        assert p.is_unlimited("max_concurrent_runs") is False

    def test_cost_limit_action_default_block(self) -> None:
        p = OrgPolicyLimits(org_id="org1")
        assert p.cost_limit_action == PolicyLimitAction.BLOCK


# ────────────────────────────────────────────────────────────────────────────
# PolicyService — concurrent runs
# ────────────────────────────────────────────────────────────────────────────

class TestPolicyConcurrentRuns:
    def _svc(self) -> PolicyService:
        return PolicyService()

    def test_no_violation_below_limit(self) -> None:
        p = OrgPolicyLimits(org_id="o1", max_concurrent_runs=5)
        v = self._svc().check_concurrent_runs(p, "o1", current_runs=4)
        assert v is None

    def test_violation_at_limit(self) -> None:
        p = OrgPolicyLimits(org_id="o1", max_concurrent_runs=5)
        v = self._svc().check_concurrent_runs(p, "o1", current_runs=5)
        assert v is not None
        assert v.limit_name == "max_concurrent_runs"
        assert v.action == PolicyLimitAction.BLOCK

    def test_violation_above_limit(self) -> None:
        p = OrgPolicyLimits(org_id="o1", max_concurrent_runs=2)
        v = self._svc().check_concurrent_runs(p, "o1", current_runs=10)
        assert v is not None

    def test_unlimited_never_violated(self) -> None:
        p = OrgPolicyLimits(org_id="o1", max_concurrent_runs=-1)
        v = self._svc().check_concurrent_runs(p, "o1", current_runs=999)
        assert v is None

    def test_per_user_violation(self) -> None:
        p = OrgPolicyLimits(org_id="o1", max_concurrent_runs_per_user=3)
        v = self._svc().check_concurrent_runs_per_user(p, "o1", user_runs=3)
        assert v is not None
        assert v.limit_name == "max_concurrent_runs_per_user"


# ────────────────────────────────────────────────────────────────────────────
# PolicyService — token budget
# ────────────────────────────────────────────────────────────────────────────

class TestPolicyTokenBudget:
    def _svc(self) -> PolicyService:
        return PolicyService()

    def test_no_violation_below(self) -> None:
        p = OrgPolicyLimits(org_id="o1", max_tokens_per_run=1000)
        v = self._svc().check_token_budget(p, "o1", "r1", tokens_used=999)
        assert v is None

    def test_violation_at_limit(self) -> None:
        p = OrgPolicyLimits(org_id="o1", max_tokens_per_run=1000)
        v = self._svc().check_token_budget(p, "o1", "r1", tokens_used=1000)
        assert v is not None
        assert v.run_id == "r1"

    def test_cost_action_propagated(self) -> None:
        p = OrgPolicyLimits(
            org_id="o1",
            max_tokens_per_run=100,
            cost_limit_action=PolicyLimitAction.WARN,
        )
        v = self._svc().check_token_budget(p, "o1", "r1", tokens_used=200)
        assert v is not None
        assert v.action == PolicyLimitAction.WARN


# ────────────────────────────────────────────────────────────────────────────
# PolicyService — cost budget
# ────────────────────────────────────────────────────────────────────────────

class TestPolicyCostBudget:
    def _svc(self) -> PolicyService:
        return PolicyService()

    def test_per_run_no_violation(self) -> None:
        p = OrgPolicyLimits(org_id="o1", max_cost_usd_per_run=5.0)
        v = self._svc().check_cost_budget_run(p, "o1", "r1", cost_usd=4.99)
        assert v is None

    def test_per_run_violation(self) -> None:
        p = OrgPolicyLimits(org_id="o1", max_cost_usd_per_run=5.0)
        v = self._svc().check_cost_budget_run(p, "o1", "r1", cost_usd=5.0)
        assert v is not None
        assert v.limit_name == "max_cost_usd_per_run"

    def test_monthly_no_violation(self) -> None:
        p = OrgPolicyLimits(org_id="o1", max_cost_usd_monthly=100.0)
        v = self._svc().check_cost_budget_monthly(p, "o1", 99.99)
        assert v is None

    def test_monthly_violation(self) -> None:
        p = OrgPolicyLimits(org_id="o1", max_cost_usd_monthly=100.0)
        v = self._svc().check_cost_budget_monthly(p, "o1", 100.0)
        assert v is not None
        assert v.action == PolicyLimitAction.BLOCK


# ────────────────────────────────────────────────────────────────────────────
# PolicyService — action limit
# ────────────────────────────────────────────────────────────────────────────

class TestPolicyActionLimit:
    def _svc(self) -> PolicyService:
        return PolicyService()

    def test_no_violation_below(self) -> None:
        p = OrgPolicyLimits(org_id="o1", max_actions_per_run=50)
        v = self._svc().check_action_limit(p, "o1", "r1", actions_taken=49)
        assert v is None

    def test_violation_at_limit(self) -> None:
        p = OrgPolicyLimits(org_id="o1", max_actions_per_run=50)
        v = self._svc().check_action_limit(p, "o1", "r1", actions_taken=50)
        assert v is not None
        assert v.limit_name == "max_actions_per_run"


# ────────────────────────────────────────────────────────────────────────────
# PolicyService — tool allow-list
# ────────────────────────────────────────────────────────────────────────────

class TestPolicyToolAllowList:
    def _svc(self) -> PolicyService:
        return PolicyService()

    def test_empty_allowed_tools_allows_all(self) -> None:
        p = OrgPolicyLimits(org_id="o1", allowed_tools=[])
        v = self._svc().check_tool_allowed(p, "o1", "r1", "any_tool")
        assert v is None

    def test_tool_in_list_passes(self) -> None:
        p = OrgPolicyLimits(org_id="o1", allowed_tools=["search", "retrieve"])
        v = self._svc().check_tool_allowed(p, "o1", "r1", "search")
        assert v is None

    def test_tool_not_in_list_blocked(self) -> None:
        p = OrgPolicyLimits(org_id="o1", allowed_tools=["search"])
        v = self._svc().check_tool_allowed(p, "o1", "r1", "shell")
        assert v is not None
        assert "shell" in v.message
        assert v.action == PolicyLimitAction.BLOCK


# ────────────────────────────────────────────────────────────────────────────
# PolicyViolation model
# ────────────────────────────────────────────────────────────────────────────

class TestPolicyViolation:
    def test_violation_has_message(self) -> None:
        v = PolicyViolation(
            org_id="o1",
            run_id="r1",
            limit_name="max_actions_per_run",
            limit_value=200,
            current_value=201,
            action=PolicyLimitAction.BLOCK,
            message="Too many actions.",
        )
        assert "Too many" in v.message
        assert v.run_id == "r1"
