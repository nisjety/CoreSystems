"""Tool result cache — within-session deduplication of tool calls.

Mirrors CC's ToolUseContext caching: if the LLM calls the same tool
with the same input within a session, return the cached result instead
of re-executing.

Cache key: (tool_name, hash(serialized_input))
TTL: configurable, default 5 minutes.

The cache is per-run (lives for the duration of a single run_turn_loop).
"""

from __future__ import annotations

import hashlib
import json
import logging
import time
from dataclasses import dataclass, field
from typing import Any

logger = logging.getLogger(__name__)

DEFAULT_CACHE_TTL: float = 300.0  # 5 minutes


@dataclass(frozen=True)
class CacheEntry:
    """A cached tool result with timestamp."""

    output: Any
    cached_at: float


@dataclass
class ToolResultCache:
    """In-memory cache for tool call results within a run.

    Usage:
        cache = ToolResultCache(ttl=300.0)
        hit = cache.get("read_file", {"path": "/foo"})
        if hit is not None:
            return hit
        result = await execute_tool(...)
        cache.put("read_file", {"path": "/foo"}, result)
    """

    ttl: float = DEFAULT_CACHE_TTL
    _store: dict[str, CacheEntry] = field(default_factory=dict)
    _hits: int = 0
    _misses: int = 0

    def _make_key(self, tool_name: str, tool_input: dict[str, Any]) -> str:
        """Create a deterministic cache key from tool name + input."""
        serialized = json.dumps(tool_input, sort_keys=True, default=str)
        input_hash = hashlib.sha256(serialized.encode()).hexdigest()[:16]
        return f"{tool_name}:{input_hash}"

    def get(self, tool_name: str, tool_input: dict[str, Any]) -> Any | None:
        """Look up a cached result. Returns None on miss or expiry."""
        key = self._make_key(tool_name, tool_input)
        entry = self._store.get(key)
        if entry is None:
            self._misses += 1
            return None

        age = time.time() - entry.cached_at
        if age > self.ttl:
            # Expired
            del self._store[key]
            self._misses += 1
            return None

        self._hits += 1
        logger.debug(
            "tool_cache_hit",
            extra={"tool": tool_name, "key": key[:32], "age": f"{age:.1f}s"},
        )
        return entry.output

    def put(
        self, tool_name: str, tool_input: dict[str, Any], output: Any
    ) -> None:
        """Store a tool result in the cache."""
        key = self._make_key(tool_name, tool_input)
        self._store[key] = CacheEntry(output=output, cached_at=time.time())

    def contains(self, tool_name: str, tool_input: dict[str, Any]) -> bool:
        """Check if a non-expired entry exists (without counting as hit/miss)."""
        key = self._make_key(tool_name, tool_input)
        entry = self._store.get(key)
        if entry is None:
            return False
        return (time.time() - entry.cached_at) <= self.ttl

    @property
    def size(self) -> int:
        return len(self._store)

    @property
    def hit_rate(self) -> float:
        total = self._hits + self._misses
        if total == 0:
            return 0.0
        return self._hits / total

    def stats(self) -> dict[str, int | float]:
        """Return cache statistics."""
        return {
            "entries": self.size,
            "hits": self._hits,
            "misses": self._misses,
            "hit_rate": round(self.hit_rate, 4),
        }

    def clear(self) -> None:
        """Clear all cached entries."""
        self._store.clear()
