"""Tests for auto_compact — Phase R: Extended Compaction."""

from __future__ import annotations

import pytest

from app.context.auto_compact import (
    COMPACT_THRESHOLD,
    MICRO_BATCH_SIZE,
    _SUMMARY_KEY_PREFIX,
    _SUMMARY_TTL,
    _extract_summary_text,
    _format_messages_for_summary,
    auto_compact,
    micro_compact,
)


# ────────────────────────────────────────────────────────────────────────────
# Helpers
# ────────────────────────────────────────────────────────────────────────────


def _make_messages(count: int) -> list[dict]:
    """Create a message list with alternating user/assistant messages."""
    msgs = [{"role": "system", "content": "You are a helpful assistant."}]
    for i in range(count):
        role = "user" if i % 2 == 0 else "assistant"
        msgs.append({"role": role, "content": f"Message {i} " + ("word " * 50)})
    return msgs


class _FakeLLM:
    """Minimal LLM stub that returns a fixed summary string."""

    def __init__(self, summary: str = "Summarized content"):
        self.summary = summary
        self.called = 0

    async def planner_complete(self, messages: list[dict]) -> str:
        self.called += 1
        return self.summary


class _FailingLLM:
    """LLM that always raises an exception."""

    async def planner_complete(self, messages: list[dict]) -> str:
        raise RuntimeError("LLM unavailable")


class _FakeRedis:
    """In-memory Redis stub."""

    def __init__(self) -> None:
        self._store: dict[str, str] = {}
        self._ttls: dict[str, int] = {}

    async def get(self, key: str):
        return self._store.get(key)

    async def setex(self, key: str, ttl: int, value: str) -> None:
        self._store[key] = value
        self._ttls[key] = ttl

    def has_key(self, key: str) -> bool:
        return key in self._store


# ────────────────────────────────────────────────────────────────────────────
# auto_compact — threshold guard
# ────────────────────────────────────────────────────────────────────────────


class TestAutoCompactThreshold:
    @pytest.mark.asyncio
    async def test_no_compaction_below_threshold(self):
        # Short history well below 90% budget — should be a no-op
        messages = _make_messages(3)
        result, compacted = await auto_compact(messages, token_budget=100_000)
        assert not compacted
        assert result is messages  # same object returned

    @pytest.mark.asyncio
    async def test_returns_was_compacted_false_when_under_budget(self):
        messages = [{"role": "user", "content": "hi"}]
        _, was_compacted = await auto_compact(messages, token_budget=100_000)
        assert was_compacted is False


# ────────────────────────────────────────────────────────────────────────────
# auto_compact — SNIP fallback (no LLM client)
# ────────────────────────────────────────────────────────────────────────────


class TestAutoCompactSnipFallback:
    @pytest.mark.asyncio
    async def test_snip_fallback_when_no_llm(self):
        messages = _make_messages(100)
        result, was_compacted = await auto_compact(
            messages, token_budget=100, llm_client=None
        )
        assert was_compacted is True
        assert len(result) < len(messages)

    @pytest.mark.asyncio
    async def test_snip_fallback_preserves_head(self):
        messages = _make_messages(100)
        result, _ = await auto_compact(messages, token_budget=100, llm_client=None)
        # System message must be preserved
        assert result[0]["role"] == "system"


# ────────────────────────────────────────────────────────────────────────────
# auto_compact — LLM strategy
# ────────────────────────────────────────────────────────────────────────────


