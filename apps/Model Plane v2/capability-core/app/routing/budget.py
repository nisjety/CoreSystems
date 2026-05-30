"""Budget tracking — thin façade over redis budget counters."""

from __future__ import annotations

import logging

from app.domain import BudgetCheckResult, RoutingPolicy, UsageRecord
from app.redis_client import check_budget as _redis_check, record_usage as _redis_record

logger = logging.getLogger(__name__)


async def check(org_id: str, policy: RoutingPolicy) -> BudgetCheckResult:
    """Check whether an org is within its daily + monthly budget."""
    raw = await _redis_check(
        org_id,
        daily_limit=policy.daily_budget_nok,
        monthly_limit=policy.monthly_budget_nok,
    )
    return BudgetCheckResult(**raw)


async def record(usage: UsageRecord) -> None:
    """Record cost against daily/monthly/session counters."""
    await _redis_record(
        org_id=usage.org_id,
        session_id=usage.session_id,
        cost_nok=usage.cost_nok,
    )
    logger.debug(
        "usage recorded: org=%s cost=%.4f NOK",
        usage.org_id,
        usage.cost_nok,
    )
