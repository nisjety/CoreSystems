"""Policy domain models.

Defines per-organisation resource limits and how violations are handled.
"""

from __future__ import annotations

from enum import Enum

from pydantic import BaseModel, Field


class PolicyLimitAction(str, Enum):
    """What to do when a limit is hit."""

    BLOCK = "block"       # Reject the request with 429
    WARN = "warn"         # Allow but emit a warning event
    TRUNCATE = "truncate" # Allow but truncate the resource


class OrgPolicyLimits(BaseModel):
    """Per-organisation policy limits.

    All limits use a value of ``-1`` to mean *unlimited*.
    """

    org_id: str

    # Concurrent run caps
    max_concurrent_runs: int = Field(default=10, ge=-1)
    max_concurrent_runs_per_user: int = Field(default=5, ge=-1)

    # Token budget per run
    max_tokens_per_run: int = Field(default=200_000, ge=-1)

    # Cost budget per run (USD)
    max_cost_usd_per_run: float = Field(default=5.0, ge=-1.0)

    # Monthly cost budget (USD, across all runs in the org)
    max_cost_usd_monthly: float = Field(default=500.0, ge=-1.0)

    # Action limits per run
    max_actions_per_run: int = Field(default=200, ge=-1)

    # Tool allow-list (empty = allow all)
    allowed_tools: list[str] = Field(default_factory=list)

    # What to do when cost limit is hit
    cost_limit_action: PolicyLimitAction = PolicyLimitAction.BLOCK

    def is_unlimited(self, field: str) -> bool:
        """Return True if the named field is set to -1 (unlimited)."""
        return getattr(self, field) == -1


class PolicyViolation(BaseModel):
    """Describes a single policy violation."""

    org_id: str
    run_id: str | None = None
    limit_name: str
    limit_value: float | int
    current_value: float | int
    action: PolicyLimitAction
    message: str
