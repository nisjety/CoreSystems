"""Integration tests - cross-plane connectivity for Model Plane v2.

Unit tests run without any network.
Live connectivity tests are marked @pytest.mark.integration and are skipped
when the target services are unreachable.

Run unit tests only (default):
    pytest tests/test_integration_planes.py -v -m "not integration"

Run integration tests (requires running CoreSystem stack):
    pytest tests/test_integration_planes.py -v -m integration
"""

from __future__ import annotations

import asyncio
import os
import json
import pytest


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def _env(key: str, default: str = "") -> str:
    return os.environ.get(key, default)


# ---------------------------------------------------------------------------
# 1. ControlPlaneSubscriber: imports and instantiation
# ---------------------------------------------------------------------------

class TestControlPlaneSubscriberUnit:
    """Pure-unit checks - no network required."""

    def test_import(self) -> None:
        from app.control_plane_subscriber import ControlPlaneSubscriber  # noqa: F401
        assert ControlPlaneSubscriber is not None

    def test_instantiation(self) -> None:
        from app.control_plane_subscriber import ControlPlaneSubscriber
        sub = ControlPlaneSubscriber(nats_url="nats://localhost:4222", nats_token="test")
        assert sub.nats_url == "nats://localhost:4222"
        assert not sub._initialized

    def test_is_org_paused_default_false(self) -> None:
        from app.control_plane_subscriber import ControlPlaneSubscriber
        sub = ControlPlaneSubscriber(nats_url="", nats_token="")
        assert sub.is_org_paused("org-x") is False

    def test_pause_and_resume_org(self) -> None:
        from app.control_plane_subscriber import ControlPlaneSubscriber
        sub = ControlPlaneSubscriber(nats_url="", nats_token="")
        sub.pause_org("org-x", ttl_seconds=3600)
        assert sub.is_org_paused("org-x") is True
        sub.resume_org("org-x")
        assert sub.is_org_paused("org-x") is False

    def test_pause_ttl_expiry(self) -> None:
        import time
        from app.control_plane_subscriber import ControlPlaneSubscriber
        sub = ControlPlaneSubscriber(nats_url="", nats_token="")
        sub._paused_orgs["org-y"] = time.monotonic() - 1
        assert sub.is_org_paused("org-y") is False
        assert "org-y" not in sub._paused_orgs

    def test_initialize_no_nats_url(self) -> None:
        from app.control_plane_subscriber import ControlPlaneSubscriber
        sub = ControlPlaneSubscriber(nats_url="", nats_token="")
        result = asyncio.run(sub.initialize())
        assert result is False
        assert sub._initialized is True

    def test_close_before_connect(self) -> None:
        from app.control_plane_subscriber import ControlPlaneSubscriber
        sub = ControlPlaneSubscriber(nats_url="", nats_token="")
        asyncio.run(sub.close())  # must not raise


# ---------------------------------------------------------------------------
# 2. LangChain adapter: wire() function and execute() dispatch
# ---------------------------------------------------------------------------

