"""Tests for Phase C3: Slash Commands System."""

from __future__ import annotations

from typing import Any

import pytest

from app.commands.base import Command, CommandResult, CommandType
from app.commands.registry import CommandRegistry
from app.commands.builtins.commands import (
    ALL_COMMANDS,
    CompactCommand,
    CostCommand,
    ExitCommand,
    HelpCommand,
    ReviewCommand,
    register_all_commands,
)


class TestCommandBase:
    def test_command_result_success(self):
        r = CommandResult(output="ok")
        assert r.success is True

    def test_command_result_error(self):
        r = CommandResult(error="bad")
        assert r.success is False


class TestCommandRegistry:
    def _make_registry(self) -> CommandRegistry:
        reg = CommandRegistry()
        register_all_commands(reg)
        return reg

    @pytest.mark.asyncio
    async def test_dispatch_known_command(self):
        reg = self._make_registry()
        result = await reg.dispatch("/compact")
        assert result is not None
        assert result.success

    @pytest.mark.asyncio
    async def test_dispatch_unknown_command(self):
        reg = self._make_registry()
        result = await reg.dispatch("/nonexistent")
        assert result is not None
        assert not result.success
        assert "Unknown command" in (result.error or "")

    @pytest.mark.asyncio
    async def test_dispatch_not_a_command(self):
        reg = self._make_registry()
        result = await reg.dispatch("hello world")
        assert result is None

    @pytest.mark.asyncio
    async def test_alias_works(self):
        reg = self._make_registry()
        result = await reg.dispatch("/c")  # alias for /compact
        assert result is not None
        assert result.success

    @pytest.mark.asyncio
    async def test_alias_quit(self):
        reg = self._make_registry()
        result = await reg.dispatch("/quit")
        assert result is not None
        assert result.metadata.get("action") == "exit"

    def test_get_by_name(self):
        reg = self._make_registry()
        cmd = reg.get("compact")
        assert cmd is not None
        assert cmd.name == "compact"

    def test_get_by_alias(self):
        reg = self._make_registry()
        cmd = reg.get("cr")
        assert cmd is not None
        assert cmd.name == "review"

    def test_get_unknown(self):
        reg = self._make_registry()
        assert reg.get("foobar") is None

    def test_list_all(self):
        reg = self._make_registry()
        cmds = reg.list_all()
        # ThinkCommand is hidden, should be excluded
        names = {c.name for c in cmds}
        assert "compact" in names
        assert "think" not in names

    def test_list_all_with_hidden(self):
        reg = self._make_registry()
        cmds = reg.list_all(include_hidden=True)
        names = {c.name for c in cmds}
        assert "think" in names

    def test_search(self):
        reg = self._make_registry()
        results = reg.search("review")
        assert any(c.name == "review" for c in results)

    def test_count(self):
        reg = self._make_registry()
        assert reg.count == len(ALL_COMMANDS)

    def test_is_command(self):
        reg = self._make_registry()
        assert reg.is_command("/compact") is True
        assert reg.is_command("/nonexistent") is False
        assert reg.is_command("hello") is False

    @pytest.mark.asyncio
    async def test_history_recorded(self):
        reg = self._make_registry()
        await reg.dispatch("/compact --hard")
        assert len(reg.history) == 1
        assert reg.history[0] == ("compact", "--hard")


class TestBuiltinCommands:
    @pytest.mark.asyncio
    async def test_compact_hard(self):
        cmd = CompactCommand()
        result = await cmd.execute("--hard", {})
        assert result.metadata["hard"] is True

    @pytest.mark.asyncio
    async def test_compact_soft(self):
        cmd = CompactCommand()
        result = await cmd.execute("", {})
        assert result.metadata["hard"] is False

    @pytest.mark.asyncio
    async def test_review_prompt(self):
        cmd = ReviewCommand()
        result = await cmd.execute("main.py", {})
        assert result.inject_prompt is not None
        assert "main.py" in result.inject_prompt

    @pytest.mark.asyncio
    async def test_cost_with_tracker(self):
        from app.cost_tracker import CostTracker, TurnUsage

        tracker = CostTracker(budget=100_000)
        tracker.record(TurnUsage(input_tokens=500, output_tokens=200))
        cmd = CostCommand()
        result = await cmd.execute("", {"cost_tracker": tracker})
        assert "700" in result.output

    @pytest.mark.asyncio
    async def test_help_with_registry(self):
        reg = CommandRegistry()
        register_all_commands(reg)
        cmd = reg.get("help")
        assert cmd is not None
        result = await cmd.execute("", {"registry": reg})
        assert "compact" in result.output.lower()

    @pytest.mark.asyncio
    async def test_help_specific_command(self):
        reg = CommandRegistry()
        register_all_commands(reg)
        cmd = reg.get("help")
        assert cmd is not None
        result = await cmd.execute("compact", {"registry": reg})
        assert "compact" in result.output.lower()

    @pytest.mark.asyncio
    async def test_all_commands_registered(self):
        assert len(ALL_COMMANDS) >= 30

    @pytest.mark.asyncio
    async def test_all_commands_have_names(self):
        for cls in ALL_COMMANDS:
            cmd = cls()
            assert cmd.name
            assert cmd.description
