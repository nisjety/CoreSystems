"""Tests for Phase T+U — Tool Result Cache & Dedup."""

from __future__ import annotations

import time

import pytest

from app.tool_cache import ToolResultCache


# ---------------------------------------------------------------------------
# ToolResultCache
# ---------------------------------------------------------------------------


class TestToolResultCache:
    def test_put_and_get(self) -> None:
        cache = ToolResultCache()
        cache.put("read_file", {"path": "/a.py"}, "file content")
        result = cache.get("read_file", {"path": "/a.py"})
        assert result == "file content"

    def test_miss(self) -> None:
        cache = ToolResultCache()
        assert cache.get("read_file", {"path": "/a.py"}) is None

    def test_different_input_is_miss(self) -> None:
        cache = ToolResultCache()
        cache.put("read_file", {"path": "/a.py"}, "content A")
        assert cache.get("read_file", {"path": "/b.py"}) is None

    def test_different_tool_is_miss(self) -> None:
        cache = ToolResultCache()
        cache.put("read_file", {"path": "/a.py"}, "content")
        assert cache.get("write_file", {"path": "/a.py"}) is None

    def test_contains(self) -> None:
        cache = ToolResultCache()
        cache.put("read_file", {"path": "/a.py"}, "content")
        assert cache.contains("read_file", {"path": "/a.py"})
        assert not cache.contains("read_file", {"path": "/b.py"})

    def test_ttl_expiry(self) -> None:
        cache = ToolResultCache(ttl=0.01)  # 10ms TTL
        cache.put("tool", {"x": 1}, "value")
        assert cache.get("tool", {"x": 1}) == "value"
        time.sleep(0.02)
        assert cache.get("tool", {"x": 1}) is None

    def test_clear(self) -> None:
        cache = ToolResultCache()
        cache.put("a", {}, "1")
        cache.put("b", {}, "2")
        cache.clear()
        assert cache.get("a", {}) is None
        assert cache.get("b", {}) is None

    def test_stats(self) -> None:
        cache = ToolResultCache()
        cache.put("read_file", {"p": 1}, "v")
        cache.get("read_file", {"p": 1})  # hit
        cache.get("read_file", {"p": 2})  # miss
        cache.get("read_file", {"p": 1})  # hit

        stats = cache.stats()
        assert stats["hits"] == 2
        assert stats["misses"] == 1
        assert stats["entries"] == 1

    def test_hit_rate(self) -> None:
        cache = ToolResultCache()
        cache.put("t", {}, "v")
        cache.get("t", {})  # hit
        cache.get("t", {})  # hit
        cache.get("t2", {})  # miss
        assert cache.hit_rate == pytest.approx(2 / 3)

    def test_hit_rate_zero_lookups(self) -> None:
        cache = ToolResultCache()
        assert cache.hit_rate == 0.0

    def test_input_key_order_independence(self) -> None:
        """Different dict key ordering should produce same cache key."""
        cache = ToolResultCache()
        cache.put("tool", {"b": 2, "a": 1}, "value")
        result = cache.get("tool", {"a": 1, "b": 2})
        assert result == "value"

    def test_empty_input(self) -> None:
        cache = ToolResultCache()
        cache.put("tool", {}, "value")
        assert cache.get("tool", {}) == "value"
