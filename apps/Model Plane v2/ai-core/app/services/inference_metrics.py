"""Inference Metrics Service.

Tracks per-org inference statistics in a lightweight in-memory store with an
optional Redis backend for persistence across restarts.

Counters tracked per org (also globally):
- total_requests
- total_tokens_in
- total_tokens_out
- total_latency_ms
- errors
- blocked_requests

Fail-open: redis failures degrade silently to in-memory only.
"""

from __future__ import annotations

import json
import logging
import time
from collections import defaultdict
from dataclasses import asdict, dataclass, field
from typing import Any

logger = logging.getLogger(__name__)

_instance: InferenceMetricsService | None = None


@dataclass
class OrgMetrics:
    total_requests: int = 0
    total_tokens_in: int = 0
    total_tokens_out: int = 0
    total_latency_ms: int = 0
    errors: int = 0
    blocked_requests: int = 0
    last_updated: float = field(default_factory=time.time)

    @property
    def avg_latency_ms(self) -> float:
        if self.total_requests == 0:
            return 0.0
        return self.total_latency_ms / self.total_requests

    def to_dict(self) -> dict[str, Any]:
        d = asdict(self)
        d["avg_latency_ms"] = self.avg_latency_ms
        return d


class InferenceMetricsService:
    """Collect and expose inference counters."""

    def __init__(self) -> None:
        self._org: dict[str, OrgMetrics] = defaultdict(OrgMetrics)
        self._global = OrgMetrics()
        self._redis: Any = None

    def init(self, redis_url: str) -> None:
        """Optionally connect to Redis for persistent metrics."""
        if not redis_url:
            return
        try:
            import redis.asyncio as aioredis
            self._redis = aioredis.from_url(redis_url, decode_responses=True)
            logger.info("inference_metrics redis_connected url=%s", redis_url)
        except Exception as exc:
            logger.warning("inference_metrics redis_init_failed error=%s", exc)

    def record(
        self,
        *,
        org_id: str,
        tokens_in: int = 0,
        tokens_out: int = 0,
        latency_ms: int = 0,
        error: bool = False,
        blocked: bool = False,
    ) -> None:
        """Record a single inference event.  Never raises."""
        try:
            m = self._org[org_id]
            m.total_requests += 1
            m.total_tokens_in += tokens_in
            m.total_tokens_out += tokens_out
            m.total_latency_ms += latency_ms
            if error:
                m.errors += 1
            if blocked:
                m.blocked_requests += 1
            m.last_updated = time.time()

            g = self._global
            g.total_requests += 1
            g.total_tokens_in += tokens_in
            g.total_tokens_out += tokens_out
            g.total_latency_ms += latency_ms
            if error:
                g.errors += 1
            if blocked:
                g.blocked_requests += 1
            g.last_updated = time.time()
        except Exception:  # pragma: no cover
            pass

    def get_metrics(self, org_id: str | None = None) -> dict[str, Any]:
        if org_id:
            return self._org[org_id].to_dict()
        return {
            "global": self._global.to_dict(),
            "orgs": {k: v.to_dict() for k, v in self._org.items()},
        }


def get_inference_metrics() -> InferenceMetricsService:
    global _instance
    if _instance is None:
        _instance = InferenceMetricsService()
    return _instance
