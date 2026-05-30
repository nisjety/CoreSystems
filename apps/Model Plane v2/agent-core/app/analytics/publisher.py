"""Analytics event publisher — delegates to cost-core-v2 via CostClient."""
from __future__ import annotations

import asyncio
import logging

from app.analytics.domain import AnalyticsEvent
from app.cost_client import CostClient

logger = logging.getLogger(__name__)


class AnalyticsPublisher:
    """Fire-and-forget analytics event publisher.

    Events are forwarded to cost-core-v2 over HTTP via ``CostClient.emit_event``.
    The cost service owns the ``VELION_COST`` NATS stream and the
    ``analytics_events`` table; agent-core no longer writes either directly.
    """

    def __init__(self, cost_client: CostClient | None = None) -> None:
        self._cost_client = cost_client

    def set_cost_client(self, cost_client: CostClient) -> None:
        """Wire the cost client after application startup."""
        self._cost_client = cost_client

    async def emit(self, event: AnalyticsEvent) -> None:
        """Schedule the event for publication without awaiting delivery."""
        asyncio.ensure_future(self._publish(event))

    async def emit_and_wait(self, event: AnalyticsEvent) -> None:
        """Publish and wait for cost-core-v2 to acknowledge the event."""
        await self._publish(event)

    async def _publish(self, event: AnalyticsEvent) -> None:
        if self._cost_client is None:
            logger.debug(
                "analytics_publisher_unconfigured",
                extra={"event_id": event.event_id},
            )
            return
        try:
            await self._cost_client.emit_event(
                event_type=event.event_type.value,
                org_id=event.org_id,
                run_id=event.run_id,
                user_id=event.user_id,
                agent_id=event.agent_id,
                props=event.props,
            )
        except Exception as exc:  # noqa: BLE001 — fire-and-forget
            logger.warning(
                "analytics_emit_failed",
                extra={"event_id": event.event_id, "error": str(exc)},
            )


analytics_publisher = AnalyticsPublisher()
