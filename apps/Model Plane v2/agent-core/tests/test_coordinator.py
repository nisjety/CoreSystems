"""Tests for Phase D1: Coordinator Mode (Team/Swarm Orchestration)."""

from __future__ import annotations

import pytest

from app.coordinator.mode import CoordinatorMode
from app.coordinator.prompt import (
    COORDINATOR_ALLOWED_TOOLS,
    COORDINATOR_SYSTEM_PROMPT,
)
from app.coordinator.workers import WorkerManager, WorkerState, WorkerTask


# ── Prompt & Config ────────────────────────────────────────────

class TestCoordinatorPrompt:
    def test_system_prompt_exists(self):
        assert len(COORDINATOR_SYSTEM_PROMPT) > 200

    def test_allowed_tools_set(self):
        assert "agent" in COORDINATOR_ALLOWED_TOOLS
        assert "bash" not in COORDINATOR_ALLOWED_TOOLS
        assert "file_edit" not in COORDINATOR_ALLOWED_TOOLS


# ── WorkerManager ──────────────────────────────────────────────

class TestWorkerManager:
    def test_create_task(self):
        wm = WorkerManager()
        task = wm.create_task("fix bug in auth")
        assert task.state == WorkerState.PENDING
        assert task.objective == "fix bug in auth"
        assert task.worker_id

    def test_start_task(self):
        wm = WorkerManager()
        task = wm.create_task("do work")
        started = wm.start(task.worker_id)
        assert started.state == WorkerState.RUNNING

    def test_complete_task(self):
        wm = WorkerManager()
        task = wm.create_task("do work")
        wm.start(task.worker_id)
        done = wm.complete(task.worker_id, summary="done", artifacts=["a.py"])
        assert done.state == WorkerState.COMPLETED
        assert done.result_summary == "done"
        assert done.completed_at is not None

    def test_fail_task(self):
        wm = WorkerManager()
        task = wm.create_task("risky work")
        wm.start(task.worker_id)
        failed = wm.fail(task.worker_id, "timeout")
        assert failed.state == WorkerState.FAILED
        assert failed.error == "timeout"

    def test_retry_task(self):
        wm = WorkerManager()
        task = wm.create_task("retry me")
        wm.start(task.worker_id)
        wm.fail(task.worker_id, "oops")
        retried = wm.retry(task.worker_id)
        assert retried is not None
        assert retried.state == WorkerState.PENDING
        assert retried.retry_count == 1

    def test_retry_exceeds_max(self):
        wm = WorkerManager()
        task = wm.create_task("fail forever")
        wm.start(task.worker_id)
        wm.fail(task.worker_id, "err")
        wm.retry(task.worker_id)
        wm.start(task.worker_id)
        wm.fail(task.worker_id, "err2")
        assert wm.retry(task.worker_id) is None  # MAX_RETRIES=1

    def test_list_all_with_filter(self):
        wm = WorkerManager()
        wm.create_task("a")
        t2 = wm.create_task("b")
        wm.start(t2.worker_id)
        assert len(wm.list_all()) == 2
        assert len(wm.list_all(state=WorkerState.RUNNING)) == 1

    def test_all_done(self):
        wm = WorkerManager()
        t1 = wm.create_task("a")
        t2 = wm.create_task("b")
        wm.start(t1.worker_id)
        wm.complete(t1.worker_id)
        assert not wm.all_done
        wm.start(t2.worker_id)
        wm.fail(t2.worker_id, "err")
        assert wm.all_done

    def test_counts(self):
        wm = WorkerManager()
        t1 = wm.create_task("a")
        t2 = wm.create_task("b")
        assert wm.pending_count == 2
        wm.start(t1.worker_id)
        wm.complete(t1.worker_id)
        assert wm.completed_count == 1
        wm.start(t2.worker_id)
        wm.fail(t2.worker_id, "x")
        assert wm.failed_count == 1

    def test_summary_xml(self):
        wm = WorkerManager()
        t = wm.create_task("test xml")
        xml = wm.summary_xml()
        assert "<worker_summary>" in xml
        assert t.worker_id in xml

    def test_unknown_worker_raises(self):
        wm = WorkerManager()
        with pytest.raises(KeyError):
            wm.start("nonexistent")


# ── CoordinatorMode ────────────────────────────────────────────

class TestCoordinatorMode:
    def test_enter_exit(self):
        cm = CoordinatorMode()
        assert not cm.active
        prompt = cm.enter()
        assert cm.active
        assert len(prompt) > 0
        cm.exit()
        assert not cm.active

    def test_enter_idempotent(self):
        cm = CoordinatorMode()
        p1 = cm.enter()
        p2 = cm.enter()
        assert p1 != ""
        assert p2 == ""  # Already active

    def test_filter_tools_active(self):
        cm = CoordinatorMode()
        cm.enter()
        tools = cm.filter_tools(["agent", "bash", "file_edit", "grep"])
        assert "agent" in tools
        assert "grep" in tools
        assert "bash" not in tools
        assert "file_edit" not in tools

    def test_filter_tools_inactive(self):
        cm = CoordinatorMode()
        tools = cm.filter_tools(["agent", "bash", "file_edit"])
        assert tools == ["agent", "bash", "file_edit"]

    def test_inject_system_prompt(self):
        cm = CoordinatorMode()
        cm.enter()
        result = cm.inject_system_prompt("You are an AI assistant.")
        assert "coordinator mode" in result.lower()
        assert "You are an AI assistant." in result

    def test_inject_prompt_inactive(self):
        cm = CoordinatorMode()
        base = "You are an AI assistant."
        assert cm.inject_system_prompt(base) == base

    def test_workers_reset_on_enter(self):
        cm = CoordinatorMode()
        cm.enter()
        cm.workers.create_task("a")
        assert cm.workers.pending_count == 1
        cm.exit()
        cm.enter()
        assert cm.workers.pending_count == 0
