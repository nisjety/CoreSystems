"""Safety Telemetry — in-memory ring buffer for safety audit events.

Ported from v1 ai-core, de-coupled from SQLAlchemy and SafetyService
imports. Uses only structlog for structured emission.
"""

from __future__ import annotations

import hashlib
import logging
import uuid
from collections import deque
from dataclasses import asdict, dataclass, field
from datetime import datetime, timezone
from enum import Enum
from typing import Any

import structlog

from app.domain import SafetyVerdict

logger = structlog.get_logger(__name__)

_std_logger = logging.getLogger(__name__)


class SafetyEventType(str, Enum):
    INPUT_CHECK = "input_check"
    OUTPUT_CHECK = "output_check"
    IMAGE_CHECK = "image_check"
    AUDIO_CHECK = "audio_check"
    REVIEW_CREATED = "review_created"
    REVIEW_RESOLVED = "review_resolved"
    POLICY_UPDATED = "policy_updated"
    CACHE_HIT = "cache_hit"
    API_ERROR = "api_error"


@dataclass
class SafetyEvent:
    event_id: str
    event_type: SafetyEventType
    timestamp: str
    org_id: str
    request_id: str | None = None
    verdict: SafetyVerdict | None = None
    detail: dict[str, Any] | None = None
    latency_ms: int = 0


class SafetyTelemetryService:
    """In-memory ring-buffer for safety audit events.

    Thread-safe for single-process use (asyncio event loop).
    """

    def __init__(self, max_events: int = 10_000) -> None:
        self._events: deque[SafetyEvent] = deque(maxlen=max_events)
        self._metrics: dict[str, int] = {
            "total_checks": 0,
            "blocked": 0,
            "flagged": 0,
            "allowed": 0,
            "sdk_errors": 0,
        }
        # Per-org view: org_id -> same metric dict
        self._org_metrics: dict[str, dict[str, int]] = {}

    def record_event(
        self,
        *,
        org_id: str,
        request_id: str,
        event_type: SafetyEventType,
        verdict: SafetyVerdict,
        detail: dict[str, Any] | None = None,
        latency_ms: int = 0,
    ) -> None:
        event = SafetyEvent(
            event_id=str(uuid.uuid4()),
            event_type=event_type,
            timestamp=datetime.now(tz=timezone.utc).isoformat(),
            org_id=org_id,
            request_id=request_id,
            verdict=verdict,
            detail=detail,
            latency_ms=latency_ms,
        )
        self._events.append(event)
        self._update_metrics(org_id, verdict, event_type)

        logger.info(
            "safety_event",
            event_id=event.event_id,
            event_type=event_type.value,
            org_id=org_id,
            request_id=request_id,
            verdict=verdict.value,
            latency_ms=latency_ms,
        )

    def _update_metrics(
        self,
        org_id: str,
        verdict: SafetyVerdict,
        event_type: SafetyEventType,
    ) -> None:
        self._metrics["total_checks"] += 1
        if verdict == SafetyVerdict.BLOCKED:
            self._metrics["blocked"] += 1
        elif verdict == SafetyVerdict.FLAGGED:
            self._metrics["flagged"] += 1
        else:
            self._metrics["allowed"] += 1
        if event_type == SafetyEventType.API_ERROR:
            self._metrics["sdk_errors"] += 1

        if org_id not in self._org_metrics:
            self._org_metrics[org_id] = {
                "total_checks": 0,
                "blocked": 0,
                "flagged": 0,
                "allowed": 0,
                "sdk_errors": 0,
            }
        om = self._org_metrics[org_id]
        om["total_checks"] += 1
        if verdict == SafetyVerdict.BLOCKED:
            om["blocked"] += 1
        elif verdict == SafetyVerdict.FLAGGED:
            om["flagged"] += 1
        else:
            om["allowed"] += 1
        if event_type == SafetyEventType.API_ERROR:
            om["sdk_errors"] += 1

    def get_metrics(self, org_id: str | None = None) -> dict[str, Any]:
        if org_id:
            return dict(self._org_metrics.get(org_id, {}))
        return dict(self._metrics)

    def get_recent_events(self, org_id: str | None = None, limit: int = 100) -> list[dict[str, Any]]:
        events = list(self._events)
        if org_id:
            events = [e for e in events if e.org_id == org_id]
        return [asdict(e) for e in events[-limit:]]


_telemetry: SafetyTelemetryService | None = None


def get_safety_telemetry() -> SafetyTelemetryService:
    global _telemetry
    if _telemetry is None:
        _telemetry = SafetyTelemetryService()
    return _telemetry
