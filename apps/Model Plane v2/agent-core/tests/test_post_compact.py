"""Tests for Phase B5: Post-compact cleanup and file restoration."""

from __future__ import annotations

import pytest

from app.context.micro_compact import MicroCompactState
from app.context.post_compact import (
    CompactBoundaryMarker,
    build_file_restoration_messages,
    extract_referenced_files,
    post_compact_cleanup,
)


class TestPostCompactCleanup:
    def test_resets_micro_compact_state(self):
        state = MicroCompactState()
        state.mark_compacted(0, 100)
        post_compact_cleanup(state)
        assert not state.is_compacted(0)
        assert state.total_tokens_saved == 0

    def test_handles_none(self):
        post_compact_cleanup(None)  # Should not raise


class TestExtractReferencedFiles:
    def test_extracts_paths(self):
        msgs = [
            {"role": "assistant", "content": "I read `src/main.py` and `tests/test_foo.py`"},
            {"role": "tool", "content": "File: src/main.py\nresult here"},
        ]
        paths = extract_referenced_files(msgs)
        assert any("src/main.py" in p for p in paths)

    def test_empty_messages(self):
        assert extract_referenced_files([]) == []

    def test_deduplicates(self):
        msgs = [
            {"role": "user", "content": "check src/app.py"},
            {"role": "user", "content": "also src/app.py"},
        ]
        paths = extract_referenced_files(msgs)
        assert paths.count("src/app.py") == 1


class TestFileRestoration:
    def test_build_with_reader(self):
        def reader(path: str) -> str:
            return f"content of {path}"

        msgs = build_file_restoration_messages(
            ["src/main.py", "src/utils.py"],
            file_reader=reader,
        )
        assert len(msgs) == 2
        assert msgs[0]["role"] == "system"
        assert "main.py" in msgs[0]["content"]

    def test_build_without_reader(self):
        msgs = build_file_restoration_messages(["src/main.py"])
        assert len(msgs) == 0  # No reader → no content

    def test_respects_budget(self):
        def reader(path: str) -> str:
            return "x" * 200_000  # Large file

        msgs = build_file_restoration_messages(
            [f"file{i}.py" for i in range(20)],
            file_reader=reader,
        )
        # Should stop before restoring all files
        assert len(msgs) < 20

    def test_skips_on_reader_error(self):
        def reader(path: str) -> str:
            if "bad" in path:
                raise FileNotFoundError
            return "ok"

        msgs = build_file_restoration_messages(
            ["good.py", "bad.py"],
            file_reader=reader,
        )
        assert len(msgs) == 1


class TestCompactBoundaryMarker:
    def test_create_marker(self):
        marker = CompactBoundaryMarker.create(
            turn_index=5, tokens_before=10000, tokens_after=4000
        )
        assert marker["role"] == "system"
        assert "COMPACT BOUNDARY" in marker["content"]
        assert marker["metadata"]["turn_index"] == 5
        assert marker["metadata"]["tokens_before"] == 10000
        assert marker["metadata"]["tokens_after"] == 4000
