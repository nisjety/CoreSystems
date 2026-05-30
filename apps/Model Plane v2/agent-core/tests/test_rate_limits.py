"""Tests for Phase C1: Rate Limit Handling."""

from __future__ import annotations

from datetime import datetime, timezone

import pytest

from app.rate_limits.backoff import (
    MAX_DELAY_MS,
    compute_backoff_ms,
    compute_backoff_seconds,
    should_retry,
)
from app.rate_limits.detector import RateLimitInfo, detect_rate_limits
from app.rate_limits.quota import LimitType, QuotaStatus, QuotaTracker
from app.rate_limits.service import RateLimitService, RetryDecision


# ── Detector ───────────────────────────────────────────────────

class TestDetector:
    def test_empty_headers(self):
        info = detect_rate_limits({})
        assert info.requests_limit is None
        assert not info.is_rate_limited

    def test_anthropic_headers(self):
        headers = {
            "anthropic-ratelimit-requests-limit": "100",
            "anthropic-ratelimit-requests-remaining": "50",
            "anthropic-ratelimit-tokens-limit": "100000",
            "anthropic-ratelimit-tokens-remaining": "80000",
        }
        info = detect_rate_limits(headers)
        assert info.requests_limit == 100
        assert info.requests_remaining == 50
        assert info.tokens_limit == 100000
        assert info.tokens_remaining == 80000

    def test_rate_limited_zero_remaining(self):
        headers = {
            "anthropic-ratelimit-requests-limit": "100",
            "anthropic-ratelimit-requests-remaining": "0",
        }
        info = detect_rate_limits(headers)
        assert info.is_rate_limited is True

    def test_retry_after_seconds(self):
        headers = {"retry-after": "30"}
        info = detect_rate_limits(headers)
        assert info.retry_after_seconds == 30.0

    def test_reset_time_parsing(self):
        headers = {
            "anthropic-ratelimit-requests-reset": "2025-06-01T12:00:00Z",
        }
        info = detect_rate_limits(headers)
        assert info.requests_reset is not None
        assert info.requests_reset.year == 2025

    def test_utilization_calculation(self):
        info = RateLimitInfo(requests_limit=100, requests_remaining=25)
        assert info.request_utilization == pytest.approx(0.75)

    def test_token_utilization(self):
        info = RateLimitInfo(tokens_limit=1000, tokens_remaining=100)
        assert info.token_utilization == pytest.approx(0.9)

    def test_utilization_none_when_missing(self):
        info = RateLimitInfo()
        assert info.request_utilization is None
        assert info.token_utilization is None

    def test_invalid_header_ignored(self):
        headers = {"anthropic-ratelimit-requests-limit": "not-a-number"}
        info = detect_rate_limits(headers)
        assert info.requests_limit is None


# ── Backoff ────────────────────────────────────────────────────

class TestBackoff:
    def test_base_delay(self):
        delay = compute_backoff_ms(0, jitter=0)
        assert delay == 500.0

    def test_exponential_growth(self):
        d0 = compute_backoff_ms(0, jitter=0)
        d1 = compute_backoff_ms(1, jitter=0)
        d2 = compute_backoff_ms(2, jitter=0)
        assert d1 == d0 * 2
        assert d2 == d0 * 4

    def test_max_cap(self):
        delay = compute_backoff_ms(20, jitter=0)
        assert delay == MAX_DELAY_MS

    def test_jitter_varies(self):
        delays = {compute_backoff_ms(2, jitter=0.25) for _ in range(20)}
        assert len(delays) > 1  # jitter should produce variation

    def test_should_retry_within_limit(self):
        assert should_retry(0) is True
        assert should_retry(4) is True

    def test_should_retry_exceeds_limit(self):
        assert should_retry(5) is False

    def test_seconds_conversion(self):
        s = compute_backoff_seconds(0, jitter=0)
        assert s == pytest.approx(0.5)


# ── Quota ──────────────────────────────────────────────────────

