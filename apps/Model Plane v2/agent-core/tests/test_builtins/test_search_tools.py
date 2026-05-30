"""Tests for GrepTool and GlobTool."""

from __future__ import annotations

import pytest
from pathlib import Path
from app.tools.builtins.grep import GrepTool
from app.tools.builtins.glob import GlobTool


@pytest.fixture
def grep_tool():
    return GrepTool()


@pytest.fixture
def glob_tool():
    return GlobTool()


# ===========================================================================
# GrepTool tests
# ===========================================================================

class TestGrepValidation:
    def test_missing_pattern(self, grep_tool):
        with pytest.raises(ValueError, match="pattern"):
            grep_tool.validate_input({})


class TestGrepExecution:
    @pytest.mark.asyncio
    async def test_grep_finds_match(self, grep_tool, tmp_path):
        f = tmp_path / "code.py"
        f.write_text("def hello():\n    return 42\n")
        result = await grep_tool.call({"pattern": "hello", "path": str(tmp_path)})
        assert result.success is True
        assert "hello" in result.output

    @pytest.mark.asyncio
    async def test_grep_no_match(self, grep_tool, tmp_path):
        f = tmp_path / "empty.txt"
        f.write_text("nothing here\n")
        result = await grep_tool.call({"pattern": "zzzznotfound", "path": str(tmp_path)})
        assert "no matches" in result.output.lower()

    @pytest.mark.asyncio
    async def test_grep_with_include(self, grep_tool, tmp_path):
        py = tmp_path / "test.py"
        py.write_text("import os\n")
        txt = tmp_path / "test.txt"
        txt.write_text("import os\n")
        result = await grep_tool.call({
            "pattern": "import",
            "path": str(tmp_path),
            "include": "*.py",
        })
        assert result.success is True
        assert "test.py" in result.output


class TestGrepProperties:
    def test_is_read_only(self, grep_tool):
        assert grep_tool.is_read_only() is True
        assert grep_tool.is_concurrent_safe() is True


# ===========================================================================
# GlobTool tests
# ===========================================================================

class TestGlobValidation:
    def test_missing_pattern(self, glob_tool):
        with pytest.raises(ValueError, match="pattern"):
            glob_tool.validate_input({})


class TestGlobExecution:
    @pytest.mark.asyncio
    async def test_glob_finds_files(self, glob_tool, tmp_path):
        (tmp_path / "a.py").write_text("x")
        (tmp_path / "b.py").write_text("x")
        (tmp_path / "c.txt").write_text("x")
        result = await glob_tool.call({"pattern": "*.py", "path": str(tmp_path)})
        assert result.success is True
        assert "a.py" in result.output
        assert "b.py" in result.output
        assert "c.txt" not in result.output
        assert result.metadata["count"] == 2

    @pytest.mark.asyncio
    async def test_glob_recursive(self, glob_tool, tmp_path):
        sub = tmp_path / "sub"
        sub.mkdir()
        (sub / "deep.py").write_text("x")
        result = await glob_tool.call({"pattern": "**/*.py", "path": str(tmp_path)})
        assert "deep.py" in result.output

    @pytest.mark.asyncio
    async def test_glob_no_matches(self, glob_tool, tmp_path):
        result = await glob_tool.call({"pattern": "*.xyz", "path": str(tmp_path)})
        assert "no matching" in result.output.lower()

    @pytest.mark.asyncio
    async def test_glob_nonexistent_path(self, glob_tool):
        result = await glob_tool.call({"pattern": "*.py", "path": "/nonexistent/dir"})
        assert result.success is False


class TestGlobProperties:
    def test_is_read_only(self, glob_tool):
        assert glob_tool.is_read_only() is True
        assert glob_tool.is_concurrent_safe() is True
