"""Tests for WebSearchTool, WebFetchTool, AgentTool, ToolSearchTool."""

from __future__ import annotations

import pytest
from app.tools.builtins.web_search import WebSearchTool
from app.tools.builtins.web_fetch import WebFetchTool
from app.tools.builtins.agent import AgentTool
from app.tools.builtins.tool_search import ToolSearchTool
from app.tools.registry import ToolRegistry


# ===========================================================================
# WebSearchTool tests
# ===========================================================================

class TestWebSearchTool:
    @pytest.fixture
    def tool(self):
        return WebSearchTool()

    def test_validation_missing_query(self, tool):
        with pytest.raises(ValueError, match="query"):
            tool.validate_input({})

    @pytest.mark.asyncio
    async def test_stub_response(self, tool):
        result = await tool.call({"query": "test query"})
        assert result.success is True
        assert "stub" in result.output.lower()
        assert result.metadata["stub"] is True

    def test_is_deferred(self, tool):
        assert tool.should_defer is True
        assert tool.is_read_only() is True


# ===========================================================================
# WebFetchTool tests
# ===========================================================================

class TestWebFetchTool:
    @pytest.fixture
    def tool(self):
        return WebFetchTool()

    def test_validation_missing_url(self, tool):
        with pytest.raises(ValueError, match="url"):
            tool.validate_input({})

    def test_validation_bad_scheme(self, tool):
        with pytest.raises(ValueError, match="http"):
            tool.validate_input({"url": "ftp://example.com"})

    def test_validation_valid(self, tool):
        result = tool.validate_input({"url": "https://example.com"})
        assert result["url"] == "https://example.com"

    @pytest.mark.asyncio
    async def test_stub_response(self, tool):
        result = await tool.call({"url": "https://example.com"})
        assert result.success is True
        assert "stub" in result.output.lower()

    def test_is_deferred(self, tool):
        assert tool.should_defer is True


# ===========================================================================
# AgentTool tests
# ===========================================================================

class TestAgentTool:
    @pytest.fixture
    def tool(self):
        return AgentTool()

    def test_validation_missing_prompt(self, tool):
        with pytest.raises(ValueError, match="prompt"):
            tool.validate_input({})

    @pytest.mark.asyncio
    async def test_creates_task(self, tool, monkeypatch):
        """AgentTool should create a TaskRecord and return its ID."""
        from app.tools.builtins.agent import configure_agent_tool
        from app.tasks.domain import TaskRecord, TaskStatus
        import uuid

        configure_agent_tool(
            run_id="run-123",
            session_id="sess-456",
            org_id="org-789",
        )

        fake_task = TaskRecord(
            id="task-abc",
            run_id="run-123",
            session_id="sess-456",
            subject="Sub-agent: do something",
            status=TaskStatus.PENDING,
        )

        async def fake_create_task(task):
            return fake_task

        import app.tasks.repository as task_repo
        monkeypatch.setattr(task_repo, "create_task", fake_create_task)

        result = await tool.call({"prompt": "do something", "agent_name": "planner"})
        assert result.success is True
        assert "task-abc" in result.output
        assert result.metadata["task_id"] == "task-abc"
        assert result.metadata["agent_name"] == "planner"
        assert result.metadata["status"] == "pending"

    @pytest.mark.asyncio
    async def test_db_failure_returns_error(self, tool, monkeypatch):
        """When task creation fails, AgentTool returns an error ToolResult."""
        from app.tools.builtins.agent import configure_agent_tool

        configure_agent_tool(run_id="r", session_id="s")

        async def failing_create(task):
            raise RuntimeError("DB unavailable")

        import app.tasks.repository as task_repo
        monkeypatch.setattr(task_repo, "create_task", failing_create)

        result = await tool.call({"prompt": "do something"})
        assert result.success is False
        assert "Failed to create sub-agent task" in result.error

    @pytest.mark.asyncio
    async def test_default_agent_name(self, tool, monkeypatch):
        from app.tasks.domain import TaskRecord, TaskStatus
        from app.tools.builtins.agent import configure_agent_tool

        configure_agent_tool(run_id="r", session_id="s")

        async def fake_create(task):
            return TaskRecord(
                id="tid",
                run_id="r",
                session_id="s",
                subject=task.subject,
                status=TaskStatus.PENDING,
            )

        import app.tasks.repository as task_repo
        monkeypatch.setattr(task_repo, "create_task", fake_create)

        result = await tool.call({"prompt": "task"})
        assert result.metadata["agent_name"] == "default"

    def test_properties(self, tool):
        assert tool.is_read_only() is False
        assert tool.is_concurrent_safe() is False
        assert tool.is_destructive() is False


# ===========================================================================
# ToolSearchTool tests
# ===========================================================================

class TestToolSearchTool:
    @pytest.fixture
    def tool(self):
        t = ToolSearchTool()
        reg = ToolRegistry()
        # Register some tools for searching
        from app.tools.builtins.bash import BashTool
        from app.tools.builtins.file_read import FileReadTool
        reg.register(BashTool())
        reg.register(FileReadTool())
        t._registry = reg
        return t

    def test_validation_missing_query(self):
        t = ToolSearchTool()
        with pytest.raises(ValueError, match="query"):
            t.validate_input({})

    @pytest.mark.asyncio
    async def test_search_finds_tools(self, tool):
        result = await tool.call({"query": "bash"})
        assert result.success is True
        assert "bash" in result.output
        assert result.metadata["count"] >= 1

    @pytest.mark.asyncio
    async def test_search_no_results(self, tool):
        result = await tool.call({"query": "zzzznonexistent"})
        assert result.success is True
        assert "No tools found" in result.output

    @pytest.mark.asyncio
    async def test_no_registry_configured(self):
        t = ToolSearchTool()
        t._registry = None
        result = await t.call({"query": "bash"})
        assert result.success is False
        assert "registry" in result.error.lower()


# ===========================================================================
# register_builtins integration test
# ===========================================================================

class TestRegisterBuiltins:
    def test_register_all_builtins(self):
        from app.tools.builtins import register_builtins
        reg = ToolRegistry()
        register_builtins(reg)
        assert reg.count == 22
        # Non-deferred should be active immediately
        assert reg.active_count >= 20  # web_search and web_fetch are deferred
        assert reg.get("bash") is not None
        assert reg.get("file_read") is not None
        assert reg.get("file_edit") is not None
        assert reg.get("file_write") is not None
        assert reg.get("grep") is not None
        assert reg.get("glob") is not None
        assert reg.get("agent") is not None
        assert reg.get("tool_search") is not None
