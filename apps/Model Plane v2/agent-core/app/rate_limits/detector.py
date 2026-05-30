"""Rate-limit header detection.

Parses Anthropic-style rate-limit headers:
  - anthropic-ratelimit-requests-limit
  - anthropic-ratelimit-requests-remaining
  - anthropic-ratelimit-requests-reset
  - anthropic-ratelimit-tokens-limit
  - anthropic-ratelimit-tokens-remaining
  - anthropic-ratelimit-tokens-reset
  - retry-after (standard HTTP)
"""

from __future__ import annotations

import logging
from datetime import datetime, timezone
from typing import Any

from pydantic import BaseModel, Field

logger = logging.getLogger(__name__)


class RateLimitInfo(BaseModel):
    """Parsed rate-limit state from response headers."""

    requests_limit: int | None = None
    requests_remaining: int | None = None
    requests_reset: datetime | None = None
    tokens_limit: int | None = None
    tokens_remaining: int | None = None
    tokens_reset: datetime | None = None
    retry_after_seconds: float | None = None

    @property
    def is_rate_limited(self) -> bool:
        """True if remaining requests or tokens are zero."""
        if self.requests_remaining is not None and self.requests_remaining <= 0:
            return True
        if self.tokens_remaining is not None and self.tokens_remaining <= 0:
            return True
        return False

    @property
    def request_utilization(self) -> float | None:
        """Request utilization as a fraction [0, 1]."""
        if self.requests_limit and self.requests_remaining is not None:
            return 1.0 - (self.requests_remaining / self.requests_limit)
        return None

    @property
    def token_utilization(self) -> float | None:
        """Token utilization as a fraction [0, 1]."""
        if self.tokens_limit and self.tokens_remaining is not None:
            return 1.0 - (self.tokens_remaining / self.tokens_limit)
        return None


def _parse_int(headers: dict[str, str], key: str) -> int | None:
    v = headers.get(key)
    if v is not None:
        try:
            return int(v)
        except ValueError:
            pass
    return None


def _parse_datetime(headers: dict[str, str], key: str) -> datetime | None:
    v = headers.get(key)
    if v is None:
        return None
    try:
        return datetime.fromisoformat(v.replace("Z", "+00:00"))
    except (ValueError, AttributeError):
        return None


def _parse_retry_after(headers: dict[str, str]) -> float | None:
    v = headers.get("retry-after")
    if v is None:
        return None
    try:
        return float(v)
    except ValueError:
        # Could be HTTP-date, try parsing
        try:
            dt = datetime.strptime(v, "%a, %d %b %Y %H:%M:%S GMT")
            dt = dt.replace(tzinfo=timezone.utc)
            delta = (dt - datetime.now(timezone.utc)).total_seconds()
            return max(0.0, delta)
        except ValueError:
            return None


def detect_rate_limits(headers: dict[str, str]) -> RateLimitInfo:
    """Parse rate-limit information from HTTP response headers."""
    prefix = "anthropic-ratelimit-"
    return RateLimitInfo(
        requests_limit=_parse_int(headers, f"{prefix}requests-limit"),
        requests_remaining=_parse_int(headers, f"{prefix}requests-remaining"),
        requests_reset=_parse_datetime(headers, f"{prefix}requests-reset"),
        tokens_limit=_parse_int(headers, f"{prefix}tokens-limit"),
        tokens_remaining=_parse_int(headers, f"{prefix}tokens-remaining"),
        tokens_reset=_parse_datetime(headers, f"{prefix}tokens-reset"),
        retry_after_seconds=_parse_retry_after(headers),
    )
