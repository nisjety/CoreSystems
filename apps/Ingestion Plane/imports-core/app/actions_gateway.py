"""Async client for integration-corev2's connections + actions surface.

imports-core never sees provider OAuth tokens: token resolution and the provider
HTTP call happen inside integration-corev2's executor. We only speak the
connections-list and per-connection actions HTTP contract:

  GET  {base}/api/v1/connections?organizationId=&providerKey=
       -> {"success": true, "data": {"connections": [...]}}
  POST {base}/api/v1/connections/{connectionId}/actions
       body {"operation","params","body"}
       -> {"success": true, "data": {"action": {"providerKey","operation","result"}}}

Auth is the caller's verified ingestion-audience Bearer token. Provider secrets
remain inside integration-corev2.
"""

from typing import Any
from urllib.parse import quote

import httpx


class ActionsGatewayError(Exception):
    """Raised when the gateway returns a non-success envelope or transport fails."""


class ActionsGateway:
    def __init__(
        self,
        base_url: str,
        bearer_token: str,
        http_client: httpx.AsyncClient,
    ) -> None:
        self._base = (base_url or "").rstrip("/")
        self._bearer_token = bearer_token or ""
        self._client = http_client

    def configured(self) -> bool:
        return bool(self._base and self._bearer_token)

    def _headers(self, org_id: str | None) -> dict[str, str]:
        headers = {
            "Authorization": f"Bearer {self._bearer_token}",
            "Content-Type": "application/json",
        }
        if org_id:
            headers["x-org-id"] = org_id
        return headers

    async def list_connections(self, org_id: str, provider_key: str) -> list[dict[str, Any]]:
        """List connections, optionally filtered by org + provider. An empty
        org_id lists across orgs (internal discovery)."""
        if not self.configured():
            raise ActionsGatewayError("actions gateway not configured")
        params: dict[str, str] = {}
        if org_id:
            params["organizationId"] = org_id
        if provider_key:
            params["providerKey"] = provider_key
        resp = await self._client.get(
            f"{self._base}/api/v1/connections",
            params=params,
            headers=self._headers(org_id),
            timeout=15.0,
        )
        resp.raise_for_status()
        env = resp.json()
        if not env.get("success", True):
            raise ActionsGatewayError(f"list connections failed: {env.get('error')}")
        data = env.get("data") or {}
        connections = data.get("connections") or []
        return [c for c in connections if isinstance(c, dict)]

    async def execute_action(
        self,
        connection_id: str,
        operation: str,
        params: dict[str, Any] | None = None,
        body: dict[str, Any] | None = None,
        org_id: str | None = None,
    ) -> Any:
        """Execute one action on a connection and return the provider result
        (the executor's pass-through of the provider response)."""
        if not self.configured():
            raise ActionsGatewayError("actions gateway not configured")
        payload: dict[str, Any] = {"operation": operation}
        if params:
            payload["params"] = params
        if body:
            payload["body"] = body
        resp = await self._client.post(
            f"{self._base}/api/v1/connections/{quote(connection_id, safe='')}/actions",
            json=payload,
            headers=self._headers(org_id),
            timeout=30.0,
        )
        resp.raise_for_status()
        env = resp.json()
        if not env.get("success", False):
            raise ActionsGatewayError(f"action {operation} failed: {env.get('error')}")
        action = (env.get("data") or {}).get("action") or {}
        return action.get("result")
