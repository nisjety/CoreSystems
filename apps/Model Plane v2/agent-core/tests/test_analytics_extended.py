"""Tests for Phase B4: Analytics — Event Types, Multi-Sink, PII, Feature Flags."""

from __future__ import annotations

import pytest
import pytest_asyncio

from app.analytics.domain import AnalyticsEvent, AnalyticsEventType
from app.analytics.pii import has_pii, strip_pii
from app.analytics.sink import SinkConfig, SinkDispatcher, SinkType
from app.analytics.feature_flags import FeatureFlagService


# ===========================================================================
# Expanded event types
# ===========================================================================


class TestExpandedEventTypes:
    def test_total_event_count(self):
        # Should have ~30+ event types
        assert len(AnalyticsEventType) >= 30

    def test_tool_events(self):
        assert AnalyticsEventType.TOOL_CALLED == "tool.called"
        assert AnalyticsEventType.TOOL_RESULT == "tool.result"
        assert AnalyticsEventType.TOOL_BLOCKED == "tool.blocked"

    def test_hook_events(self):
        assert AnalyticsEventType.HOOK_EXECUTED == "hook.executed"
        assert AnalyticsEventType.HOOK_BLOCKED == "hook.blocked"

    def test_task_events(self):
        assert AnalyticsEventType.TASK_SPAWNED == "task.spawned"
        assert AnalyticsEventType.TASK_COMPLETED == "task.completed"
        assert AnalyticsEventType.TASK_KILLED == "task.killed"

    def test_mcp_events(self):
        assert AnalyticsEventType.MCP_CONNECTED == "mcp.connected"
        assert AnalyticsEventType.MCP_DISCONNECTED == "mcp.disconnected"

    def test_compact_events(self):
        assert AnalyticsEventType.COMPACT_STARTED == "compact.started"
        assert AnalyticsEventType.COMPACT_COMPLETED == "compact.completed"

    def test_turn_events(self):
        assert AnalyticsEventType.TURN_STARTED == "turn.started"
        assert AnalyticsEventType.TURN_COMPLETED == "turn.completed"

    def test_experiment_event(self):
        assert AnalyticsEventType.EXPERIMENT_EXPOSED == "experiment.exposed"


# ===========================================================================
# PII stripping
# ===========================================================================


class TestPIIStripping:
    def test_strip_prefixed_fields(self):
        props = {"_PII_email": "a@b.com", "action": "read", "_PII_ip": "1.2.3.4"}
        clean = strip_pii(props)
        assert "_PII_email" not in clean
        assert "_PII_ip" not in clean
        assert clean["action"] == "read"

    def test_strip_denylist_fields(self):
        props = {"email": "a@b.com", "phone": "123", "model": "gpt-4"}
        clean = strip_pii(props)
        assert "email" not in clean
        assert "phone" not in clean
        assert clean["model"] == "gpt-4"

    def test_has_pii_true(self):
        assert has_pii({"_PII_name": "Alice"}) is True
        assert has_pii({"email": "x"}) is True

    def test_has_pii_false(self):
        assert has_pii({"action": "read", "count": 5}) is False

    def test_empty_props(self):
        assert strip_pii({}) == {}
        assert has_pii({}) is False

    def test_case_insensitive_denylist(self):
        props = {"Email": "x@y.com", "TOKEN": "abc"}
        clean = strip_pii(props)
        assert "Email" not in clean
        assert "TOKEN" not in clean


# ===========================================================================
# Multi-sink dispatcher
# ===========================================================================


class FakeSink:
    """Test sink that records sent events."""

    def __init__(self, cfg: SinkConfig) -> None:
        self._config = cfg
        self.events: list[AnalyticsEvent] = []

    @property
    def config(self) -> SinkConfig:
        return self._config

    async def send(self, event: AnalyticsEvent) -> None:
        self.events.append(event)


class FailingSink:
    """Sink that always raises."""

    def __init__(self, cfg: SinkConfig) -> None:
        self._config = cfg

    @property
    def config(self) -> SinkConfig:
        return self._config

    async def send(self, event: AnalyticsEvent) -> None:
        raise ConnectionError("sink down")


def _make_event(**overrides) -> AnalyticsEvent:
    defaults = {
        "event_type": AnalyticsEventType.RUN_STARTED,
        "org_id": "org1",
    }
    defaults.update(overrides)
    return AnalyticsEvent(**defaults)


