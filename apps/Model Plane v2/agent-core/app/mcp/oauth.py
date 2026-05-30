"""MCP OAuth 2.0 / PKCE token manager.

Implements the authorization code flow with PKCE for MCP servers that
require OAuth.  Tokens are stored in Redis under the key
``mcp:oauth:{server_id}`` and auto-refreshed 5 minutes before expiry.

Usage:
    mgr = McpOAuthTokenManager(redis_client, server_config)
    token = await mgr.get_token()          # fetches / refreshes as needed
    headers = await mgr.auth_headers()     # {"Authorization": "Bearer <token>"}
"""

from __future__ import annotations

import base64
import hashlib
import json
import logging
import os
import time
import urllib.parse
from typing import Any

import httpx

from app.mcp.config import McpOAuthConfig

logger = logging.getLogger(__name__)

# Refresh a token this many seconds before it actually expires
_EARLY_REFRESH_SECS = 300  # 5 minutes


def _generate_pkce() -> tuple[str, str]:
    """Return (code_verifier, code_challenge) for PKCE flow."""
    verifier = base64.urlsafe_b64encode(os.urandom(32)).rstrip(b"=").decode()
    digest = hashlib.sha256(verifier.encode()).digest()
    challenge = base64.urlsafe_b64encode(digest).rstrip(b"=").decode()
    return verifier, challenge


class McpOAuthTokenManager:
    """Per-server OAuth token lifecycle manager, backed by Redis.

    Args:
        redis: An ``aioredis`` / ``redis.asyncio`` client instance.
        server_id: Unique server identifier (used as Redis key suffix).
        config: OAuth configuration from ``McpServerConfig.oauth``.
    """

    def __init__(self, redis: Any, server_id: str, config: McpOAuthConfig) -> None:
        self._redis = redis
        self._server_id = server_id
        self._config = config
        self._key = f"mcp:oauth:{server_id}"

    # ------------------------------------------------------------------
    # Public API
    # ------------------------------------------------------------------

    async def get_token(self) -> str | None:
        """Return a valid access token, refreshing if necessary.

        Returns None if no token has been acquired yet or refresh failed.
        """
        data = await self._load()
        if data is None:
            return None

        expires_at = data.get("expires_at", 0)
        if time.time() >= expires_at - _EARLY_REFRESH_SECS:
            data = await self._refresh(data)
            if data is None:
                return None

        return data.get("access_token")

    async def auth_headers(self) -> dict[str, str]:
        """Return Authorization header dict, or empty dict if no token."""
        token = await self.get_token()
        if not token:
            return {}
        return {"Authorization": f"Bearer {token}"}

    async def store_token_response(self, token_response: dict[str, Any]) -> None:
        """Persist a raw token endpoint response to Redis."""
        access_token = token_response.get("access_token")
        if not access_token:
            raise ValueError("token_response missing 'access_token'")

        expires_in = int(token_response.get("expires_in", 3600))
        data = {
            "access_token": access_token,
            "refresh_token": token_response.get("refresh_token"),
            "expires_at": time.time() + expires_in,
            "token_type": token_response.get("token_type", "Bearer"),
        }
        await self._save(data, ttl=expires_in + _EARLY_REFRESH_SECS)

    def build_authorization_url(self, redirect_uri: str) -> tuple[str, str]:
        """Build the OAuth authorization URL with PKCE.

        Returns:
            (authorization_url, code_verifier) — caller must store the
            verifier to complete the flow in ``exchange_code()``.
        """
        verifier, challenge = _generate_pkce()
        params = {
            "response_type": "code",
            "client_id": self._config.client_id,
            "redirect_uri": redirect_uri,
            "scope": " ".join(self._config.scopes),
            "code_challenge": challenge,
            "code_challenge_method": "S256",
        }
        url = f"{self._config.authorization_url}?{urllib.parse.urlencode(params)}"
        return url, verifier

    async def exchange_code(
        self,
        code: str,
        code_verifier: str,
        redirect_uri: str,
    ) -> None:
        """Exchange an authorization code for tokens and persist them."""
        body: dict[str, str] = {
            "grant_type": "authorization_code",
            "code": code,
            "redirect_uri": redirect_uri,
            "code_verifier": code_verifier,
            "client_id": self._config.client_id,
        }
        if self._config.client_secret:
            body["client_secret"] = self._config.client_secret

        async with httpx.AsyncClient(timeout=15.0) as client:
            resp = await client.post(self._config.token_url, data=body)
            resp.raise_for_status()
            await self.store_token_response(resp.json())

    # ------------------------------------------------------------------
    # Private helpers
    # ------------------------------------------------------------------

    async def _load(self) -> dict[str, Any] | None:
        raw = await self._redis.get(self._key)
        if raw is None:
            return None
        return json.loads(raw)

    async def _save(self, data: dict[str, Any], ttl: int = 7200) -> None:
        await self._redis.setex(self._key, ttl, json.dumps(data))

    async def _refresh(self, data: dict[str, Any]) -> dict[str, Any] | None:
        refresh_token = data.get("refresh_token")
        if not refresh_token:
            logger.warning("mcp_oauth_no_refresh_token", extra={"server": self._server_id})
            return None

        body: dict[str, str] = {
            "grant_type": "refresh_token",
            "refresh_token": refresh_token,
            "client_id": self._config.client_id,
        }
        if self._config.client_secret:
            body["client_secret"] = self._config.client_secret

        try:
            async with httpx.AsyncClient(timeout=15.0) as client:
                resp = await client.post(self._config.token_url, data=body)
                resp.raise_for_status()
                token_data = resp.json()
        except Exception as exc:
            logger.error(
                "mcp_oauth_refresh_failed",
                extra={"server": self._server_id, "error": str(exc)},
            )
            return None

        expires_in = int(token_data.get("expires_in", 3600))
        new_data: dict[str, Any] = {
            "access_token": token_data["access_token"],
            # Servers may not return a new refresh_token; keep old one
            "refresh_token": token_data.get("refresh_token", refresh_token),
            "expires_at": time.time() + expires_in,
            "token_type": token_data.get("token_type", "Bearer"),
        }
        await self._save(new_data, ttl=expires_in + _EARLY_REFRESH_SECS)
        logger.info("mcp_oauth_token_refreshed", extra={"server": self._server_id})
        return new_data
