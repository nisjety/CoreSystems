"""Capability-core HTTP client — tool pool, model routing, budget."""

from __future__ import annotations

import logging
from typing import Any

import httpx

from app.config import settings

logger = logging.getLogger(__name__)


class CapabilityClient:
    """Async HTTP client to capability-core (Model Plane v2).

    Handles tool registry, model selection, and budget tracking.
    Replaces the tool-pool and routing portions of the old AICoreClient.
    """

    def __init__(self) -> None:
        self._client: httpx.AsyncClient | None = None

    async def open(self) -> None:
        self._client = httpx.AsyncClient(
            base_url=settings.capability_core_url,
            timeout=httpx.Timeout(30, connect=10),
            headers=self._base_headers(),
        )

    async def close(self) -> None:
        if self._client:
            await self._client.aclose()
            self._client = None

    def _base_headers(self) -> dict[str, str]:
        h: dict[str, str] = {}
        if settings.internal_api_key:
            h["x-internal-api-key"] = settings.internal_api_key
        return h

    @property
    def client(self) -> httpx.AsyncClient:
        if self._client is None:
            raise RuntimeError("CapabilityClient not opened")
        return self._client

    # ---- Tool pool ----

    async def get_tool_pool(
        self,
        session_id: str,
        agent_type: str = "general",
        allowed_tools: list[str] | None = None,
        org_id: str | None = None,
        mcp_manager: Any | None = None,
    ) -> dict[str, Any]:
        """Build the tool pool for a session via capability-core.

        If mcp_manager and org_id are provided, appends MCP tools with
        'mcp:server:tool' prefix to the base pool.
        """
        body: dict[str, Any] = {"session_id": session_id}
        if allowed_tools:
            body["always_load"] = allowed_tools

        resp = await self.client.post("/v1/catalog/pool", json=body)
        resp.raise_for_status()
        pool = resp.json()

        # Append MCP tools (Phase E)
        if mcp_manager and org_id:
            try:
                mcp_tools = await mcp_manager.get_tools_for_org(org_id)
                mcp_tool_names = [
                    f"mcp:{t.server_name}:{t.name}" for t in mcp_tools
                ]
                existing_names = pool.get("tool_names", [])
                pool["tool_names"] = existing_names + mcp_tool_names
            except Exception as exc:
                logger.warning(
                    "mcp_tool_pool_append_failed",
                    extra={"org_id": org_id, "error": str(exc)},
                )

        return pool

    # ---- Tool execution (via capability-core) ----

    async def execute_tool(
        self,
        tool_name: str,
        parameters: dict[str, Any],
        *,
        user_id: str | None = None,
        org_id: str | None = None,
        session_id: str | None = None,
        run_id: str | None = None,
        action_id: str | None = None,
    ) -> dict[str, Any]:
        """Execute a tool through capability-core's tool runtime."""
        body: dict[str, Any] = {
            "tool_name": tool_name,
            "parameters": parameters,
        }
        if session_id:
            body["session_id"] = session_id
        if run_id:
            body["run_id"] = run_id
        if action_id:
            body["action_id"] = action_id

        headers: dict[str, str] = {}
        if user_id:
            headers["X-User-ID"] = user_id
        if org_id:
            headers["X-Org-ID"] = org_id

        resp = await self.client.post(
            "/v1/tools/execute", json=body, headers=headers,
        )
        resp.raise_for_status()
        return resp.json()

    # ---- Model routing ----

    async def select_model(
        self,
        org_id: str,
        feature_requirements: dict[str, bool] | None = None,
        max_input_tokens: int | None = None,
    ) -> dict[str, Any]:
        """Select the best model via capability-core routing policy."""
        body: dict[str, Any] = {"org_id": org_id}
        if feature_requirements:
            body["feature_requirements"] = feature_requirements
        if max_input_tokens is not None:
            body["max_input_tokens"] = max_input_tokens

        resp = await self.client.post("/v1/routing/select", json=body)
        resp.raise_for_status()
        return resp.json()

    # ---- Budget ----

    async def check_budget(self, org_id: str) -> dict[str, Any]:
        """Check whether the org has remaining LLM budget."""
        resp = await self.client.get(f"/v1/routing/budget/{org_id}")
        resp.raise_for_status()
        return resp.json()

    async def record_usage(
        self, org_id: str, session_id: str, cost_nok: float,
    ) -> None:
        """Record LLM spend after a completion."""
        resp = await self.client.post(
            "/v1/routing/usage",
            json={"org_id": org_id, "session_id": session_id, "cost_nok": cost_nok},
        )
        resp.raise_for_status()