class TestSinkDispatcher:
    @pytest.mark.asyncio
    async def test_dispatch_to_single_sink(self):
        disp = SinkDispatcher()
        cfg = SinkConfig(name="test", sink_type=SinkType.NATS)
        sink = FakeSink(cfg)
        disp.register(sink)

        event = _make_event()
        sent = await disp.dispatch(event)
        assert sent == 1
        assert len(sink.events) == 1

    @pytest.mark.asyncio
    async def test_dispatch_to_multiple_sinks(self):
        disp = SinkDispatcher()
        s1 = FakeSink(SinkConfig(name="s1", sink_type=SinkType.NATS))
        s2 = FakeSink(SinkConfig(name="s2", sink_type=SinkType.POSTGRES))
        disp.register(s1)
        disp.register(s2)

        sent = await disp.dispatch(_make_event())
        assert sent == 2

    @pytest.mark.asyncio
    async def test_disabled_sink_skipped(self):
        disp = SinkDispatcher()
        cfg = SinkConfig(name="off", sink_type=SinkType.HTTP, enabled=False)
        sink = FakeSink(cfg)
        disp.register(sink)

        sent = await disp.dispatch(_make_event())
        assert sent == 0

    @pytest.mark.asyncio
    async def test_pii_stripped_for_non_privileged(self):
        disp = SinkDispatcher()
        cfg = SinkConfig(name="ext", sink_type=SinkType.HTTP, privileged=False)
        sink = FakeSink(cfg)
        disp.register(sink)

        event = _make_event(props={"email": "a@b.com", "action": "read"})
        await disp.dispatch(event)

        assert len(sink.events) == 1
        assert "email" not in sink.events[0].props
        assert sink.events[0].props["action"] == "read"

    @pytest.mark.asyncio
    async def test_pii_preserved_for_privileged(self):
        disp = SinkDispatcher()
        cfg = SinkConfig(name="int", sink_type=SinkType.POSTGRES, privileged=True)
        sink = FakeSink(cfg)
        disp.register(sink)

        event = _make_event(props={"email": "a@b.com"})
        await disp.dispatch(event)

        assert "email" in sink.events[0].props

    @pytest.mark.asyncio
    async def test_failing_sink_doesnt_crash(self):
        disp = SinkDispatcher()
        ok_sink = FakeSink(SinkConfig(name="ok", sink_type=SinkType.NATS))
        fail_sink = FailingSink(SinkConfig(name="fail", sink_type=SinkType.HTTP))
        disp.register(fail_sink)
        disp.register(ok_sink)

        sent = await disp.dispatch(_make_event())
        assert sent == 1  # ok_sink succeeded, fail_sink didn't
        assert len(ok_sink.events) == 1

    @pytest.mark.asyncio
    async def test_zero_sample_rate_skips(self):
        disp = SinkDispatcher()
        cfg = SinkConfig(name="s", sink_type=SinkType.NATS, sample_rate=0.0)
        sink = FakeSink(cfg)
        disp.register(sink)

        sent = await disp.dispatch(_make_event())
        assert sent == 0

    @pytest.mark.asyncio
    async def test_per_event_sample_override(self):
        disp = SinkDispatcher()
        cfg = SinkConfig(name="s", sink_type=SinkType.NATS, sample_rate=1.0)
        sink = FakeSink(cfg)
        disp.register(sink)

        disp.set_sample_rate(AnalyticsEventType.RUN_STARTED, 0.0)
        sent = await disp.dispatch(_make_event())
        assert sent == 0


# ===========================================================================
# Feature flags
# ===========================================================================


class TestFeatureFlagsNoRedis:
    @pytest.mark.asyncio
    async def test_default_false(self):
        ff = FeatureFlagService()
        assert await ff.is_enabled("new-feature") is False

    @pytest.mark.asyncio
    async def test_explicit_default(self):
        ff = FeatureFlagService()
        assert await ff.is_enabled("my-flag", default=True) is True

    @pytest.mark.asyncio
    async def test_set_default(self):
        ff = FeatureFlagService()
        ff.set_default("my-flag", True)
        assert await ff.is_enabled("my-flag") is True

    @pytest.mark.asyncio
    async def test_set_flag_without_redis(self):
        ff = FeatureFlagService()
        await ff.set_flag("my-flag", True)
        assert await ff.is_enabled("my-flag") is True

    @pytest.mark.asyncio
    async def test_get_all_defaults(self):
        ff = FeatureFlagService()
        ff.set_default("a", True)
        ff.set_default("b", False)
        defaults = await ff.get_all_defaults()
        assert defaults == {"a": True, "b": False}


class TestFeatureFlagsWithFakeRedis:
    """Test Redis integration with a fake async Redis client."""

    class FakeRedis:
        def __init__(self):
            self._store: dict[str, str] = {}

        async def get(self, key: str) -> str | None:
            return self._store.get(key)

        async def set(self, key: str, value: str, ex: int | None = None) -> None:
            self._store[key] = value

    @pytest.mark.asyncio
    async def test_global_flag(self):
        redis = self.FakeRedis()
        ff = FeatureFlagService(redis=redis)
        await ff.set_flag("beta", True)
        assert await ff.is_enabled("beta") is True

    @pytest.mark.asyncio
    async def test_org_override(self):
        redis = self.FakeRedis()
        ff = FeatureFlagService(redis=redis)
        await ff.set_flag("beta", True)
        await ff.set_flag("beta", False, org_id="org-1")
        # Global is True, but org-1 override is False
        assert await ff.is_enabled("beta", org_id="org-1") is False
        assert await ff.is_enabled("beta") is True
