"""Command base types — protocol for slash commands.

Two command types:
  - PromptCommand: injects text into the conversation (like /review)
  - LocalCommand: executes logic and returns output (like /cost)
"""

from __future__ import annotations

from enum import Enum
from typing import Any, Protocol, runtime_checkable

from pydantic import BaseModel, Field


class CommandType(str, Enum):
    PROMPT = "prompt"  # Injects a prompt into the conversation
    LOCAL = "local"  # Executes locally and returns output


class CommandResult(BaseModel):
    """Result of executing a command."""

    output: str = ""
    inject_prompt: str | None = None  # For PROMPT commands
    error: str | None = None
    metadata: dict[str, Any] = Field(default_factory=dict)

    @property
    def success(self) -> bool:
        return self.error is None


@runtime_checkable
class Command(Protocol):
    """Protocol for a slash command."""

    @property
    def name(self) -> str:
        """Command name without the slash (e.g. 'compact')."""
        ...

    @property
    def description(self) -> str:
        """Short description shown in /help."""
        ...

    @property
    def command_type(self) -> CommandType:
        ...

    @property
    def aliases(self) -> list[str]:
        """Alternate names (e.g. /c for /compact)."""
        ...

    @property
    def usage(self) -> str:
        """Usage string (e.g. '/compact [--hard]')."""
        ...

    @property
    def hidden(self) -> bool:
        """If True, not shown in /help."""
        ...

    async def execute(self, args: str, context: dict[str, Any]) -> CommandResult:
        """Execute the command with the given arguments and context."""
        ...
