"""Tests for Phase C2: Prompt Cache Optimization."""

from __future__ import annotations

import os
from unittest.mock import patch

import pytest

from app.prompt_cache import (
    CacheBreakEvent,
    CacheControl,
    CacheMetrics,
    PromptCacheManager,
)


class TestCacheControl:
    def test_defaults(self):
        cc = CacheControl()
        assert cc.type == "ephemeral"
        assert cc.ttl is None


class TestCacheMetrics:
    def test_hit_rate_empty(self):
        m = CacheMetrics()
        assert m.hit_rate == 0.0

    def test_hit_rate_calculation(self):
        m = CacheMetrics(cache_hits=3, cache_misses=1)
        assert m.hit_rate == pytest.approx(0.75)


class TestPromptCacheManager:
    def test_enabled_by_default(self):
        mgr = PromptCacheManager()
        assert mgr.enabled is True

    def test_disabled_explicitly(self):
        mgr = PromptCacheManager(enabled=False)
        assert mgr.enabled is False

    @patch.dict(os.environ, {"DISABLE_PROMPT_CACHING": "1"})
    def test_disabled_via_env(self):
        mgr = PromptCacheManager()
        assert mgr.enabled is False

    def test_inject_system_message(self):
        mgr = PromptCacheManager()
        msgs = [{"role": "system", "content": "You are helpful"}]
        result = mgr.inject_cache_control(msgs, "claude-sonnet-4-20250514")
        assert result[0]["cache_control"] == {"type": "ephemeral"}

    def test_inject_preserves_existing(self):
        mgr = PromptCacheManager()
        msgs = [
            {
                "role": "system",
                "content": "x",
                "cache_control": {"type": "permanent"},
            }
        ]
        result = mgr.inject_cache_control(msgs, "claude-sonnet-4-20250514")
        assert result[0]["cache_control"]["type"] == "permanent"

    def test_inject_skips_user_messages(self):
        mgr = PromptCacheManager()
        msgs = [{"role": "user", "content": "hello"}]
        result = mgr.inject_cache_control(msgs, "claude-sonnet-4-20250514")
        assert "cache_control" not in result[0]

    def test_inject_does_not_mutate_original(self):
        mgr = PromptCacheManager()
        msgs = [{"role": "system", "content": "x"}]
        result = mgr.inject_cache_control(msgs, "claude-sonnet-4-20250514")
        assert "cache_control" not in msgs[0]
        assert "cache_control" in result[0]

    def test_inject_tools(self):
        mgr = PromptCacheManager()
        tools = [
            {"name": "tool1", "description": "First"},
            {"name": "tool2", "description": "Last"},
        ]
        result = mgr.inject_tool_cache_control(tools, "claude-sonnet-4-20250514")
        assert "cache_control" not in result[0]
        assert result[1]["cache_control"] == {"type": "ephemeral"}

    def test_inject_tools_empty(self):
        mgr = PromptCacheManager()
        assert mgr.inject_tool_cache_control([], "claude-sonnet-4-20250514") == []

    @patch.dict(os.environ, {"DISABLE_PROMPT_CACHING_CLAUDE_HAIKU": "1"})
    def test_disabled_for_specific_model(self):
        mgr = PromptCacheManager()
        assert mgr.is_enabled_for_model("claude-haiku") is False
        assert mgr.is_enabled_for_model("claude-sonnet") is True

    def test_disabled_model_skips_inject(self):
        mgr = PromptCacheManager(enabled=False)
        msgs = [{"role": "system", "content": "x"}]
        result = mgr.inject_cache_control(msgs, "claude-sonnet-4-20250514")
        assert "cache_control" not in result[0]


class TestCacheBreakDetection:
    def test_no_break_first_turn(self):
        mgr = PromptCacheManager()
        event = mgr.record_usage(1000, 200)
        assert event is None

    def test_no_break_stable_cache(self):
        mgr = PromptCacheManager()
        mgr.record_usage(1000, 200)
        event = mgr.record_usage(900, 0)
        assert event is None  # 90% of previous, within threshold

    def test_break_detected_on_drop(self):
        mgr = PromptCacheManager()
        mgr.record_usage(1000, 200)
        event = mgr.record_usage(100, 0)  # 90% drop
        assert event is not None
        assert event.drop_ratio == pytest.approx(0.9)

    def test_break_detected_on_zero(self):
        mgr = PromptCacheManager()
        mgr.record_usage(1000, 200)
        event = mgr.record_usage(0, 500)  # Complete loss
        assert event is not None
        assert event.drop_ratio == pytest.approx(1.0)

    def test_metrics_updated(self):
        mgr = PromptCacheManager()
        mgr.record_usage(1000, 200)
        mgr.record_usage(500, 0)
        m = mgr.metrics
        assert m.total_cache_read_tokens == 1500
        assert m.total_cache_creation_tokens == 200
        assert m.cache_hits == 2

    def test_miss_counted(self):
        mgr = PromptCacheManager()
        mgr.record_usage(0, 500)
        assert mgr.metrics.cache_misses == 1

    def test_breaks_list(self):
        mgr = PromptCacheManager()
        mgr.record_usage(1000, 0)
        mgr.record_usage(0, 500)
        assert len(mgr.breaks) == 1
        assert mgr.metrics.cache_breaks_detected == 1
