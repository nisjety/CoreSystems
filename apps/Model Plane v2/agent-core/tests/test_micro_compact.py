"""Tests for Phase B5: Compaction — micro-compact, grouping, image stripping."""

from __future__ import annotations

import pytest

from app.context.micro_compact import (
    COMPACTABLE_TOOLS,
    MicroCompactState,
    micro_compact_messages,
    strip_images_from_messages,
)
from app.context.grouping import (
    drop_oldest_groups,
    group_by_round,
    ptl_retry_truncate,
)


# ===========================================================================
# MicroCompactState
# ===========================================================================


class TestMicroCompactState:
    def test_initial_state(self):
        state = MicroCompactState()
        assert state.compacted_ids == set()
        assert state.total_tokens_saved == 0

    def test_mark_compacted(self):
        state = MicroCompactState()
        state.mark_compacted(5, 100)
        assert state.is_compacted(5)
        assert state.total_tokens_saved == 100

    def test_reset(self):
        state = MicroCompactState()
        state.mark_compacted(1, 50)
        state.reset()
        assert not state.is_compacted(1)
        assert state.total_tokens_saved == 0


# ===========================================================================
# micro_compact_messages
# ===========================================================================


class TestMicroCompact:
    def _make_tool_msg(self, tool: str, content_length: int = 2000) -> dict:
        return {
            "role": "tool",
            "name": tool,
            "content": "x" * content_length,
        }

    def test_compacts_large_file_read(self):
        msgs = [self._make_tool_msg("FileRead", 2000)]
        result, state = micro_compact_messages(msgs)
        assert len(result) == 1
        assert "[... FileRead result truncated" in result[0]["content"]
        assert state.total_tokens_saved > 0

    def test_skips_small_tool_result(self):
        msgs = [self._make_tool_msg("FileRead", 100)]
        result, state = micro_compact_messages(msgs)
        assert result[0]["content"] == "x" * 100
        assert state.total_tokens_saved == 0

    def test_skips_non_compactable_tools(self):
        msgs = [self._make_tool_msg("Bash", 2000)]
        result, _ = micro_compact_messages(msgs)
        assert result[0]["content"] == "x" * 2000

    def test_compactable_tools_set(self):
        assert "FileRead" in COMPACTABLE_TOOLS
        assert "Grep" in COMPACTABLE_TOOLS
        assert "Glob" in COMPACTABLE_TOOLS
        assert "Bash" not in COMPACTABLE_TOOLS

    def test_preserves_other_messages(self):
        msgs = [
            {"role": "user", "content": "hello"},
            self._make_tool_msg("FileRead", 2000),
            {"role": "assistant", "content": "done"},
        ]
        result, _ = micro_compact_messages(msgs)
        assert result[0]["content"] == "hello"
        assert result[2]["content"] == "done"
        assert "[... FileRead result truncated" in result[1]["content"]

    def test_idempotent_with_state(self):
        msgs = [self._make_tool_msg("FileRead", 2000)]
        result1, state = micro_compact_messages(msgs)
        # Run again with same state
        result2, state2 = micro_compact_messages(result1, state)
        assert result1[0]["content"] == result2[0]["content"]

    def test_multiple_tools(self):
        msgs = [
            self._make_tool_msg("FileRead", 2000),
            self._make_tool_msg("Grep", 1500),
            self._make_tool_msg("Bash", 3000),
        ]
        result, state = micro_compact_messages(msgs)
        assert "[... FileRead" in result[0]["content"]
        assert "[... Grep" in result[1]["content"]
        assert result[2]["content"] == "x" * 3000  # Bash not compacted


# ===========================================================================
# Image stripping
# ===========================================================================


class TestImageStripping:
    def test_strips_image_parts(self):
        msgs = [
            {
                "role": "user",
                "content": [
                    {"type": "text", "text": "Look at this"},
                    {"type": "image_url", "image_url": {"url": "data:image/png;base64,abc"}},
                ],
            }
        ]
        result = strip_images_from_messages(msgs)
        assert len(result) == 1
        assert len(result[0]["content"]) == 1
        assert result[0]["content"][0]["type"] == "text"

    def test_keeps_text_messages(self):
        msgs = [{"role": "user", "content": "hello"}]
        result = strip_images_from_messages(msgs)
        assert result[0]["content"] == "hello"

    def test_drops_image_only_message(self):
        msgs = [
            {
                "role": "user",
                "content": [
                    {"type": "image_url", "image_url": {"url": "data:image/png;base64,abc"}},
                ],
            }
        ]
        result = strip_images_from_messages(msgs)
        assert len(result) == 0


# ===========================================================================
# Grouping
# ===========================================================================


class TestGroupByRound:
    def test_basic_grouping(self):
        msgs = [
            {"role": "system", "content": "sys"},
            {"role": "user", "content": "hi"},
            {"role": "assistant", "content": "hello"},
            {"role": "tool", "content": "result"},
            {"role": "user", "content": "next"},
        ]
        groups = group_by_round(msgs)
        assert len(groups) >= 3

    def test_system_at_start(self):
        msgs = [
            {"role": "system", "content": "sys"},
            {"role": "user", "content": "q"},
        ]
        groups = group_by_round(msgs)
        # System should be in first group
        assert groups[0][0]["role"] == "system"


class TestDropOldestGroups:
    def test_drop_one_group(self):
        groups = [
            [{"role": "system", "content": "sys"}],
            [{"role": "user", "content": "q1"}],
            [{"role": "assistant", "content": "a1"}],
            [{"role": "user", "content": "q2"}],
        ]
        result = drop_oldest_groups(groups, keep_first=1, drop_count=1)
        # Should keep system + skip q1 + keep a1 + q2
        assert len(result) == 3

    def test_keep_first_protected(self):
        groups = [
            [{"role": "system", "content": "sys"}],
            [{"role": "user", "content": "q1"}],
        ]
        result = drop_oldest_groups(groups, keep_first=1, drop_count=1)
        # Only system kept
        assert len(result) == 1
        assert result[0]["role"] == "system"


class TestPTLRetry:
    def test_generates_attempts(self):
        msgs = [
            {"role": "system", "content": "sys"},
            {"role": "user", "content": "q1"},
            {"role": "assistant", "content": "a1"},
            {"role": "user", "content": "q2"},
            {"role": "assistant", "content": "a2"},
        ]
        attempts = ptl_retry_truncate(msgs, max_retries=2)
        assert len(attempts) >= 1
        # Each attempt should be shorter
        for i in range(1, len(attempts)):
            assert len(attempts[i]) <= len(attempts[i - 1])
