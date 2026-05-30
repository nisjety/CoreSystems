"""Rate limit service — orchestrates detection, backoff, and quota.

Provides the top-level ``should_retry()`` decision for the LLM client
retry loop.
"""

from __future__ import annotations

import logging
from typing import Any

from app.rate_limits.backoff import (
    MAX_RETRIES,
    compute_backoff_ms,
    should_retry as backoff_should_retry,
)
from app.rate_limits.detector import RateLimitInfo, detect_rate_limits
from app.rate_limits.quota import LimitType, QuotaStatus, QuotaTracker

logger = logging.getLogger(__name__)


class RetryDecision:
    """Result of should_retry evaluation."""

    __slots__ = ("retry", "delay_ms", "reason")

    def __init__(self, retry: bool, delay_ms: float = 0.0, reason: str = "") -> None:
        self.retry = retry
        self.delay_ms = delay_ms
        self.reason = reason


class RateLimitService:
    """Coordinates rate-limit checking and retry decisions.

    Usage in llm_client retry loop:
    1. After each response, call ``process_headers(headers, status_code, ...)``
    2. If status is 429, call ``should_retry(attempt)`` for the delay
    3. Quota warnings are updated automatically
    """

    def __init__(self, max_retries: int = MAX_RETRIES) -> None:
        self._quota = QuotaTracker()
        self._max_retries = max_retries
        self._last_info: RateLimitInfo | None = None

    @property
    def quota(self) -> QuotaTracker:
        return self._quota

    @property
    def last_info(self) -> RateLimitInfo | None:
        return self._last_info

    def process_headers(
        self,
        headers: dict[str, str],
        status_code: int,
        org_id: str = "",
        model_id: str = "",
    ) -> RateLimitInfo:
        """Parse response headers and update quota state."""
        info = detect_rate_limits(headers)
        self._last_info = info

        if org_id and model_id:
            self._update_quota(info, org_id, model_id)

        if info.is_rate_limited:
            logger.warning(
                "rate_limited",
                extra={
                    "model": model_id,
                    "org": org_id,
                    "req_remaining": info.requests_remaining,
                    "tok_remaining": info.tokens_remaining,
                },
            )
        return info

    def should_retry(
        self,
        attempt: int,
        status_code: int,
    ) -> RetryDecision:
        """Decide whether to retry a failed request.

        Handles HTTP 429 (rate limited) and 529 (overloaded).
        """
        if status_code not in (429, 529):
            return RetryDecision(retry=False, reason="not_retryable")

        if not backoff_should_retry(attempt, self._max_retries):
            return RetryDecision(retry=False, reason="max_retries_exceeded")

        # Use retry-after header if available, otherwise compute backoff
        if self._last_info and self._last_info.retry_after_seconds:
            delay_ms = self._last_info.retry_after_seconds * 1000
        else:
            delay_ms = compute_backoff_ms(attempt)

        return RetryDecision(
            retry=True,
            delay_ms=delay_ms,
            reason=f"rate_limited_attempt_{attempt}",
        )

    def _update_quota(
        self, info: RateLimitInfo, org_id: str, model_id: str
    ) -> None:
        """Update quota tracker from rate-limit info."""
        utilization = 0.0
        remaining = 0
        limit = 0

        if info.request_utilization is not None:
            utilization = info.request_utilization
            remaining = info.requests_remaining or 0
            limit = info.requests_limit or 0
        elif info.token_utilization is not None:
            utilization = info.token_utilization
            remaining = info.tokens_remaining or 0
            limit = info.tokens_limit or 0

        status = QuotaStatus(
            model_id=model_id,
            org_id=org_id,
            utilization=utilization,
            limit=limit,
            remaining=remaining,
            reset_time=info.requests_reset or info.tokens_reset,
            is_exceeded=info.is_rate_limited,
        )
        self._quota.update(status)
