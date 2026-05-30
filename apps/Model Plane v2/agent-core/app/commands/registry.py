"""Command registry — register, lookup, search slash commands."""

from __future__ import annotations

import logging
from typing import Any

from app.commands.base import Command, CommandResult

logger = logging.getLogger(__name__)


class CommandRegistry:
    """Registry for slash commands with alias support and search."""

    def __init__(self) -> None:
        self._commands: dict[str, Command] = {}
        self._aliases: dict[str, str] = {}  # alias → canonical name
        self._history: list[tuple[str, str]] = []  # (command_name, args)

    def register(self, command: Command) -> None:
        """Register a command."""
        name = command.name.lower()
        self._commands[name] = command
        for alias in command.aliases:
            self._aliases[alias.lower()] = name

    def get(self, name: str) -> Command | None:
        """Get a command by name or alias."""
        name = name.lower().lstrip("/")
        if name in self._commands:
            return self._commands[name]
        canonical = self._aliases.get(name)
        if canonical:
            return self._commands.get(canonical)
        return None

    async def dispatch(
        self, input_text: str, context: dict[str, Any] | None = None
    ) -> CommandResult | None:
        """Parse and dispatch a slash command.

        Returns None if the input is not a command.
        """
        if not input_text.startswith("/"):
            return None

        parts = input_text[1:].split(maxsplit=1)
        cmd_name = parts[0].lower()
        args = parts[1] if len(parts) > 1 else ""

        command = self.get(cmd_name)
        if command is None:
            return CommandResult(
                error=f"Unknown command: /{cmd_name}. Type /help for available commands."
            )

        self._history.append((cmd_name, args))
        try:
            return await command.execute(args, context or {})
        except Exception as exc:
            logger.error(
                "command_error",
                extra={"command": cmd_name, "error": str(exc)},
            )
            return CommandResult(error=f"Command /{cmd_name} failed: {exc}")

    def list_all(self, include_hidden: bool = False) -> list[Command]:
        """List all registered commands."""
        cmds = list(self._commands.values())
        if not include_hidden:
            cmds = [c for c in cmds if not c.hidden]
        return sorted(cmds, key=lambda c: c.name)

    def search(self, query: str) -> list[Command]:
        """Search commands by name or description."""
        q = query.lower()
        results: list[Command] = []
        for cmd in self._commands.values():
            if q in cmd.name.lower() or q in cmd.description.lower():
                results.append(cmd)
        return results

    @property
    def count(self) -> int:
        return len(self._commands)

    @property
    def history(self) -> list[tuple[str, str]]:
        return list(self._history)

    def is_command(self, text: str) -> bool:
        """Check if text starts with a valid command."""
        if not text.startswith("/"):
            return False
        parts = text[1:].split(maxsplit=1)
        return self.get(parts[0]) is not None