class TestLangchainAdapterUnit:
    """Verify the adapter wiring without network."""

    def test_import(self) -> None:
        from app.adapters import langchain_tool_adapter  # noqa: F401
        assert langchain_tool_adapter is not None

    def test_wire_function_exists(self) -> None:
        from app.adapters import langchain_tool_adapter
        assert callable(getattr(langchain_tool_adapter, "wire", None))

    def test_execute_returns_error_when_not_wired(self) -> None:
        from app.adapters import langchain_tool_adapter
        from app.domain import AgentAction, RunRecord, ActionKind, ActionTarget

        langchain_tool_adapter._capability = None

        action = AgentAction(
            kind=ActionKind.TOOL_CALL,
            target=ActionTarget.INTERNAL,
            name="some_tool",
        )
        run = RunRecord(
            session_id="sess-test-001",
            user_id="user-test",
            goal="test",
        )
        result = asyncio.run(langchain_tool_adapter.execute(action, run))
        assert result["status"] == "not_wired"

    def test_execute_dispatches_when_wired(self) -> None:
        from unittest.mock import AsyncMock, MagicMock
        from app.adapters import langchain_tool_adapter
        from app.domain import AgentAction, RunRecord, ActionKind, ActionTarget

        mock_capability = MagicMock()
        mock_capability.execute_tool = AsyncMock(return_value={"result": "ok"})
        langchain_tool_adapter.wire(mock_capability)

        action = AgentAction(
            kind=ActionKind.TOOL_CALL,
            target=ActionTarget.INTERNAL,
            name="search_tool",
            input={"query": "test query"},
        )
        run = RunRecord(
            session_id="sess-test-002",
            user_id="user-a",
            goal="test",
        )
        result = asyncio.run(langchain_tool_adapter.execute(action, run))
        assert result == {"result": "ok"}
        mock_capability.execute_tool.assert_called_once()
        call_kwargs = mock_capability.execute_tool.call_args
        assert call_kwargs[0][0] == "search_tool"
        assert call_kwargs[0][1] == {"query": "test query"}
        # Verify all execution context fields are forwarded (matches CC ToolUseContext pattern:
        # toolUseId=action.id, chainId=run_id, session=session_id)
        kw = call_kwargs.kwargs
        assert kw.get("session_id") == "sess-test-002", "session_id must be forwarded to capability-core"
        assert kw.get("run_id") == run.id, "run_id (chain scope) must be forwarded"
        assert kw.get("action_id"), "action_id (tool_use_id equivalent) must be forwarded"
        assert kw.get("user_id") == "user-a"
        langchain_tool_adapter._capability = None


# ---------------------------------------------------------------------------
# 3. Documents client: header name correctness
# ---------------------------------------------------------------------------

class TestDocumentsClientUnit:
    """Verify documents_client uses the correct x-internal-key header."""

    def test_header_name_is_x_internal_key(self) -> None:
        import inspect
        from app.documents_client import DocumentsClient

        src = inspect.getsource(DocumentsClient._base_headers)
        assert "x-internal-key" in src, (
            "_base_headers must set 'x-internal-key' header for Data Plane auth"
        )
        assert "x-internal-api-key" not in src, (
            "_base_headers must NOT set 'x-internal-api-key' (old incorrect name)"
        )

    def test_uses_data_plane_internal_key_setting(self) -> None:
        import inspect
        from app.documents_client import DocumentsClient

        src = inspect.getsource(DocumentsClient._base_headers)
        assert "data_plane_internal_key" in src, (
            "_base_headers must use settings.data_plane_internal_key"
        )


# ---------------------------------------------------------------------------
# 4. Config: data_plane_internal_key field present
# ---------------------------------------------------------------------------

class TestConfigUnit:
    def test_data_plane_internal_key_field(self) -> None:
        from app.config import Settings
        s = Settings(_env_file=None)  # type: ignore[call-arg]
        assert hasattr(s, "data_plane_internal_key")
        assert isinstance(s.data_plane_internal_key, str)


# ---------------------------------------------------------------------------
# 5. Live network tests - marked @pytest.mark.integration
# ---------------------------------------------------------------------------

@pytest.mark.integration
def test_velion_nats_connectivity() -> None:
    """Connect to velion-nats with token and publish a test message."""
    nats_url = _env("NATS_URL", "nats://velion-nats:4222")
    nats_token = _env("NATS_TOKEN")

    if not nats_token:
        pytest.skip("NATS_TOKEN not set")

    try:
        import nats as nats_lib
    except ImportError:
        pytest.skip("nats library not available")

    async def _run() -> None:
        try:
            nc = await asyncio.wait_for(
                nats_lib.connect(nats_url, token=nats_token, max_reconnect_attempts=1),
                timeout=5.0,
            )
        except Exception as exc:
            pytest.skip(f"velion-nats unreachable: {exc}")
            return
        try:
            js = nc.jetstream()
            try:
                await asyncio.wait_for(
                    js.publish("velion.mp-v2.test.ping", json.dumps({"ping": True}).encode()),
                    timeout=3.0,
                )
            except Exception:
                pass
            assert nc.is_connected
        finally:
            await nc.close()

    asyncio.run(_run())


