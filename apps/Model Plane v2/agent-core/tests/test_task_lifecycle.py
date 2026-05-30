"""Tests for Phase B2: Tasks — Full CC Task Lifecycle."""

from __future__ import annotations

import pytest

from app.tasks.domain import (
    ExecutedTask,
    LocalAgentTaskState,
    RemoteAgentTaskState,
    TaskNotification,
    TaskProgress,
    TaskRecord,
    TaskStatus,
    TaskType,
    is_terminal,
)
from app.tasks.lifecycle import TaskLimitError, TaskManager


# ===========================================================================
# Domain model tests
# ===========================================================================

class TestTaskTypeExtended:
    def test_new_types_exist(self):
        assert TaskType.REMOTE_AGENT == "remote_agent"
        assert TaskType.IN_PROCESS_TEAMMATE == "in_process"
        assert TaskType.WORKFLOW == "workflow"
        assert TaskType.MONITOR_MCP == "monitor_mcp"
        assert TaskType.DREAM == "dream"

    def test_original_types_preserved(self):
        assert TaskType.GENERAL == "general"
        assert TaskType.LOCAL_BASH == "local_bash"
        assert TaskType.SUB_AGENT == "sub_agent"


class TestAgentTaskState:
    def test_local_state_defaults(self):
        state = LocalAgentTaskState(agent_id="a1", task_id="t1")
        assert state.progress.tool_use_count == 0
        assert state.is_backgrounded is False
        assert state.pending_messages == []

    def test_remote_state_defaults(self):
        state = RemoteAgentTaskState(session_id="s1", task_id="t1")
        assert state.todo_list == []
        assert state.log == []
        assert state.poll_started_at is None

    def test_task_notification(self):
        n = TaskNotification(task_id="t1", event="spawned", summary="hi")
        assert n.task_id == "t1"
        assert n.event == "spawned"
        assert n.timestamp is not None


# ===========================================================================
# TaskManager tests
# ===========================================================================

class TestTaskManagerSpawn:
    def setup_method(self):
        self.mgr = TaskManager(max_tasks=5)

    def test_spawn_creates_task(self):
        task = self.mgr.spawn("r1", "s1", "Do stuff")
        assert task.subject == "Do stuff"
        assert task.status == TaskStatus.PENDING
        assert self.mgr.get(task.id) is not None

    def test_spawn_emits_notification(self):
        task = self.mgr.spawn("r1", "s1", "Work")
        notifs = self.mgr.drain_notifications()
        assert len(notifs) == 1
        assert notifs[0].event == "spawned"
        assert notifs[0].task_id == task.id

    def test_spawn_respects_limit(self):
        for i in range(5):
            self.mgr.spawn("r1", "s1", f"task-{i}")
        with pytest.raises(TaskLimitError):
            self.mgr.spawn("r1", "s1", "overflow")

    def test_spawn_after_evict_allows_new(self):
        tasks = [self.mgr.spawn("r1", "s1", f"t-{i}") for i in range(5)]
        self.mgr.update_status(tasks[0].id, TaskStatus.COMPLETED)
        self.mgr.evict(tasks[0].id)
        # Should succeed now
        new = self.mgr.spawn("r1", "s1", "new task")
        assert new is not None


