"""Tests for MCP SSE/WS/OAuth transport additions (Phase O).

These tests use mock objects to avoid needing real servers.
"""

from __future__ import annotations

import asyncio
import json
import time
from unittest.mock import AsyncMock, MagicMock, patch

import pytest

from app.mcp.config import (
    McpOAuthConfig,
    McpServerConfig,
    McpServerStatus,
    McpTransport,
)
from app.mcp.oauth import McpOAuthTokenManager, _generate_pkce


# ────────────────────────────────────────────────────────────────────────────
# Helper factories
# ────────────────────────────────────────────────────────────────────────────


def _sse_config(name: str = "test-sse") -> McpServerConfig:
    return McpServerConfig(
        name=name,
        transport=McpTransport.SSE,
        url="http://sse-server/",
        org_id="org1",
    )


def _ws_config(name: str = "test-ws") -> McpServerConfig:
    return McpServerConfig(
        name=name,
        transport=McpTransport.WS,
        url="ws://ws-server/",
        org_id="org1",
    )


def _oauth_config() -> McpOAuthConfig:
    return McpOAuthConfig(
        client_id="my-client",
        authorization_url="https://auth.example.com/authorize",
        token_url="https://auth.example.com/token",
        scopes=["mcp:read", "mcp:write"],
        client_secret="secret",
    )


# ────────────────────────────────────────────────────────────────────────────
# McpTransport enum
# ────────────────────────────────────────────────────────────────────────────


class TestMcpTransportEnum:
    def test_sse_value(self) -> None:
        assert McpTransport.SSE.value == "sse"

    def test_ws_value(self) -> None:
        assert McpTransport.WS.value == "ws"

    def test_stdio_unchanged(self) -> None:
        assert McpTransport.STDIO.value == "stdio"

    def test_http_unchanged(self) -> None:
        assert McpTransport.HTTP.value == "http"


# ────────────────────────────────────────────────────────────────────────────
# McpOAuthConfig
# ────────────────────────────────────────────────────────────────────────────


class TestMcpOAuthConfig:
    def test_round_trip(self) -> None:
        cfg = _oauth_config()
        assert cfg.client_id == "my-client"
        assert "mcp:read" in cfg.scopes

    def test_optional_secret_defaults_none(self) -> None:
        cfg = McpOAuthConfig(
            client_id="x",
            authorization_url="https://a",
            token_url="https://t",
            scopes=[],
        )
        assert cfg.client_secret is None

    def test_server_config_accepts_oauth(self) -> None:
        cfg = McpServerConfig(
            name="srv",
            transport=McpTransport.SSE,
            url="http://x/",
            org_id="org1",
            oauth=_oauth_config(),
        )
        assert cfg.oauth is not None
        assert cfg.oauth.client_id == "my-client"

    def test_server_config_oauth_defaults_none(self) -> None:
        cfg = _sse_config()
        assert cfg.oauth is None


# ────────────────────────────────────────────────────────────────────────────
# PKCE helpers
# ────────────────────────────────────────────────────────────────────────────


class TestPKCE:
    def test_verifier_and_challenge_differ(self) -> None:
        verifier, challenge = _generate_pkce()
        assert verifier != challenge

    def test_verifier_is_base64url(self) -> None:
        verifier, _ = _generate_pkce()
        # base64url chars only, no padding
        assert "=" not in verifier

    def test_challenge_is_base64url(self) -> None:
        _, challenge = _generate_pkce()
        assert "=" not in challenge

    def test_unique_per_call(self) -> None:
        v1, _ = _generate_pkce()
        v2, _ = _generate_pkce()
        assert v1 != v2


# ────────────────────────────────────────────────────────────────────────────
# McpOAuthTokenManager
# ────────────────────────────────────────────────────────────────────────────


