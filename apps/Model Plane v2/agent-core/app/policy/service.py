"""Policy enforcement service.

Pre-flight and mid-run limit checks.  All checks are *advisory* in the
sense that the caller decides the response action; this service just
evaluates the policy and returns a ``PolicyViolation`` if one occurs.
"""

from __future__ import annotations

import logging
from typing import Any

from app.policy import OrgPolicyLimits, PolicyLimitAction, PolicyViolation

logger = logging.getLogger(__name__)

# Default fallback policy (permissive) used when no org policy is configured
_DEFAULT_POLICY = OrgPolicyLimits(org_id="__default__")


class PolicyService:
    """Stateless policy checker — inject an org policy, get violations back."""

    # ----------------------------------------------------------------
    # Pre-flight checks (before a run is started)
    # ----------------------------------------------------------------

    def check_concurrent_runs(
        self,
        policy: OrgPolicyLimits,
        org_id: str,
        current_runs: int,
    ) -> PolicyViolation | None:
        if policy.is_unlimited("max_concurrent_runs"):
            return None
        if current_runs >= policy.max_concurrent_runs:
            return PolicyViolation(
                org_id=org_id,
                limit_name="max_concurrent_runs",
                limit_value=policy.max_concurrent_runs,
                current_value=current_runs,
                action=PolicyLimitAction.BLOCK,
                message=(
                    f"Organisation has reached the concurrent run limit "
                    f"({current_runs}/{policy.max_concurrent_runs})."
                ),
            )
        return None

    def check_concurrent_runs_per_user(
        self,
        policy: OrgPolicyLimits,
        org_id: str,
        user_runs: int,
    ) -> PolicyViolation | None:
        if policy.is_unlimited("max_concurrent_runs_per_user"):
            return None
        if user_runs >= policy.max_concurrent_runs_per_user:
            return PolicyViolation(
                org_id=org_id,
                limit_name="max_concurrent_runs_per_user",
                limit_value=policy.max_concurrent_runs_per_user,
                current_value=user_runs,
                action=PolicyLimitAction.BLOCK,
                message=(
                    f"User has reached the per-user concurrent run limit "
                    f"({user_runs}/{policy.max_concurrent_runs_per_user})."
                ),
            )
        return None

    # ----------------------------------------------------------------
    # Mid-run checks (called after each turn / action)
    # ----------------------------------------------------------------

    def check_token_budget(
        self,
        policy: OrgPolicyLimits,
        org_id: str,
        run_id: str,
        tokens_used: int,
    ) -> PolicyViolation | None:
        if policy.is_unlimited("max_tokens_per_run"):
            return None
        if tokens_used >= policy.max_tokens_per_run:
            return PolicyViolation(
                org_id=org_id,
                run_id=run_id,
                limit_name="max_tokens_per_run",
                limit_value=policy.max_tokens_per_run,
                current_value=tokens_used,
                action=policy.cost_limit_action,
                message=(
                    f"Run has consumed {tokens_used:,} tokens "
                    f"(limit: {policy.max_tokens_per_run:,})."
                ),
            )
        return None

    def check_cost_budget_run(
        self,
        policy: OrgPolicyLimits,
        org_id: str,
        run_id: str,
        cost_usd: float,
    ) -> PolicyViolation | None:
        if policy.is_unlimited("max_cost_usd_per_run"):
            return None
        if cost_usd >= policy.max_cost_usd_per_run:
            return PolicyViolation(
                org_id=org_id,
                run_id=run_id,
                limit_name="max_cost_usd_per_run",
                limit_value=policy.max_cost_usd_per_run,
                current_value=cost_usd,
                action=policy.cost_limit_action,
                message=(
                    f"Run cost ${cost_usd:.4f} has exceeded per-run limit "
                    f"${policy.max_cost_usd_per_run:.4f}."
                ),
            )
        return None

    def check_cost_budget_monthly(
        self,
        policy: OrgPolicyLimits,
        org_id: str,
        monthly_cost_usd: float,
    ) -> PolicyViolation | None:
        if policy.is_unlimited("max_cost_usd_monthly"):
            return None
        if monthly_cost_usd >= policy.max_cost_usd_monthly:
            return PolicyViolation(
                org_id=org_id,
                limit_name="max_cost_usd_monthly",
                limit_value=policy.max_cost_usd_monthly,
                current_value=monthly_cost_usd,
                action=PolicyLimitAction.BLOCK,
                message=(
                    f"Organisation monthly cost ${monthly_cost_usd:.2f} has exceeded "
                    f"the monthly limit ${policy.max_cost_usd_monthly:.2f}."
                ),
            )
        return None

    def check_action_limit(
        self,
        policy: OrgPolicyLimits,
        org_id: str,
        run_id: str,
        actions_taken: int,
    ) -> PolicyViolation | None:
        if policy.is_unlimited("max_actions_per_run"):
            return None
        if actions_taken >= policy.max_actions_per_run:
            return PolicyViolation(
                org_id=org_id,
                run_id=run_id,
                limit_name="max_actions_per_run",
                limit_value=policy.max_actions_per_run,
                current_value=actions_taken,
                action=PolicyLimitAction.BLOCK,
                message=(
                    f"Run has taken {actions_taken} actions "
                    f"(limit: {policy.max_actions_per_run})."
                ),
            )
        return None

    def check_tool_allowed(
        self,
        policy: OrgPolicyLimits,
        org_id: str,
        run_id: str,
        tool_name: str,
    ) -> PolicyViolation | None:
        if not policy.allowed_tools:
            return None  # empty = allow all
        if tool_name not in policy.allowed_tools:
            return PolicyViolation(
                org_id=org_id,
                run_id=run_id,
                limit_name="allowed_tools",
                limit_value=0,
                current_value=0,
                action=PolicyLimitAction.BLOCK,
                message=f"Tool '{tool_name}' is not in the organisation's allowed_tools list.",
            )
        return None


# Module-level singleton (stateless)
policy_service = PolicyService()
