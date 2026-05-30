"""Tests for FileReadTool, FileEditTool, FileWriteTool."""

from __future__ import annotations

import pytest
from pathlib import Path
from app.tools.builtins.file_read import FileReadTool
from app.tools.builtins.file_edit import FileEditTool
from app.tools.builtins.file_write import FileWriteTool


@pytest.fixture
def read_tool():
    return FileReadTool()


@pytest.fixture
def edit_tool():
    return FileEditTool()


@pytest.fixture
def write_tool():
    return FileWriteTool()


# ===========================================================================
# FileReadTool tests
# ===========================================================================

class TestFileReadValidation:
    def test_missing_path(self, read_tool):
        with pytest.raises(ValueError, match="path"):
            read_tool.validate_input({})

    def test_invalid_line_range(self, read_tool):
        with pytest.raises(ValueError, match="start_line must be <= end_line"):
            read_tool.validate_input({"path": "x", "start_line": 10, "end_line": 5})

    def test_valid_input(self, read_tool):
        result = read_tool.validate_input({"path": "/tmp/test.txt", "start_line": 1, "end_line": 10})
        assert result["path"] == "/tmp/test.txt"


class TestFileReadExecution:
    @pytest.mark.asyncio
    async def test_read_existing_file(self, read_tool, tmp_path):
        f = tmp_path / "hello.txt"
        f.write_text("line1\nline2\nline3\n")
        result = await read_tool.call({"path": str(f)})
        assert result.success is True
        assert "line1" in result.output
        assert result.metadata["lines"] == 3

    @pytest.mark.asyncio
    async def test_read_with_line_range(self, read_tool, tmp_path):
        f = tmp_path / "lines.txt"
        f.write_text("a\nb\nc\nd\ne\n")
        result = await read_tool.call({"path": str(f), "start_line": 2, "end_line": 4})
        assert result.success
        assert "b\n" in result.output
        assert "a\n" not in result.output

    @pytest.mark.asyncio
    async def test_file_not_found(self, read_tool):
        result = await read_tool.call({"path": "/nonexistent/file.txt"})
        assert result.success is False
        assert "not found" in result.error.lower()

    @pytest.mark.asyncio
    async def test_directory_not_file(self, read_tool, tmp_path):
        result = await read_tool.call({"path": str(tmp_path)})
        assert result.success is False
        assert "Not a file" in result.error


# ===========================================================================
# FileEditTool tests
# ===========================================================================

class TestFileEditValidation:
    def test_missing_fields(self, edit_tool):
        with pytest.raises(ValueError):
            edit_tool.validate_input({"path": "x"})

    def test_empty_old_string(self, edit_tool):
        with pytest.raises(ValueError, match="old_string"):
            edit_tool.validate_input({"path": "x", "old_string": "", "new_string": "y"})


class TestFileEditExecution:
    @pytest.mark.asyncio
    async def test_replace_once(self, edit_tool, tmp_path):
        f = tmp_path / "edit.txt"
        f.write_text("hello world\n")
        result = await edit_tool.call({
            "path": str(f),
            "old_string": "hello",
            "new_string": "goodbye",
        })
        assert result.success is True
        assert f.read_text() == "goodbye world\n"

    @pytest.mark.asyncio
    async def test_old_string_not_found(self, edit_tool, tmp_path):
        f = tmp_path / "edit2.txt"
        f.write_text("abc\n")
        result = await edit_tool.call({
            "path": str(f),
            "old_string": "xyz",
            "new_string": "123",
        })
        assert result.success is False
        assert "not found" in result.error

    @pytest.mark.asyncio
    async def test_multiple_matches_rejected(self, edit_tool, tmp_path):
        f = tmp_path / "edit3.txt"
        f.write_text("foo\nfoo\n")
        result = await edit_tool.call({
            "path": str(f),
            "old_string": "foo",
            "new_string": "bar",
        })
        assert result.success is False
        assert "2 times" in result.error

    @pytest.mark.asyncio
    async def test_file_not_found(self, edit_tool):
        result = await edit_tool.call({
            "path": "/nonexistent/file.txt",
            "old_string": "a",
            "new_string": "b",
        })
        assert result.success is False


# ===========================================================================
# FileWriteTool tests
# ===========================================================================

class TestFileWriteValidation:
    def test_missing_path(self, write_tool):
        with pytest.raises(ValueError, match="path"):
            write_tool.validate_input({"content": "hello"})

    def test_missing_content(self, write_tool):
        with pytest.raises(ValueError, match="content"):
            write_tool.validate_input({"path": "/tmp/x"})


class TestFileWriteExecution:
    @pytest.mark.asyncio
    async def test_write_new_file(self, write_tool, tmp_path):
        f = tmp_path / "new.txt"
        result = await write_tool.call({"path": str(f), "content": "hello world"})
        assert result.success is True
        assert f.read_text() == "hello world"
        assert result.metadata["bytes"] == 11

    @pytest.mark.asyncio
    async def test_overwrite_existing(self, write_tool, tmp_path):
        f = tmp_path / "existing.txt"
        f.write_text("old content")
        result = await write_tool.call({"path": str(f), "content": "new content"})
        assert result.success is True
        assert f.read_text() == "new content"

    @pytest.mark.asyncio
    async def test_creates_directories(self, write_tool, tmp_path):
        f = tmp_path / "a" / "b" / "c" / "deep.txt"
        result = await write_tool.call({"path": str(f), "content": "deep"})
        assert result.success is True
        assert f.read_text() == "deep"


class TestFileToolProperties:
    def test_read_is_read_only(self, read_tool):
        assert read_tool.is_read_only() is True
        assert read_tool.is_concurrent_safe() is True

    def test_edit_is_not_read_only(self, edit_tool):
        assert edit_tool.is_read_only() is False
        assert edit_tool.is_destructive() is False

    def test_write_is_destructive(self, write_tool):
        assert write_tool.is_destructive() is True