class TestMcpOAuthTokenManager:
    def _make_redis(self, stored: dict | None = None) -> MagicMock:
        redis = MagicMock()
        if stored:
            redis.get = AsyncMock(return_value=json.dumps(stored).encode())
        else:
            redis.get = AsyncMock(return_value=None)
        redis.setex = AsyncMock()
        return redis

    def _make_mgr(self, redis: MagicMock) -> McpOAuthTokenManager:
        return McpOAuthTokenManager(
            redis=redis,
            server_id="srv1",
            config=_oauth_config(),
        )

    @pytest.mark.asyncio
    async def test_get_token_no_stored_returns_none(self) -> None:
        mgr = self._make_mgr(self._make_redis(None))
        token = await mgr.get_token()
        assert token is None

    @pytest.mark.asyncio
    async def test_get_token_valid_returns_token(self) -> None:
        stored = {
            "access_token": "valid-token",
            "expires_at": time.time() + 3600,
        }
        mgr = self._make_mgr(self._make_redis(stored))
        token = await mgr.get_token()
        assert token == "valid-token"

    @pytest.mark.asyncio
    async def test_auth_headers_valid_token(self) -> None:
        stored = {
            "access_token": "valid-token",
            "expires_at": time.time() + 3600,
        }
        mgr = self._make_mgr(self._make_redis(stored))
        headers = await mgr.auth_headers()
        assert headers == {"Authorization": "Bearer valid-token"}

    @pytest.mark.asyncio
    async def test_auth_headers_no_token_is_empty(self) -> None:
        mgr = self._make_mgr(self._make_redis(None))
        headers = await mgr.auth_headers()
        assert headers == {}

    @pytest.mark.asyncio
    async def test_store_token_response_persists(self) -> None:
        redis = self._make_redis(None)
        mgr = self._make_mgr(redis)
        await mgr.store_token_response({
            "access_token": "new-tok",
            "expires_in": 3600,
            "refresh_token": "ref-tok",
        })
        redis.setex.assert_called_once()
        key, ttl, payload = redis.setex.call_args[0]
        assert "mcp:oauth:srv1" in key
        data = json.loads(payload)
        assert data["access_token"] == "new-tok"
        assert data["refresh_token"] == "ref-tok"

    @pytest.mark.asyncio
    async def test_store_token_missing_access_token_raises(self) -> None:
        mgr = self._make_mgr(self._make_redis(None))
        with pytest.raises(ValueError, match="access_token"):
            await mgr.store_token_response({"expires_in": 3600})

    def test_build_authorization_url_contains_pkce(self) -> None:
        redis = self._make_redis(None)
        mgr = self._make_mgr(redis)
        url, verifier = mgr.build_authorization_url("https://app/callback")
        assert "code_challenge=" in url
        assert "code_challenge_method=S256" in url
        assert "client_id=my-client" in url
        assert len(verifier) > 20


# ────────────────────────────────────────────────────────────────────────────
# MCPClient reconnect logic (unit, no network)
# ────────────────────────────────────────────────────────────────────────────


class TestMcpClientReconnect:
    @pytest.mark.asyncio
    async def test_reconnect_calls_connect_after_disconnect(self) -> None:
        from app.mcp.client import MCPClient

        cfg = _sse_config()
        client = MCPClient(cfg)

        connect_calls = []
        disconnect_calls = []

        async def fake_connect() -> None:
            connect_calls.append(1)
            client._status = McpServerStatus.CONNECTED

        async def fake_disconnect() -> None:
            disconnect_calls.append(1)
            client._status = McpServerStatus.DISCONNECTED

        with (
            patch.object(client, "connect", side_effect=fake_connect),
            patch.object(client, "disconnect", side_effect=fake_disconnect),
            patch("asyncio.sleep", new_callable=AsyncMock),
        ):
            await client.reconnect_with_backoff(max_retries=3)

        assert len(disconnect_calls) == 1
        assert len(connect_calls) == 1

    @pytest.mark.asyncio
    async def test_reconnect_retries_on_failure(self) -> None:
        from app.mcp.client import MCPClient

        cfg = _sse_config()
        client = MCPClient(cfg)

        attempt = {"n": 0}

        async def flaky_connect() -> None:
            attempt["n"] += 1
            if attempt["n"] < 3:
                raise RuntimeError("not ready")
            client._status = McpServerStatus.CONNECTED

        with (
            patch.object(client, "connect", side_effect=flaky_connect),
            patch.object(client, "disconnect", new_callable=AsyncMock),
            patch("asyncio.sleep", new_callable=AsyncMock),
        ):
            await client.reconnect_with_backoff(max_retries=5)

        assert attempt["n"] == 3

    @pytest.mark.asyncio
    async def test_reconnect_raises_after_max_retries_exceeded(self) -> None:
        from app.mcp.client import MCPClient

        cfg = _sse_config()
        client = MCPClient(cfg)

        async def always_fail() -> None:
            raise RuntimeError("down")

        with (
            patch.object(client, "connect", side_effect=always_fail),
            patch.object(client, "disconnect", new_callable=AsyncMock),
            patch("asyncio.sleep", new_callable=AsyncMock),
        ):
            with pytest.raises(RuntimeError, match="could not reconnect"):
                await client.reconnect_with_backoff(max_retries=3)
