"""Quota tracking — utilization, limits, and overage detection.

CC pattern: track quota across different limit windows (5-hour, 7-day,
model-specific) and detect when approaching or exceeding limits.
"""

from __future__ import annotations

import logging
from datetime import datetime, timezone
from enum import Enum
from typing import Any

from pydantic import BaseModel, Field

logger = logging.getLogger(__name__)


class LimitType(str, Enum):
    """Rate limit window type."""

    HOURLY = "hourly"
    FIVE_HOUR = "5h"
    DAILY = "daily"
    SEVEN_DAY = "7d"
    MODEL_SPECIFIC = "model"
    UNKNOWN = "unknown"


class QuotaStatus(BaseModel):
    """Current quota state for a model/org combination."""

    model_id: str
    org_id: str
    limit_type: LimitType = LimitType.UNKNOWN
    utilization: float = 0.0  # 0.0 to 1.0+
    limit: int = 0
    remaining: int = 0
    reset_time: datetime | None = None
    is_exceeded: bool = False
    overage_tokens: int = 0

    @property
    def is_warning(self) -> bool:
        """True if utilization exceeds 80%."""
        return self.utilization >= 0.8 and not self.is_exceeded

    @property
    def seconds_until_reset(self) -> float | None:
        """Seconds until quota resets."""
        if self.reset_time is None:
            return None
        delta = (self.reset_time - datetime.now(timezone.utc)).total_seconds()
        return max(0.0, delta)


class QuotaTracker:
    """In-memory quota tracker per model/org."""

    def __init__(self) -> None:
        self._quotas: dict[str, QuotaStatus] = {}

    def _key(self, org_id: str, model_id: str) -> str:
        return f"{org_id}:{model_id}"

    def update(self, status: QuotaStatus) -> None:
        """Update quota status for a model/org."""
        key = self._key(status.org_id, status.model_id)
        self._quotas[key] = status

    def get(self, org_id: str, model_id: str) -> QuotaStatus | None:
        """Get current quota status."""
        return self._quotas.get(self._key(org_id, model_id))

    def check(self, org_id: str, model_id: str) -> tuple[bool, str]:
        """Check if a request should proceed.

        Returns (should_proceed, reason).
        """
        status = self.get(org_id, model_id)
        if status is None:
            return True, "no_quota_info"
        if status.is_exceeded:
            return False, f"quota_exceeded:{status.limit_type.value}"
        return True, "ok"

    def list_exceeded(self) -> list[QuotaStatus]:
        """List all exceeded quotas."""
        return [q for q in self._quotas.values() if q.is_exceeded]

    def list_warnings(self) -> list[QuotaStatus]:
        """List all quotas at >80% utilization."""
        return [q for q in self._quotas.values() if q.is_warning]

    def clear(self) -> None:
        self._quotas.clear()
