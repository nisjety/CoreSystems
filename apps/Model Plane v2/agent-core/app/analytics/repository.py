"""Analytics event persistence — DEPRECATED.

Moved to cost-core-v2. Use CostClient.emit_event from app.cost_client.
Preserved as stubs to keep import paths stable; migration 017 drops the
analytics_events table.
"""
from __future__ import annotations

import logging
from typing import Any

from app.analytics.domain import AnalyticsEvent

logger = logging.getLogger(__name__)


async def insert_event(conn: Any, event: AnalyticsEvent) -> None:
    raise NotImplementedError(
        "analytics_events persistence moved to cost-core-v2; "
        "use CostClient.emit_event instead"
    )


async def query_events(*args: Any, **kwargs: Any) -> list[AnalyticsEvent]:
    raise NotImplementedError(
        "analytics_events moved to cost-core-v2; "
        "query via cost-core-v2 API"
    )
