"""Tests for query lineage propagation on agent runs."""

from __future__ import annotations

from unittest.mock import AsyncMock

import pytest

from app.agent_service import AgentService
from app.domain import AgentType, CreateRunRequest, ExecutionPolicy, RunMode, RunRecord
from app.hooks.domain import HookType, LifecycleHookResult
from app.nats_publisher import EventPublisher


class DummyNatsManager:
    def __init__(self) -> None:
        self.calls: list[tuple[str, dict, bool]] = []

    async def publish_jetstream(self, subject: str, payload: dict, local: bool = False) -> None:
        self.calls.append((subject, payload, local))


@pytest.mark.asyncio
async def test_create_run_sets_root_query_depth(monkeypatch: pytest.MonkeyPatch) -> None:
    create_run_mock = AsyncMock()
    lifecycle_mock = AsyncMock(return_value=LifecycleHookResult(proceed=True))
    monkeypatch.setattr("app.agent_service.repo.create_run", create_run_mock)
    monkeypatch.setattr("app.agent_service.run_lifecycle_hooks", lifecycle_mock)

    service = AgentService(
        capability=AsyncMock(),
        llm=AsyncMock(),
        documents=AsyncMock(),
        publisher=AsyncMock(),
    )

    request = CreateRunRequest(
        goal="root task",
        mode=RunMode.EXECUTE,
        agent_type=AgentType.GENERAL,
        policy=ExecutionPolicy(),
        context={"source": "api"},
    )

    run = await service.create_run(request, "session-1", "user-1", "org-1")

    assert run.metadata["query_depth"] == 0
    assert run.metadata["source"] == "api"
    create_run_mock.assert_awaited_once()
    lifecycle_mock.assert_awaited_once()


@pytest.mark.asyncio
async def test_create_run_increments_query_depth_from_parent(monkeypatch: pytest.MonkeyPatch) -> None:
    create_run_mock = AsyncMock()
    lifecycle_mock = AsyncMock(return_value=LifecycleHookResult(proceed=True))
    get_run_mock = AsyncMock(
        return_value=RunRecord(
            id="parent-run",
            session_id="session-1",
            user_id="user-1",
            org_id="org-1",
            goal="parent task",
            metadata={"query_depth": 2},
        )
    )
    monkeypatch.setattr("app.agent_service.repo.create_run", create_run_mock)
    monkeypatch.setattr("app.agent_service.repo.get_run", get_run_mock)
    monkeypatch.setattr("app.agent_service.run_lifecycle_hooks", lifecycle_mock)

    service = AgentService(
        capability=AsyncMock(),
        llm=AsyncMock(),
        documents=AsyncMock(),
        publisher=AsyncMock(),
    )

    request = CreateRunRequest(
        goal="child task",
        parent_run_id="parent-run",
        context={"fork_reason": "delegate"},
    )

    run = await service.create_run(request, "session-1", "user-1", "org-1")

    assert run.metadata["query_depth"] == 3
    assert run.metadata["fork_reason"] == "delegate"
    get_run_mock.assert_awaited_once_with("parent-run")
    create_run_mock.assert_awaited_once()
    lifecycle_mock.assert_awaited_once()


@pytest.mark.asyncio
async def test_event_publisher_includes_query_depth() -> None:
    nats_mgr = DummyNatsManager()
    publisher = EventPublisher(nats_mgr)
    run = RunRecord(
        id="run-1",
        session_id="session-1",
        user_id="user-1",
        goal="do work",
        metadata={"query_depth": 1},
    )

    await publisher.run_started(run)

    subject, payload, local = nats_mgr.calls[0]
    assert subject == "velion.agent.run.run-1.event"
    assert local is False
    assert payload["payload"]["query_depth"] == 1


@pytest.mark.asyncio
async def test_create_run_blocks_on_session_start_hook(monkeypatch: pytest.MonkeyPatch) -> None:
    create_run_mock = AsyncMock()
    lifecycle_mock = AsyncMock(
        return_value=LifecycleHookResult(
            proceed=False,
            reason="session start blocked by policy",
        )
    )
    monkeypatch.setattr("app.agent_service.repo.create_run", create_run_mock)
    monkeypatch.setattr("app.agent_service.run_lifecycle_hooks", lifecycle_mock)

    service = AgentService(
        capability=AsyncMock(),
        llm=AsyncMock(),
        documents=AsyncMock(),
        publisher=AsyncMock(),
    )

    request = CreateRunRequest(goal="blocked task")

    with pytest.raises(PermissionError, match="session start blocked by policy"):
        await service.create_run(request, "session-1", "user-1", "org-1")

    lifecycle_mock.assert_awaited_once_with(
        "org-1",
        HookType.SESSION_START,
        "session:start",
        {
            "goal": "blocked task",
            "mode": RunMode.EXECUTE.value,
            "agent_type": AgentType.GENERAL.value,
            "context": {},
            "allowed_tools": [],
            "parent_run_id": None,
            "session_id": "session-1",
            "user_id": "user-1",
        },
    )
    create_run_mock.assert_not_awaited()


@pytest.mark.asyncio
async def test_create_run_applies_session_start_hook_modifications(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    create_run_mock = AsyncMock()
    lifecycle_mock = AsyncMock(
        return_value=LifecycleHookResult(
            proceed=True,
            modified_payload={
                "goal": "rewritten by hook",
                "context": {"source": "hook"},
                "allowed_tools": ["search"],
            },
        )
    )
    monkeypatch.setattr("app.agent_service.repo.create_run", create_run_mock)
    monkeypatch.setattr("app.agent_service.run_lifecycle_hooks", lifecycle_mock)

    service = AgentService(
        capability=AsyncMock(),
        llm=AsyncMock(),
        documents=AsyncMock(),
        publisher=AsyncMock(),
    )

    request = CreateRunRequest(goal="original goal", context={"source": "api"})

    run = await service.create_run(request, "session-1", "user-1", "org-1")

    assert run.goal == "rewritten by hook"
    assert run.metadata["source"] == "hook"
    assert run.policy.allowed_tools == ["search"]
    assert run.metadata["query_depth"] == 0
    create_run_mock.assert_awaited_once()


@pytest.mark.asyncio
async def test_create_run_rejects_invalid_session_start_payload(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    create_run_mock = AsyncMock()
    lifecycle_mock = AsyncMock(
        return_value=LifecycleHookResult(
            proceed=True,
            modified_payload={"goal": "   "},
        )
    )
    monkeypatch.setattr("app.agent_service.repo.create_run", create_run_mock)
    monkeypatch.setattr("app.agent_service.run_lifecycle_hooks", lifecycle_mock)

    service = AgentService(
        capability=AsyncMock(),
        llm=AsyncMock(),
        documents=AsyncMock(),
        publisher=AsyncMock(),
    )

    request = CreateRunRequest(goal="original goal")

    with pytest.raises(PermissionError, match="invalid session start payload"):
        await service.create_run(request, "session-1", "user-1", "org-1")

    create_run_mock.assert_not_awaited()