"""Tests for analytics — Phase T: Analytics Events."""

from __future__ import annotations

import asyncio
import json
from datetime import datetime, timezone
from typing import Any

import pytest

from app.analytics.domain import AnalyticsEvent, AnalyticsEventType
from app.analytics.publisher import AnalyticsPublisher


# ────────────────────────────────────────────────────────────────────────────
# Stubs
# ────────────────────────────────────────────────────────────────────────────


class _FakeJS:
    """Fake NATS JetStream client."""

    def __init__(self) -> None:
        self.published: list[tuple[str, bytes]] = []

    async def publish(self, subject: str, payload: bytes) -> None:
        self.published.append((subject, payload))


class _FakeConn:
    """Fake asyncpg connection."""

    def __init__(self) -> None:
        self.executed: list[tuple] = []

    async def execute(self, sql: str, *args) -> None:
        self.executed.append((sql, args))


class _FakePool:
    def __init__(self, conn: _FakeConn) -> None:
        self._conn = conn

    def acquire(self):
        return _AsyncContextManager(self._conn)


class _AsyncContextManager:
    def __init__(self, value: Any) -> None:
        self._value = value

    async def __aenter__(self):
        return self._value

    async def __aexit__(self, *args):
        pass


# ────────────────────────────────────────────────────────────────────────────
# AnalyticsEvent domain
# ────────────────────────────────────────────────────────────────────────────


class TestAnalyticsEventDomain:
    def test_event_id_auto_generated(self):
        evt = AnalyticsEvent(event_type=AnalyticsEventType.RUN_STARTED, org_id="org1")
        assert evt.event_id
        assert len(evt.event_id) == 36  # UUID format

    def test_ts_is_utc(self):
        evt = AnalyticsEvent(event_type=AnalyticsEventType.RUN_STARTED, org_id="org1")
        assert evt.ts.tzinfo is not None

    def test_nats_subject(self):
        evt = AnalyticsEvent(event_type=AnalyticsEventType.RUN_STARTED, org_id="my-org")
        assert evt.nats_subject() == "analytics.events.my-org"

    def test_to_json_bytes_produces_valid_json(self):
        evt = AnalyticsEvent(
            event_type=AnalyticsEventType.ACTION_EXECUTED,
            org_id="org1",
            run_id="run-1",
            props={"tool": "bash", "exit_code": 0},
        )
        data = json.loads(evt.to_json_bytes())
        assert data["event_type"] == "action.executed"
        assert data["org_id"] == "org1"
        assert data["props"]["tool"] == "bash"

    def test_event_is_immutable(self):
        evt = AnalyticsEvent(event_type=AnalyticsEventType.RUN_STARTED, org_id="org1")
        with pytest.raises(Exception):
            evt.org_id = "other"  # type: ignore[misc]

    def test_all_event_types_have_dotted_values(self):
        for et in AnalyticsEventType:
            assert "." in et.value

    def test_props_default_empty(self):
        evt = AnalyticsEvent(event_type=AnalyticsEventType.RUN_STARTED, org_id="org1")
        assert evt.props == {}


# ────────────────────────────────────────────────────────────────────────────
# AnalyticsPublisher — NATS
# ────────────────────────────────────────────────────────────────────────────


class TestAnalyticsPublisherNATS:
    @pytest.mark.asyncio
    async def test_publishes_to_correct_subject(self):
        js = _FakeJS()
        publisher = AnalyticsPublisher(js=js)
        evt = AnalyticsEvent(
            event_type=AnalyticsEventType.RUN_STARTED, org_id="acme"
        )
        await publisher.emit_and_wait(evt)
        assert len(js.published) == 1
        subject, _ = js.published[0]
        assert subject == "analytics.events.acme"

    @pytest.mark.asyncio
    async def test_payload_includes_event_type(self):
        js = _FakeJS()
        publisher = AnalyticsPublisher(js=js)
        evt = AnalyticsEvent(
            event_type=AnalyticsEventType.RUN_COMPLETED, org_id="acme"
        )
        await publisher.emit_and_wait(evt)
        _, payload = js.published[0]
        data = json.loads(payload)
        assert data["event_type"] == "run.completed"

    @pytest.mark.asyncio
    async def test_no_crash_when_js_is_none(self):
        publisher = AnalyticsPublisher(js=None)
        evt = AnalyticsEvent(
            event_type=AnalyticsEventType.RUN_STARTED, org_id="org1"
        )
        await publisher.emit_and_wait(evt)  # should not raise

    @pytest.mark.asyncio
    async def test_nats_failure_does_not_raise(self):
        class BrokenJS:
            async def publish(self, subject, payload):
                raise OSError("NATS disconnected")

        publisher = AnalyticsPublisher(js=BrokenJS())
        evt = AnalyticsEvent(
            event_type=AnalyticsEventType.RUN_STARTED, org_id="org1"
        )
        await publisher.emit_and_wait(evt)  # swallowed