class TestQuota:
    def test_quota_status_warning(self):
        qs = QuotaStatus(
            model_id="m1",
            org_id="o1",
            utilization=0.85,
            limit=100,
            remaining=15,
        )
        assert qs.is_warning is True
        assert qs.is_exceeded is False

    def test_quota_status_exceeded(self):
        qs = QuotaStatus(
            model_id="m1",
            org_id="o1",
            utilization=1.1,
            is_exceeded=True,
        )
        assert qs.is_warning is False
        assert qs.is_exceeded is True

    def test_tracker_update_and_get(self):
        tracker = QuotaTracker()
        qs = QuotaStatus(model_id="m1", org_id="o1", utilization=0.5)
        tracker.update(qs)
        result = tracker.get("o1", "m1")
        assert result is not None
        assert result.utilization == 0.5

    def test_tracker_check_no_info(self):
        tracker = QuotaTracker()
        ok, reason = tracker.check("o1", "m1")
        assert ok is True
        assert reason == "no_quota_info"

    def test_tracker_check_exceeded(self):
        tracker = QuotaTracker()
        tracker.update(
            QuotaStatus(
                model_id="m1",
                org_id="o1",
                is_exceeded=True,
                limit_type=LimitType.FIVE_HOUR,
            )
        )
        ok, reason = tracker.check("o1", "m1")
        assert ok is False
        assert "5h" in reason

    def test_list_exceeded(self):
        tracker = QuotaTracker()
        tracker.update(QuotaStatus(model_id="m1", org_id="o1", is_exceeded=True))
        tracker.update(QuotaStatus(model_id="m2", org_id="o1", is_exceeded=False))
        assert len(tracker.list_exceeded()) == 1

    def test_list_warnings(self):
        tracker = QuotaTracker()
        tracker.update(QuotaStatus(model_id="m1", org_id="o1", utilization=0.9))
        tracker.update(QuotaStatus(model_id="m2", org_id="o1", utilization=0.3))
        assert len(tracker.list_warnings()) == 1

    def test_seconds_until_reset(self):
        future = datetime(2099, 1, 1, tzinfo=timezone.utc)
        qs = QuotaStatus(
            model_id="m1",
            org_id="o1",
            reset_time=future,
        )
        assert qs.seconds_until_reset is not None
        assert qs.seconds_until_reset > 0


# ── Service ────────────────────────────────────────────────────

class TestRateLimitService:
    def test_process_headers_updates_quota(self):
        svc = RateLimitService()
        headers = {
            "anthropic-ratelimit-requests-limit": "100",
            "anthropic-ratelimit-requests-remaining": "10",
        }
        info = svc.process_headers(headers, 200, org_id="o1", model_id="m1")
        assert info.requests_remaining == 10
        qs = svc.quota.get("o1", "m1")
        assert qs is not None
        assert qs.utilization == pytest.approx(0.9)

    def test_should_retry_429(self):
        svc = RateLimitService()
        decision = svc.should_retry(0, 429)
        assert decision.retry is True
        assert decision.delay_ms > 0

    def test_should_retry_529(self):
        svc = RateLimitService()
        decision = svc.should_retry(0, 529)
        assert decision.retry is True

    def test_no_retry_200(self):
        svc = RateLimitService()
        decision = svc.should_retry(0, 200)
        assert decision.retry is False

    def test_no_retry_max_attempts(self):
        svc = RateLimitService(max_retries=3)
        decision = svc.should_retry(3, 429)
        assert decision.retry is False
        assert "max_retries" in decision.reason

    def test_retry_uses_retry_after(self):
        svc = RateLimitService()
        svc.process_headers({"retry-after": "5"}, 429)
        decision = svc.should_retry(0, 429)
        assert decision.retry is True
        assert decision.delay_ms == pytest.approx(5000.0)

    def test_last_info_stored(self):
        svc = RateLimitService()
        svc.process_headers({}, 200)
        assert svc.last_info is not None
