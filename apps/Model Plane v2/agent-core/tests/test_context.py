"""Tests for Phase F — Context injection (memory, injector, compact)."""

from __future__ import annotations

import pytest

from app.context.compact import SNIP_MARKER, compact_history
from app.context.injector import (
    _hash_snippets,
    compose_system_prompt,
    invalidate_prompt_cache,
)


# ---------------------------------------------------------------------------
# compose_system_prompt
# ---------------------------------------------------------------------------


class TestComposeSystemPrompt:
    def setup_method(self) -> None:
        invalidate_prompt_cache()

    def test_no_snippets_returns_base(self) -> None:
        result = compose_system_prompt("base prompt", [])
        assert result == "base prompt"

    def test_injects_memory_block(self) -> None:
        result = compose_system_prompt("base prompt", ["## key1\ncontent1"])
        assert "<memory>" in result
        assert "## key1" in result
        assert "content1" in result
        assert result.endswith("base prompt")

    def test_multiple_snippets(self) -> None:
        snippets = ["## a\nfoo", "## b\nbar"]
        result = compose_system_prompt("base", snippets)
        assert "## a" in result
        assert "## b" in result

    def test_caching(self) -> None:
        """Same inputs should return cached result."""
        snippets = ["## x\ny"]
        r1 = compose_system_prompt("base", snippets, session_id="s1")
        r2 = compose_system_prompt("base", snippets, session_id="s1")
        assert r1 is r2  # Same object from cache

    def test_different_session_not_cached(self) -> None:
        snippets = ["## x\ny"]
        r1 = compose_system_prompt("base", snippets, session_id="s1")
        r2 = compose_system_prompt("base", snippets, session_id="s2")
        assert r1 == r2  # Same content
        assert r1 is not r2  # Different cache entries


class TestHashSnippets:
    def test_deterministic(self) -> None:
        h1 = _hash_snippets(["a", "b"])
        h2 = _hash_snippets(["a", "b"])
        assert h1 == h2

    def test_different_input_different_hash(self) -> None:
        h1 = _hash_snippets(["a", "b"])
        h2 = _hash_snippets(["a", "c"])
        assert h1 != h2


# ---------------------------------------------------------------------------
# compact_history
# ---------------------------------------------------------------------------


class TestCompactHistory:
    def test_no_compaction_under_limit(self) -> None:
        msgs = [{"role": "user", "content": f"msg{i}"} for i in range(10)]
        result = compact_history(msgs, max_messages=30)
        assert result == msgs

    def test_compaction_over_limit(self) -> None:
        msgs = [{"role": "user", "content": f"msg{i}"} for i in range(50)]
        result = compact_history(msgs, max_messages=20, keep_tail=8)
        # Should have: 2 head + 1 snip + 8 tail = 11
        assert len(result) == 11
        assert result[0]["content"] == "msg0"
        assert result[1]["content"] == "msg1"
        assert "SNIP" in result[2]["content"]
        assert result[-1]["content"] == "msg49"

    def test_snip_count_is_correct(self) -> None:
        msgs = [{"role": "user", "content": f"m{i}"} for i in range(30)]
        result = compact_history(msgs, max_messages=15, keep_tail=5)
        snip = result[2]
        # 30 - 2 (head) - 5 (tail) = 23 removed
        assert "23 messages removed" in snip["content"]

    def test_exact_boundary(self) -> None:
        msgs = [{"role": "user", "content": f"m{i}"} for i in range(30)]
        result = compact_history(msgs, max_messages=30)
        assert result == msgs  # No compaction needed

    def test_small_list(self) -> None:
        msgs = [{"role": "user", "content": "only"}]
        result = compact_history(msgs, max_messages=5)
        assert result == msgs