# ────────────────────────────────────────────────────────────────────────────
# AnalyticsPublisher — Postgres persistence
# ────────────────────────────────────────────────────────────────────────────


class TestAnalyticsPublisherDB:
    @pytest.mark.asyncio
    async def test_persists_event_to_db(self):
        conn = _FakeConn()
        pool = _FakePool(conn)
        publisher = AnalyticsPublisher(pool=pool)
        evt = AnalyticsEvent(
            event_type=AnalyticsEventType.ACTION_EXECUTED,
            org_id="org1",
            run_id="run-abc",
            props={"tool": "read_file"},
        )
        await publisher.emit_and_wait(evt)
        assert len(conn.executed) == 1
        sql, args = conn.executed[0]
        assert "INSERT INTO analytics_events" in sql
        assert evt.event_id in args

    @pytest.mark.asyncio
    async def test_no_crash_when_pool_is_none(self):
        publisher = AnalyticsPublisher(pool=None)
        evt = AnalyticsEvent(
            event_type=AnalyticsEventType.RUN_STARTED, org_id="org1"
        )
        await publisher.emit_and_wait(evt)  # should not raise

    @pytest.mark.asyncio
    async def test_db_failure_does_not_raise(self):
        class BrokenConn:
            async def execute(self, *args):
                raise RuntimeError("DB error")

        class BrokenPool:
            def acquire(self):
                return _AsyncContextManager(BrokenConn())

        publisher = AnalyticsPublisher(pool=BrokenPool())
        evt = AnalyticsEvent(
            event_type=AnalyticsEventType.RUN_STARTED, org_id="org1"
        )
        await publisher.emit_and_wait(evt)  # swallowed


# ────────────────────────────────────────────────────────────────────────────
# AnalyticsPublisher — fire-and-forget via emit()
# ────────────────────────────────────────────────────────────────────────────


class TestAnalyticsPublisherFireAndForget:
    @pytest.mark.asyncio
    async def test_emit_schedules_background_task(self):
        js = _FakeJS()
        publisher = AnalyticsPublisher(js=js)
        evt = AnalyticsEvent(
            event_type=AnalyticsEventType.RUN_STARTED, org_id="org1"
        )
        await publisher.emit(evt)
        # Allow background task to run
        await asyncio.sleep(0.05)
        assert len(js.published) == 1

    @pytest.mark.asyncio
    async def test_emit_does_not_block(self):
        """emit() should return before the NATS publish completes."""
        import time

        slowness: list[float] = []

        class SlowJS:
            async def publish(self, subject, payload):
                await asyncio.sleep(0.1)
                slowness.append(1)

        publisher = AnalyticsPublisher(js=SlowJS())
        evt = AnalyticsEvent(
            event_type=AnalyticsEventType.RUN_STARTED, org_id="org1"
        )
        t0 = time.monotonic()
        await publisher.emit(evt)
        elapsed = time.monotonic() - t0
        assert elapsed < 0.05  # should return almost immediately


# ────────────────────────────────────────────────────────────────────────────
# Event type coverage
# ────────────────────────────────────────────────────────────────────────────


class TestAnalyticsEventTypes:
    def test_run_lifecycle_events_exist(self):
        types = {e.value for e in AnalyticsEventType}
        assert "run.started" in types
        assert "run.completed" in types
        assert "run.failed" in types
        assert "run.cancelled" in types

    def test_action_events_exist(self):
        types = {e.value for e in AnalyticsEventType}
        assert "action.executed" in types
        assert "action.failed" in types

    def test_policy_events_exist(self):
        types = {e.value for e in AnalyticsEventType}
        assert "policy.violated" in types
        assert "policy.blocked" in types

    def test_publisher_with_both_nats_and_db(self):
        js = _FakeJS()
        conn = _FakeConn()
        pool = _FakePool(conn)
        publisher = AnalyticsPublisher(js=js, pool=pool)
        assert publisher._js is js
        assert publisher._pool is pool