class TestAutoCompactLLM:
    @pytest.mark.asyncio
    async def test_llm_called_when_over_budget(self):
        llm = _FakeLLM("Summary of old work")
        messages = _make_messages(100)
        result, was_compacted = await auto_compact(
            messages, token_budget=100, llm_client=llm
        )
        assert was_compacted is True
        assert llm.called >= 1

    @pytest.mark.asyncio
    async def test_summary_message_injected(self):
        llm = _FakeLLM("key insight A, key insight B")
        messages = _make_messages(50)
        result, _ = await auto_compact(messages, token_budget=100, llm_client=llm)
        # There should be a system summary message
        summary_msgs = [
            m for m in result if "[COMPACT SUMMARY:" in m.get("content", "")
        ]
        assert len(summary_msgs) == 1

    @pytest.mark.asyncio
    async def test_summary_content_from_llm(self):
        expected = "LLM-produced summary text"
        llm = _FakeLLM(expected)
        messages = _make_messages(50)
        result, _ = await auto_compact(messages, token_budget=100, llm_client=llm)
        full_content = " ".join(m.get("content", "") for m in result)
        assert expected in full_content

    @pytest.mark.asyncio
    async def test_head_preserved_after_llm_compact(self):
        llm = _FakeLLM("summary")
        messages = _make_messages(50)
        result, _ = await auto_compact(messages, token_budget=100, llm_client=llm)
        assert result[0]["role"] == "system"

    @pytest.mark.asyncio
    async def test_llm_failure_falls_back_to_snip(self):
        llm = _FailingLLM()
        messages = _make_messages(100)
        result, was_compacted = await auto_compact(
            messages, token_budget=100, llm_client=llm
        )
        assert was_compacted is True
        assert len(result) < len(messages)

    @pytest.mark.asyncio
    async def test_few_messages_not_over_compacted(self):
        # Message list too small to have meaningful middle — should return unchanged
        llm = _FakeLLM("summary")
        messages = _make_messages(3)  # below KEEP_HEAD + 4
        result, _ = await auto_compact(messages, token_budget=1, llm_client=llm)
        assert len(result) == len(messages)


# ────────────────────────────────────────────────────────────────────────────
# auto_compact — Redis-backed summary caching
# ────────────────────────────────────────────────────────────────────────────


class TestAutoCompactRedisCache:
    @pytest.mark.asyncio
    async def test_summary_cached_in_redis(self):
        llm = _FakeLLM("cached summary value")
        redis = _FakeRedis()
        messages = _make_messages(50)
        await auto_compact(
            messages,
            token_budget=100,
            llm_client=llm,
            run_id="run-abc",
            redis=redis,
        )
        key = f"{_SUMMARY_KEY_PREFIX}run-abc"
        assert redis.has_key(key)
        assert redis._ttls[key] == _SUMMARY_TTL

    @pytest.mark.asyncio
    async def test_prior_summary_forwarded_to_llm(self):
        llm = _FakeLLM("new summary")
        redis = _FakeRedis()
        # Pre-seed a prior summary
        await redis.setex(f"{_SUMMARY_KEY_PREFIX}run-xyz", 3600, "prior context")

        messages = _make_messages(50)
        called_inputs: list[str] = []

        original = llm.planner_complete

        async def capture(msgs):
            user_content = next(
                (m["content"] for m in msgs if m["role"] == "user"), ""
            )
            called_inputs.append(user_content)
            return await original(msgs)

        llm.planner_complete = capture
        await auto_compact(
            messages,
            token_budget=100,
            llm_client=llm,
            run_id="run-xyz",
            redis=redis,
        )
        assert any("prior context" in s for s in called_inputs)

    @pytest.mark.asyncio
    async def test_no_crash_when_redis_fails(self):
        """Redis errors should be swallowed and compaction still succeeds."""

        class BrokenRedis:
            async def get(self, key):
                raise ConnectionError("Redis down")

            async def setex(self, key, ttl, value):
                raise ConnectionError("Redis down")

        llm = _FakeLLM("summary")
        messages = _make_messages(50)
        result, was_compacted = await auto_compact(
            messages,
            token_budget=100,
            llm_client=llm,
            run_id="run-fail",
            redis=BrokenRedis(),
        )
        assert was_compacted is True


# ────────────────────────────────────────────────────────────────────────────
# micro_compact — incremental strategy
# ────────────────────────────────────────────────────────────────────────────


