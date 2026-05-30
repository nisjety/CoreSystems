"""
Data Plane per-user rate limiting using Redis sliding window counters.

Usage:
    limiter = RateLimiter(redis_client, requests_per_minute=100)
    allowed, retry_after = await limiter.check("user_123", "/v1/retrieve")
"""
from __future__ import annotations

import time
from typing import Optional, Tuple


class RateLimiter:
    """Redis-based sliding window rate limiter."""

    def __init__(
        self,
        redis_client,
        requests_per_minute: int = 100,
    ):
        self._redis = redis_client
        self._limit = requests_per_minute
        self._window = 60  # seconds

    async def check(self, user_id: str, endpoint: str) -> Tuple[bool, Optional[int]]:
        """
        Check if request is allowed.

        Returns:
            (True, None)           — allowed
            (False, retry_after)   — blocked, with seconds until window resets
        """
        now = int(time.time())
        window_start = now - self._window
        key = f"ratelimit:{user_id}:{endpoint}:{now // self._window}"

        pipe = self._redis.pipeline()
        pipe.incr(key)
        pipe.expire(key, self._window + 1)
        results = await pipe.execute()

        current_count = results[0]

        if current_count > self._limit:
            retry_after = self._window - (now % self._window)
            return False, retry_after

        return True, None
