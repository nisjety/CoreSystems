"""Tests for BashTool."""

from __future__ import annotations

import pytest
from app.tools.builtins.bash import BashTool


@pytest.fixture
def tool():
    return BashTool()


class TestBashToolValidation:
    def test_missing_command(self, tool):
        with pytest.raises(ValueError, match="command"):
            tool.validate_input({})

    def test_empty_command(self, tool):
        with pytest.raises(ValueError, match="blank"):
            tool.validate_input({"command": "   "})

    def test_valid_command(self, tool):
        result = tool.validate_input({"command": "echo hello"})
        assert result["command"] == "echo hello"


class TestBashToolExecution:
    @pytest.mark.asyncio
    async def test_echo(self, tool):
        result = await tool.call({"command": "echo hello"})
        assert result.success is True
        assert "hello" in result.output

    @pytest.mark.asyncio
    async def test_exit_code_nonzero(self, tool):
        result = await tool.call({"command": "exit 1"})
        assert result.error is not None
        assert result.metadata["exit_code"] == 1

    @pytest.mark.asyncio
    async def test_stderr_captured(self, tool):
        result = await tool.call({"command": "echo err >&2"})
        assert "err" in result.output

    @pytest.mark.asyncio
    async def test_timeout(self, tool):
        result = await tool.call({"command": "sleep 10", "timeout": 1})
        assert result.success is False
        assert "timed out" in result.error

    @pytest.mark.asyncio
    async def test_multiline_output(self, tool):
        result = await tool.call({"command": "printf 'a\\nb\\nc'"})
        assert result.success is True
        lines = result.output.strip().split("\n")
        assert len(lines) == 3


class TestBashToolProperties:
    def test_is_not_read_only(self, tool):
        assert tool.is_read_only() is False

    def test_is_destructive(self, tool):
        assert tool.is_destructive() is True

    def test_is_not_concurrent_safe(self, tool):
        assert tool.is_concurrent_safe() is False
