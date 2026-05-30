"""Tests for Phase L — Streaming action progress."""

from __future__ import annotations

import pytest

from app.domain import AgentEvent
from app.progress import (
    ActionProgress,
    ProgressStream,
    create_progress_stream,
)


# ---------------------------------------------------------------------------
# ActionProgress
# ---------------------------------------------------------------------------


class TestActionProgress:
    def test_defaults(self) -> None:
        p = ActionProgress(run_id="r1", action_id="a1", action_name="bash")
        assert p.progress_type == "output"
        assert p.percentage is None
        assert p.bytes_processed == 0

    def test_to_event(self) -> None:
        p = ActionProgress(
            run_id="r1",
            action_id="a1",
            action_name="read_file",
            content="reading...",
            percentage=0.5,
            query_depth=2,
        )
        event = p.to_event("s1", sequence=3)
        assert isinstance(event, AgentEvent)
        assert event.event_type == "action.progress"
        assert event.run_id == "r1"
        assert event.session_id == "s1"
        assert event.sequence == 3
        assert event.payload["percentage"] == 0.5
        assert event.payload["content"] == "reading..."
        assert event.payload["query_depth"] == 2

    def test_heartbeat_type(self) -> None:
        p = ActionProgress(
            run_id="r1",
            action_id="a1",
            action_name="deploy",
            progress_type="heartbeat",
            content="Running for 5s",
        )
        event = p.to_event("s1")
        assert event.payload["progress_type"] == "heartbeat"


# ---------------------------------------------------------------------------
# ProgressStream
# ---------------------------------------------------------------------------


class MockPublisher:
    def __init__(self) -> None:
        self.events: list[AgentEvent] = []

    async def publish(self, event: AgentEvent) -> None:
        self.events.append(event)


@pytest.mark.asyncio
class TestProgressStream:
    async def test_emit_publishes_event(self) -> None:
        pub = MockPublisher()
        stream = ProgressStream(pub, "r1", "s1", "a1", "bash")
        await stream.emit("line 1")
        assert len(pub.events) == 1
        assert pub.events[0].payload["content"] == "line 1"
        assert pub.events[0].payload["progress_type"] == "output"

    async def test_sequence_increments(self) -> None:
        pub = MockPublisher()
        stream = ProgressStream(pub, "r1", "s1", "a1", "bash")
        await stream.emit("line 1")
        await stream.emit("line 2")
        assert pub.events[0].sequence == 1
        assert pub.events[1].sequence == 2

    async def test_heartbeat(self) -> None:
        pub = MockPublisher()
        stream = ProgressStream(pub, "r1", "s1", "a1", "deploy")
        await stream.heartbeat()
        assert len(pub.events) == 1
        assert pub.events[0].payload["progress_type"] == "heartbeat"

    async def test_status_change(self) -> None:
        pub = MockPublisher()
        stream = ProgressStream(pub, "r1", "s1", "a1", "deploy")
        await stream.status_change("compiling")
        assert pub.events[0].payload["progress_type"] == "status_change"
        assert pub.events[0].payload["content"] == "compiling"

    async def test_bytes_accumulate(self) -> None:
        pub = MockPublisher()
        stream = ProgressStream(pub, "r1", "s1", "a1", "upload")
        await stream.emit("chunk 1", extra_bytes=1024)
        await stream.emit("chunk 2", extra_bytes=2048)
        assert pub.events[1].payload["bytes_processed"] == 3072

    async def test_percentage_tracking(self) -> None:
        pub = MockPublisher()
        stream = ProgressStream(pub, "r1", "s1", "a1", "process")
        await stream.emit("step 1", percentage=0.25)
        await stream.emit("step 2", percentage=0.75)
        assert pub.events[0].payload["percentage"] == 0.25
        assert pub.events[1].payload["percentage"] == 0.75


# ---------------------------------------------------------------------------
# Factory
# ---------------------------------------------------------------------------


class TestCreateProgressStream:
    def test_factory_creates_stream(self) -> None:
        pub = MockPublisher()
        stream = create_progress_stream(pub, "r1", "s1", "a1", "bash", query_depth=1)
        assert isinstance(stream, ProgressStream)

    @pytest.mark.asyncio
    async def test_factory_preserves_query_depth(self) -> None:
        pub = MockPublisher()
        stream = create_progress_stream(pub, "r1", "s1", "a1", "bash", query_depth=4)
        await stream.emit("line 1")
        assert pub.events[0].payload["query_depth"] == 4