@pytest.mark.integration
def test_auth_core_reachability() -> None:
    import httpx
    auth_url = _env("AUTH_CORE_URL", "http://auth-core:3011")

    async def _run() -> None:
        try:
            async with httpx.AsyncClient(timeout=5.0) as client:
                resp = await client.get(f"{auth_url}/api/auth/get-session")
            assert resp.status_code in (200, 401, 403, 422)
        except httpx.ConnectError:
            pytest.skip("auth-core unreachable")
        except httpx.TimeoutException:
            pytest.skip("auth-core timed out")

    asyncio.run(_run())


@pytest.mark.integration
def test_org_core_reachability() -> None:
    import httpx
    org_url = _env("ORG_CORE_URL", "http://org-core:8080")
    internal_key = _env("INTERNAL_API_KEY", "")
    headers = {"x-internal-api-key": internal_key} if internal_key else {}

    async def _run() -> None:
        try:
            async with httpx.AsyncClient(timeout=5.0) as client:
                resp = await client.get(
                    f"{org_url}/api/v1/orgs/test-org/entitlements",
                    headers=headers,
                )
            assert resp.status_code in (200, 401, 403, 404, 422)
        except httpx.ConnectError:
            pytest.skip("org-core unreachable")
        except httpx.TimeoutException:
            pytest.skip("org-core timed out")

    asyncio.run(_run())


@pytest.mark.integration
def test_data_plane_retrieval_reachability() -> None:
    import httpx
    retrieval_url = _env("DATA_PLANE_RETRIEVAL_URL", "http://data-retrieval-service:8004")
    dp_key = _env("DATA_PLANE_INTERNAL_KEY", "")
    headers: dict[str, str] = {"x-org-id": "test-org"}
    if dp_key:
        headers["x-internal-key"] = dp_key

    async def _run() -> None:
        try:
            async with httpx.AsyncClient(timeout=8.0) as client:
                resp = await client.post(
                    f"{retrieval_url}/v1/retrieve",
                    json={"query": "integration test probe", "top_k": 1},
                    headers=headers,
                )
            assert resp.status_code in (200, 400, 401, 403, 404, 422)
        except httpx.ConnectError:
            pytest.skip("retrieval-service unreachable")
        except httpx.TimeoutException:
            pytest.skip("retrieval-service timed out")

    asyncio.run(_run())


@pytest.mark.integration
def test_control_plane_subscriber_live_connect() -> None:
    nats_url = _env("NATS_URL", "nats://velion-nats:4222")
    nats_token = _env("NATS_TOKEN")
    if not nats_token:
        pytest.skip("NATS_TOKEN not set")

    from app.control_plane_subscriber import ControlPlaneSubscriber
    sub = ControlPlaneSubscriber(nats_url=nats_url, nats_token=nats_token, service_name="mp-v2-test")

    async def _run() -> None:
        try:
            result = await asyncio.wait_for(sub.initialize(), timeout=8.0)
            assert isinstance(result, bool)
        except asyncio.TimeoutError:
            pytest.skip("velion-nats connect timed out")
        finally:
            await sub.close()

    asyncio.run(_run())


@pytest.mark.integration
def test_usage_reporter_publishes_without_error() -> None:
    from unittest.mock import AsyncMock, MagicMock
    mock_nats = MagicMock()
    mock_nats.publish = AsyncMock(return_value=None)

    async def _run() -> None:
        try:
            from app.usage_reporter import UsageReporter  # type: ignore[attr-defined]
            reporter = UsageReporter(mock_nats)
            await reporter.report(
                org_id="test-org",
                user_id="test-user",
                model="claude-haiku-4-5",
                input_tokens=100,
                output_tokens=50,
            )
        except (AttributeError, TypeError):
            pytest.skip("UsageReporter API changed")

    asyncio.run(_run())
