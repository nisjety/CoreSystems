"""Tests for the tools subsystem: base protocol, registry, and dispatcher."""

from __future__ import annotations

import pytest
from typing import Any

from app.tools.base import ToolDefinition, ToolResult
from app.tools.registry import ToolRegistry
from app.tools.dispatch import (
    ToolDispatcher,
    ToolBlockedError,
    ToolNotFoundError,
    ToolValidationError,
)


# ---------------------------------------------------------------------------
# Concrete tool implementations for testing
# ---------------------------------------------------------------------------

class EchoTool:
    """Minimal ToolDefinition-compliant tool that echoes input."""

    name = "echo"
    description = "Echoes the input back."
    input_schema: dict[str, Any] = {"type": "object", "properties": {"text": {"type": "string"}}}
    search_hint = "echo repeat parrot"
    should_defer = False

    def is_read_only(self) -> bool:
        return True

    def is_concurrent_safe(self) -> bool:
        return True

    def is_destructive(self) -> bool:
        return False

    def prompt(self) -> str:
        return "Use echo to repeat text."

    def validate_input(self, input_data: dict[str, Any]) -> dict[str, Any]:
        if "text" not in input_data:
            raise ValueError("Missing required field: text")
        return input_data

    async def call(self, input_data: dict[str, Any]) -> ToolResult:
        return ToolResult(output=input_data["text"])


class DeferredTool:
    """A deferred tool that becomes active when first accessed."""

    name = "deferred_search"
    description = "Search tool loaded on demand."
    input_schema: dict[str, Any] = {"type": "object"}
    search_hint = "search find locate"
    should_defer = True

    def is_read_only(self) -> bool:
        return True

    def is_concurrent_safe(self) -> bool:
        return True

    def is_destructive(self) -> bool:
        return False

    def prompt(self) -> str:
        return "Search for things."

    def validate_input(self, input_data: dict[str, Any]) -> dict[str, Any]:
        return input_data

    async def call(self, input_data: dict[str, Any]) -> ToolResult:
        return ToolResult(output="found")


class DestructiveTool:
    """A destructive, non-concurrent tool."""

    name = "delete_file"
    description = "Permanently deletes a file."
    input_schema: dict[str, Any] = {"type": "object", "properties": {"path": {"type": "string"}}}
    search_hint = "delete remove destroy"
    should_defer = False

    def is_read_only(self) -> bool:
        return False

    def is_concurrent_safe(self) -> bool:
        return False

    def is_destructive(self) -> bool:
        return True

    def prompt(self) -> str:
        return "Permanently removes files."

    def validate_input(self, input_data: dict[str, Any]) -> dict[str, Any]:
        if "path" not in input_data:
            raise ValueError("path is required")
        return input_data

    async def call(self, input_data: dict[str, Any]) -> ToolResult:
        return ToolResult(output=f"deleted {input_data['path']}")


class FailingTool:
    """Tool that raises during execution."""

    name = "crasher"
    description = "Always crashes."
    input_schema: dict[str, Any] = {}
    search_hint = "crash fail error"
    should_defer = False

    def is_read_only(self) -> bool:
        return True

    def is_concurrent_safe(self) -> bool:
        return False

    def is_destructive(self) -> bool:
        return False

    def prompt(self) -> str:
        return ""

    def validate_input(self, input_data: dict[str, Any]) -> dict[str, Any]:
        return input_data

    async def call(self, input_data: dict[str, Any]) -> ToolResult:
        raise RuntimeError("deliberate crash")


# ===========================================================================
# ToolResult tests
# ===========================================================================

class TestToolResult:
    def test_success_result(self):
        r = ToolResult(output="hello")
        assert r.success is True
        assert r.output == "hello"
        assert r.error is None

    def test_error_result(self):
        r = ToolResult(error="oops")
        assert r.success is False
        assert r.output is None

    def test_metadata_and_truncated(self):
        r = ToolResult(output="x", is_truncated=True, metadata={"bytes": 42})
        assert r.is_truncated is True
        assert r.metadata == {"bytes": 42}
        assert r.success is True


# ===========================================================================
# ToolDefinition protocol compliance
# ===========================================================================

class TestToolDefinitionProtocol:
    def test_echo_is_tool_definition(self):
        echo = EchoTool()
        assert isinstance(echo, ToolDefinition)

    def test_deferred_is_tool_definition(self):
        d = DeferredTool()
        assert isinstance(d, ToolDefinition)

    def test_destructive_distinguishes_flags(self):
        dt = DestructiveTool()
        assert isinstance(dt, ToolDefinition)
        assert dt.is_destructive() is True
        assert dt.is_read_only() is False
        assert dt.is_concurrent_safe() is False


# ===========================================================================
# ToolRegistry tests
# ===========================================================================

