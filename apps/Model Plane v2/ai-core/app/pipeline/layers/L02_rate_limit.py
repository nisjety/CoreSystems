"""Layer 2 — Rate limiting via Redis.

Checks per-org request and token budgets.
"""

from __future__ import annotations

import logging

from reasoning_runtime.redis_rate_limit import check_rate_limit

from app.domain import PipelineContext

logger = logging.getLogger(__name__)


async def run(ctx: PipelineContext) -> PipelineContext:
    try:
        allowed = await check_rate_limit(org_id=ctx.org_id)
    except RuntimeError:
        # Redis not initialised — skip rate limiting
        logger.debug("L02_rate_limit skipped (redis unavailable) org=%s", ctx.org_id)
        return ctx

    if not allowed:
        logger.warning("L02_rate_limit blocked org=%s", ctx.org_id)
        raise ValueError("Rate limit exceeded")

    logger.debug("L02_rate_limit ok org=%s", ctx.org_id)
    return ctx