class TestTaskManagerState:
    def setup_method(self):
        self.mgr = TaskManager()

    def test_register_local_state(self):
        task = self.mgr.spawn("r1", "s1", "Work")
        state = self.mgr.register_local(task.id, "agent-1")
        assert state.agent_id == "agent-1"
        assert self.mgr.get_local_state(task.id) is not None

    def test_register_remote_state(self):
        task = self.mgr.spawn("r1", "s1", "Remote")
        state = self.mgr.register_remote(task.id, "session-xyz")
        assert state.session_id == "session-xyz"
        assert self.mgr.get_remote_state(task.id) is not None

    def test_update_progress(self):
        task = self.mgr.spawn("r1", "s1", "Progress")
        self.mgr.register_local(task.id, "a1")
        self.mgr.update_progress(task.id, tool_use_delta=3, token_delta=100, activity="read file")
        state = self.mgr.get_local_state(task.id)
        assert state.progress.tool_use_count == 3
        assert state.progress.token_count == 100
        assert "read file" in state.progress.recent_activities

    def test_update_progress_cumulative(self):
        task = self.mgr.spawn("r1", "s1", "Accumulate")
        self.mgr.register_local(task.id, "a1")
        self.mgr.update_progress(task.id, tool_use_delta=1, token_delta=50)
        self.mgr.update_progress(task.id, tool_use_delta=2, token_delta=30)
        state = self.mgr.get_local_state(task.id)
        assert state.progress.tool_use_count == 3
        assert state.progress.token_count == 80


class TestTaskManagerStatusUpdate:
    def setup_method(self):
        self.mgr = TaskManager()

    def test_update_to_completed(self):
        task = self.mgr.spawn("r1", "s1", "Finish")
        result = self.mgr.update_status(task.id, TaskStatus.COMPLETED, output="done")
        assert result.status == TaskStatus.COMPLETED
        assert result.output == "done"

    def test_update_to_failed(self):
        task = self.mgr.spawn("r1", "s1", "Fail")
        result = self.mgr.update_status(task.id, TaskStatus.FAILED, error="boom")
        assert result.status == TaskStatus.FAILED
        assert result.error == "boom"

    def test_update_emits_notification(self):
        task = self.mgr.spawn("r1", "s1", "Notify")
        self.mgr.drain_notifications()  # clear spawn notification
        self.mgr.update_status(task.id, TaskStatus.COMPLETED)
        notifs = self.mgr.drain_notifications()
        assert len(notifs) == 1
        assert notifs[0].event == "completed"

    def test_update_nonexistent_returns_none(self):
        assert self.mgr.update_status("fake", TaskStatus.COMPLETED) is None


class TestTaskManagerKillEvict:
    def setup_method(self):
        self.mgr = TaskManager()

    def test_kill_running_task(self):
        task = self.mgr.spawn("r1", "s1", "Kill me")
        self.mgr.register_local(task.id, "a1")
        result = self.mgr.kill(task.id, "timeout")
        assert result is True
        assert self.mgr.get(task.id).status == TaskStatus.KILLED
        assert self.mgr.get_local_state(task.id) is None  # cleaned up

    def test_kill_terminal_returns_false(self):
        task = self.mgr.spawn("r1", "s1", "Already done")
        self.mgr.update_status(task.id, TaskStatus.COMPLETED)
        assert self.mgr.kill(task.id) is False

    def test_evict_terminal_removes(self):
        task = self.mgr.spawn("r1", "s1", "Evict me")
        self.mgr.update_status(task.id, TaskStatus.COMPLETED)
        assert self.mgr.evict(task.id) is True
        assert self.mgr.get(task.id) is None

    def test_evict_active_fails(self):
        task = self.mgr.spawn("r1", "s1", "Still active")
        assert self.mgr.evict(task.id) is False

    def test_list_active(self):
        t1 = self.mgr.spawn("r1", "s1", "Active 1")
        t2 = self.mgr.spawn("r1", "s1", "Active 2")
        self.mgr.update_status(t1.id, TaskStatus.COMPLETED)
        active = self.mgr.list_active()
        assert len(active) == 1
        assert active[0].id == t2.id


class TestIsTerminal:
    def test_terminal_statuses(self):
        assert is_terminal(TaskStatus.COMPLETED) is True
        assert is_terminal(TaskStatus.FAILED) is True
        assert is_terminal(TaskStatus.KILLED) is True
        assert is_terminal(TaskStatus.DELETED) is True

    def test_non_terminal_statuses(self):
        assert is_terminal(TaskStatus.PENDING) is False
        assert is_terminal(TaskStatus.IN_PROGRESS) is False