class TestToolRegistry:
    def setup_method(self):
        self.reg = ToolRegistry()

    def test_register_and_get(self):
        self.reg.register(EchoTool())
        assert self.reg.get("echo") is not None
        assert self.reg.get("echo").name == "echo"

    def test_get_missing_returns_none(self):
        assert self.reg.get("nonexistent") is None

    def test_deferred_routing(self):
        self.reg.register(DeferredTool())
        assert self.reg.count == 1  # deferred still counted
        assert self.reg.active_count == 0
        assert any(t.name == "deferred_search" for t in self.reg.list_deferred())

    def test_deferred_promotion_on_get(self):
        self.reg.register(DeferredTool())
        tool = self.reg.get("deferred_search")
        assert tool is not None
        assert self.reg.active_count == 1
        assert not any(t.name == "deferred_search" for t in self.reg.list_deferred())

    def test_alias(self):
        self.reg.register(EchoTool())
        self.reg.register_alias("parrot", "echo")
        assert self.reg.get("parrot").name == "echo"

    def test_list_all(self):
        self.reg.register(EchoTool())
        self.reg.register(DestructiveTool())
        names = self.reg.list_names()
        assert "echo" in names
        assert "delete_file" in names

    def test_clear(self):
        self.reg.register(EchoTool())
        self.reg.clear()
        assert self.reg.count == 0
        assert self.reg.get("echo") is None

    def test_search_exact_name(self):
        self.reg.register(EchoTool())
        self.reg.register(DestructiveTool())
        results = self.reg.search("echo")
        assert len(results) > 0
        assert results[0][0].name == "echo"

    def test_search_by_hint(self):
        self.reg.register(EchoTool())
        self.reg.register(DestructiveTool())
        results = self.reg.search("destroy")
        assert any(t.name == "delete_file" for t, _score in results)

    def test_search_limit(self):
        self.reg.register(EchoTool())
        self.reg.register(DestructiveTool())
        self.reg.register(FailingTool())
        results = self.reg.search("tool", limit=2)
        assert len(results) <= 2

    def test_search_includes_deferred(self):
        self.reg.register(DeferredTool())
        results = self.reg.search("search")
        assert any(t.name == "deferred_search" for t, _score in results)


# ===========================================================================
# ToolDispatcher tests
# ===========================================================================

class TestToolDispatcher:
    def setup_method(self):
        self.reg = ToolRegistry()
        self.reg.register(EchoTool())
        self.reg.register(DestructiveTool())
        self.reg.register(FailingTool())

    @pytest.mark.asyncio
    async def test_dispatch_success(self):
        disp = ToolDispatcher(self.reg)
        result = await disp.dispatch("echo", {"text": "hello"})
        assert result.success is True
        assert result.output == "hello"

    @pytest.mark.asyncio
    async def test_dispatch_not_found(self):
        disp = ToolDispatcher(self.reg)
        with pytest.raises(ToolNotFoundError) as exc_info:
            await disp.dispatch("nonexistent", {})
        assert exc_info.value.name == "nonexistent"

    @pytest.mark.asyncio
    async def test_dispatch_validation_failure(self):
        disp = ToolDispatcher(self.reg)
        with pytest.raises(ToolValidationError) as exc_info:
            await disp.dispatch("echo", {})  # missing "text"
        assert exc_info.value.name == "echo"

    @pytest.mark.asyncio
    async def test_dispatch_tool_crash_returns_error_result(self):
        disp = ToolDispatcher(self.reg)
        result = await disp.dispatch("crasher", {})
        assert result.success is False
        assert "deliberate crash" in result.error

    @pytest.mark.asyncio
    async def test_pre_hook_blocks(self):
        async def block_hook(name, input_data):
            return (False, input_data, "not allowed")

        disp = ToolDispatcher(self.reg, pre_hook_fn=block_hook)
        with pytest.raises(ToolBlockedError) as exc_info:
            await disp.dispatch("echo", {"text": "hi"})
        assert exc_info.value.name == "echo"
        assert "not allowed" in exc_info.value.reason

    @pytest.mark.asyncio
    async def test_pre_hook_modifies_input(self):
        async def modify_hook(name, input_data):
            modified = {**input_data, "text": input_data.get("text", "") + "!"}
            return (True, modified, None)

        disp = ToolDispatcher(self.reg, pre_hook_fn=modify_hook)
        result = await disp.dispatch("echo", {"text": "hi"})
        assert result.output == "hi!"

    @pytest.mark.asyncio
    async def test_post_hook_transforms_result(self):
        async def wrap_hook(name, input_data, result):
            return ToolResult(
                output=f"[wrapped] {result.output}",
                metadata={"hook": True},
            )

        disp = ToolDispatcher(self.reg, post_hook_fn=wrap_hook)
        result = await disp.dispatch("echo", {"text": "raw"})
        assert result.output == "[wrapped] raw"
        assert result.metadata == {"hook": True}

    @pytest.mark.asyncio
    async def test_dispatch_destructive_tool(self):
        disp = ToolDispatcher(self.reg)
        result = await disp.dispatch("delete_file", {"path": "/tmp/x"})
        assert result.success is True
        assert "deleted" in result.output


# ===========================================================================
# Integration: registry + dispatcher together
# ===========================================================================

class TestRegistryDispatcherIntegration:
    @pytest.mark.asyncio
    async def test_deferred_tool_dispatched_after_promotion(self):
        reg = ToolRegistry()
        reg.register(DeferredTool())
        disp = ToolDispatcher(reg)
        # deferred_search is in deferred pool — dispatch should promote and execute
        result = await disp.dispatch("deferred_search", {})
        assert result.success is True
        assert result.output == "found"
        assert reg.active_count == 1

    @pytest.mark.asyncio
    async def test_alias_dispatch(self):
        reg = ToolRegistry()
        reg.register(EchoTool())
        reg.register_alias("parrot", "echo")
        disp = ToolDispatcher(reg)
        result = await disp.dispatch("parrot", {"text": "squawk"})
        assert result.output == "squawk"
