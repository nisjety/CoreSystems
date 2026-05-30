"""Multi-sink analytics dispatcher with event sampling.

CC pattern: analytics events can be routed to multiple sinks
(NATS, Postgres, HTTP webhook) with configurable sampling rates
and PII stripping for non-privileged sinks.
"""

from __future__ import annotations

import hashlib
import logging
from enum import Enum
from typing import Any, Protocol

from pydantic import BaseModel, Field

from app.analytics.domain import AnalyticsEvent, AnalyticsEventType
from app.analytics.pii import strip_pii

logger = logging.getLogger(__name__)


class SinkType(str, Enum):
    NATS = "nats"
    POSTGRES = "postgres"
    HTTP = "http"


class SinkConfig(BaseModel):
    """Configuration for a single analytics sink."""

    name: str
    sink_type: SinkType
    privileged: bool = True  # If False, PII is stripped
    enabled: bool = True
    sample_rate: float = 1.0  # 0.0 - 1.0 (1.0 = 100%)


class AnalyticsSink(Protocol):
    """Protocol for an analytics sink backend."""

    async def send(self, event: AnalyticsEvent) -> None: ...

    @property
    def config(self) -> SinkConfig: ...


class SinkDispatcher:
    """Routes events to multiple sinks with sampling and PII control."""

    def __init__(self) -> None:
        self._sinks: list[AnalyticsSink] = []
        self._sample_overrides: dict[AnalyticsEventType, float] = {}

    def register(self, sink: AnalyticsSink) -> None:
        """Register a sink backend."""
        self._sinks.append(sink)

    def set_sample_rate(self, event_type: AnalyticsEventType, rate: float) -> None:
        """Override sample rate for a specific event type (0.0 - 1.0)."""
        self._sample_overrides[event_type] = max(0.0, min(1.0, rate))

    async def dispatch(self, event: AnalyticsEvent) -> int:
        """Send event to all eligible sinks. Returns count of sinks used."""
        sent = 0
        for sink in self._sinks:
            if not sink.config.enabled:
                continue
            if not self._should_sample(event, sink.config.sample_rate):
                continue

            # PII stripping for non-privileged sinks
            actual_event = event
            if not sink.config.privileged and event.props:
                clean_props = strip_pii(dict(event.props))
                actual_event = event.model_copy(update={"props": clean_props})

            try:
                await sink.send(actual_event)
                sent += 1
            except Exception as exc:
                logger.warning(
                    "analytics_sink_failed",
                    extra={
                        "sink": sink.config.name,
                        "event_id": event.event_id,
                        "error": str(exc),
                    },
                )
        return sent

    def _should_sample(self, event: AnalyticsEvent, sink_rate: float) -> bool:
        """Deterministic sampling based on event_id hash."""
        # Per-event-type overrides take priority
        rate = self._sample_overrides.get(event.event_type, sink_rate)
        if rate >= 1.0:
            return True
        if rate <= 0.0:
            return False
        # Deterministic: same event_id → same decision across sinks
        h = int(hashlib.md5(event.event_id.encode()).hexdigest()[:8], 16)
        threshold = int(rate * 0xFFFFFFFF)
        return h <= threshold
