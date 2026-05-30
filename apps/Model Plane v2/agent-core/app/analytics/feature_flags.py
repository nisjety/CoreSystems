"""Feature flags — Redis-backed boolean flag service.

CC pattern: feature flags control gradual rollouts, experiment
assignments, and runtime toggles. Flags are stored in Redis with
a simple key-value model.
"""

from __future__ import annotations

import logging
from typing import Any

logger = logging.getLogger(__name__)

# Redis key prefix for feature flags
_FLAG_PREFIX = "ff:"
_FLAG_TTL = 300  # cache TTL in seconds


class FeatureFlagService:
    """Redis-backed feature flag evaluation.

    Falls back to in-memory defaults when Redis is unavailable.
    """

    def __init__(self, redis: Any | None = None) -> None:
        self._redis = redis
        self._defaults: dict[str, bool] = {}

    def set_default(self, flag: str, value: bool) -> None:
        """Set a local default for a flag (used when Redis is unavailable)."""
        self._defaults[flag] = value

    async def is_enabled(
        self,
        flag: str,
        org_id: str | None = None,
        default: bool = False,
    ) -> bool:
        """Check if a flag is enabled.

        Checks org-specific override first, then global flag, then default.
        """
        if self._redis is not None:
            try:
                # Org-specific override
                if org_id:
                    org_val = await self._redis.get(f"{_FLAG_PREFIX}{flag}:{org_id}")
                    if org_val is not None:
                        return org_val.lower() in ("1", "true", "yes")

                # Global flag
                val = await self._redis.get(f"{_FLAG_PREFIX}{flag}")
                if val is not None:
                    return val.lower() in ("1", "true", "yes")
            except Exception as exc:
                logger.warning(
                    "feature_flag_redis_error",
                    extra={"flag": flag, "error": str(exc)},
                )

        # Fall back to local defaults
        return self._defaults.get(flag, default)

    async def set_flag(self, flag: str, value: bool, org_id: str | None = None) -> None:
        """Set a flag value in Redis."""
        if self._redis is None:
            self._defaults[flag] = value
            return
        key = f"{_FLAG_PREFIX}{flag}"
        if org_id:
            key = f"{key}:{org_id}"
        await self._redis.set(key, "1" if value else "0", ex=_FLAG_TTL)

    async def get_all_defaults(self) -> dict[str, bool]:
        """Return all local defaults."""
        return dict(self._defaults)
