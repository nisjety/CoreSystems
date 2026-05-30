"""ToolDefinition protocol — the contract every tool must satisfy.

Mirrors CC's Tool<Input, Output> interface with ~15 key members:
  name, description, input_schema, is_read_only, is_concurrent_safe,
  prompt, validate_input, call, search_hint, should_defer, etc.

Tools implement this protocol as regular classes — no base class inheritance
required (structural subtyping via Protocol).
"""

from __future__ import annotations

from typing import Any, Protocol, runtime_checkable

from pydantic import BaseModel


class ToolResult(BaseModel):
    """Result returned from a tool execution."""

    output: Any = None
    error: str | None = None
    is_truncated: bool = False
    metadata: dict[str, Any] | None = None

    @property
    def success(self) -> bool:
        return self.error is None


@runtime_checkable
class ToolDefinition(Protocol):
    """The contract every tool must satisfy (CC Tool<I, O> pattern).

    Implement as a plain class — Python's structural typing handles the rest.
    """

    @property
    def name(self) -> str:
        """Unique tool name (e.g. 'bash', 'file_read', 'mcp__server__tool')."""
        ...

    @property
    def description(self) -> str:
        """Human-readable description shown to the LLM."""
        ...

    @property
    def input_schema(self) -> dict[str, Any]:
        """JSON Schema describing the tool's input (Pydantic .model_json_schema())."""
        ...

    @property
    def search_hint(self) -> str:
        """3-10 word keyword hint for tool search (CC pattern)."""
        ...

    def is_read_only(self) -> bool:
        """True if the tool never mutates state — eligible for parallel execution."""
        ...

    def is_concurrent_safe(self) -> bool:
        """True if safe to run concurrently with other concurrent-safe tools."""
        ...

    def is_destructive(self) -> bool:
        """True if the tool can cause irreversible changes (e.g. file_write, bash)."""
        ...

    @property
    def should_defer(self) -> bool:
        """True if the tool is not loaded until searched for (deferred tool pattern)."""
        ...

    def prompt(self) -> str:
        """Tool-specific prompt injected into system when tool is available."""
        ...

    def validate_input(self, input_data: dict[str, Any]) -> dict[str, Any]:
        """Validate and normalize input. Raise ValueError on invalid input."""
        ...

    async def call(self, input_data: dict[str, Any]) -> ToolResult:
        """Execute the tool with validated input. Returns ToolResult."""
        ...