class TestMicroCompact:
    @pytest.mark.asyncio
    async def test_no_compaction_below_threshold(self):
        messages = _make_messages(3)
        result, was_compacted = await micro_compact(
            messages, token_budget=100_000, llm_client=_FakeLLM()
        )
        assert not was_compacted
        assert result is messages

    @pytest.mark.asyncio
    async def test_micro_batch_summarized(self):
        llm = _FakeLLM("micro summary text")
        messages = _make_messages(50)
        result, was_compacted = await micro_compact(
            messages, token_budget=100, llm_client=llm, batch_size=5
        )
        assert was_compacted is True
        # One MICRO SUMMARY message should appear
        micro_msgs = [
            m for m in result if "[MICRO SUMMARY:" in m.get("content", "")
        ]
        assert len(micro_msgs) == 1

    @pytest.mark.asyncio
    async def test_micro_summary_cached_in_redis(self):
        llm = _FakeLLM("micro summary cached")
        redis = _FakeRedis()
        messages = _make_messages(50)
        await micro_compact(
            messages,
            token_budget=100,
            llm_client=llm,
            run_id="run-micro",
            redis=redis,
        )
        key = f"{_SUMMARY_KEY_PREFIX}run-micro"
        assert redis.has_key(key)

    @pytest.mark.asyncio
    async def test_micro_failure_falls_back_to_snip(self):
        messages = _make_messages(50)
        result, was_compacted = await micro_compact(
            messages, token_budget=100, llm_client=_FailingLLM()
        )
        assert was_compacted is True
        assert len(result) < len(messages)

    @pytest.mark.asyncio
    async def test_micro_head_preserved(self):
        llm = _FakeLLM("micro summary")
        messages = _make_messages(50)
        result, _ = await micro_compact(
            messages, token_budget=100, llm_client=llm, batch_size=5
        )
        assert result[0]["role"] == "system"


# ────────────────────────────────────────────────────────────────────────────
# Utility helpers
# ────────────────────────────────────────────────────────────────────────────


class TestExtractSummaryText:
    def test_extracts_from_compact_summary(self):
        msgs = [
            {"role": "system", "content": "[COMPACT SUMMARY: 5 messages summarized]\n\nThe key points were X and Y."}
        ]
        result = _extract_summary_text(msgs)
        assert result == "The key points were X and Y."

    def test_extracts_from_micro_summary(self):
        msgs = [
            {"role": "system", "content": "[MICRO SUMMARY: 10 messages]\n\nTool calls: foo, bar."}
        ]
        result = _extract_summary_text(msgs)
        assert result == "Tool calls: foo, bar."

    def test_returns_none_when_no_summary(self):
        msgs = [{"role": "user", "content": "hello"}]
        assert _extract_summary_text(msgs) is None

    def test_picks_last_summary_message(self):
        msgs = [
            {"role": "system", "content": "[COMPACT SUMMARY: 3 messages summarized]\n\nFirst summary."},
            {"role": "user", "content": "continue"},
            {"role": "system", "content": "[MICRO SUMMARY: 5 messages]\n\nSecond summary."},
        ]
        result = _extract_summary_text(msgs)
        assert result == "Second summary."


class TestFormatMessagesForSummary:
    def test_basic_format(self):
        msgs = [
            {"role": "user", "content": "Hello"},
            {"role": "assistant", "content": "Hi there"},
        ]
        result = _format_messages_for_summary(msgs)
        assert "[user]: Hello" in result
        assert "[assistant]: Hi there" in result

    def test_long_content_truncated(self):
        long_content = "x" * 3000
        msgs = [{"role": "user", "content": long_content}]
        result = _format_messages_for_summary(msgs)
        assert "... [truncated]" in result
        assert len(result) < len(long_content) + 100

    def test_empty_messages(self):
        assert _format_messages_for_summary([]) == ""
