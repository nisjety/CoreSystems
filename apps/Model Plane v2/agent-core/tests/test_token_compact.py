"""Tests for Phase J — Token-aware compaction (tokenizer, auto_compact)."""

from __future__ import annotations

import pytest

from app.context.tokenizer import (
    CHARS_PER_TOKEN,
    MESSAGE_OVERHEAD,
    estimate_messages_tokens,
    estimate_tokens,
    fits_in_budget,
)
from app.context.auto_compact import (
    COMPACT_THRESHOLD,
    COMPACT_TARGET,
    _estimate_target_message_count,
    _format_messages_for_summary,
    auto_compact,
)


# ---------------------------------------------------------------------------
# Tokenizer
# ---------------------------------------------------------------------------


class TestEstimateTokens:
    def test_empty_string(self) -> None:
        assert estimate_tokens("") == 0

    def test_short_string(self) -> None:
        result = estimate_tokens("hello")
        assert result >= 1
        assert isinstance(result, int)

    def test_proportional_to_length(self) -> None:
        short = estimate_tokens("hello")
        long = estimate_tokens("hello " * 100)
        assert long > short

    def test_roughly_4_chars_per_token(self) -> None:
        # 400 chars → ~105 tokens (at 3.8 chars/token)
        text = "a" * 400
        result = estimate_tokens(text)
        assert 90 <= result <= 120


class TestEstimateMessagesTokens:
    def test_empty_list(self) -> None:
        # Just the priming overhead (2 tokens)
        result = estimate_messages_tokens([])
        assert result == 2

    def test_single_message(self) -> None:
        msgs = [{"role": "user", "content": "hello"}]
        result = estimate_messages_tokens(msgs)
        assert result > MESSAGE_OVERHEAD  # content + overhead + priming

    def test_multiple_messages(self) -> None:
        msgs = [
            {"role": "system", "content": "you are helpful"},
            {"role": "user", "content": "hi"},
        ]
        result = estimate_messages_tokens(msgs)
        single = estimate_messages_tokens([msgs[0]])
        assert result > single


class TestFitsInBudget:
    def test_small_messages_within_budget(self) -> None:
        msgs = [{"role": "user", "content": "hi"}]
        assert fits_in_budget(msgs, budget=1000) is True

    def test_large_messages_exceed_budget(self) -> None:
        msgs = [{"role": "user", "content": "x" * 10000}]
        assert fits_in_budget(msgs, budget=100) is False


# ---------------------------------------------------------------------------
# Auto-compact
# ---------------------------------------------------------------------------


class TestFormatMessagesForSummary:
    def test_formats_messages(self) -> None:
        msgs = [
            {"role": "user", "content": "hello"},
            {"role": "assistant", "content": "hi"},
        ]
        result = _format_messages_for_summary(msgs)
        assert "[user]: hello" in result
        assert "[assistant]: hi" in result

    def test_truncates_long_messages(self) -> None:
        msgs = [{"role": "user", "content": "x" * 3000}]
        result = _format_messages_for_summary(msgs)
        assert "[truncated]" in result
        assert len(result) < 3000


class TestEstimateTargetMessageCount:
    def test_empty(self) -> None:
        assert _estimate_target_message_count([], 1000) == 0

    def test_returns_reasonable_count(self) -> None:
        msgs = [{"role": "user", "content": "hello"} for _ in range(20)]
        count = _estimate_target_message_count(msgs, 500)
        assert count >= 6  # KEEP_HEAD + 4 minimum


@pytest.mark.asyncio
class TestAutoCompact:
    async def test_no_compaction_under_threshold(self) -> None:
        msgs = [{"role": "user", "content": "hi"}]
        result, was_compacted = await auto_compact(msgs, token_budget=100_000)
        assert was_compacted is False
        assert result is msgs

    async def test_snip_fallback_without_llm(self) -> None:
        # Create messages that exceed the threshold
        msgs = [{"role": "user", "content": "x" * 500} for _ in range(50)]
        # Small budget to trigger compaction
        result, was_compacted = await auto_compact(msgs, token_budget=200, llm_client=None)
        assert was_compacted is True
        assert len(result) < len(msgs)
