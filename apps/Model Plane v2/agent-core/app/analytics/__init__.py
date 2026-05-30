"""Analytics — structured event publishing for agent activity.

Events are published to NATS on subject ``analytics.events.{org_id}``.
They are also stored in ``analytics_events`` table for queries that don't
have a live NATS consumer.

Event lifecycle:
  1.  Agent service calls ``analytics_publisher.publish(event)``
  2.  Publisher fire-and-forgets to NATS + DB (no await in hot path via background task)
  3.  Downstream consumers (dashboards, billing) subscribe to NATS subjects
"""

from __future__ import annotations

from app.analytics.domain import AnalyticsEvent, AnalyticsEventType

__all__ = ["AnalyticsEvent", "AnalyticsEventType"]
