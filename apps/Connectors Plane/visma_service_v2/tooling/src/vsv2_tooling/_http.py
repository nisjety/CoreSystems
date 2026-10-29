"""Shared HTTP / OAuth helpers for the Phase 0 Python tooling.

Phase 0 runs *before* the Rust binary is wired, so we duplicate just enough
auth + GraphQL POST logic here in Python. After Phase 1, the Rust crates
own production traffic; this module remains for ad-hoc operator scripts.
"""

from __future__ import annotations

import os
import time
from dataclasses import dataclass
from typing import Any

import httpx
from dotenv import load_dotenv


class ConfigError(RuntimeError):
    pass


def _env(key: str, *, default: str | None = None, required: bool = False) -> str | None:
    value = os.environ.get(key, default)
    if required and not value:
        raise ConfigError(f"missing env var: {key}")
    return value


def _first_env(keys: list[str], *, required: bool = False) -> str | None:
    for key in keys:
        value = _env(key)
        if value:
            return value
    if required:
        raise ConfigError(f"missing env var: one of {', '.join(keys)}")
    return None


@dataclass(frozen=True)
class VismaEnv:
    client_id: str
    client_secret: str
    token_url: str
    scope_erp: str
    scope_dm: str
    erp_base: str
    datamart_url: str
    tenant_id: str
    company_id: str | None

    @classmethod
    def from_env(cls) -> VismaEnv:
        load_dotenv()
        erp_scope = _first_env(["VISMA_SCOPES", "VISMA_OAUTH_SCOPE"], required=True)
        return cls(
            client_id=_env("VISMA_CLIENT_ID", required=True),  # type: ignore[arg-type]
            client_secret=_env("VISMA_CLIENT_SECRET", required=True),  # type: ignore[arg-type]
            token_url=_env(
                "VISMA_TOKEN_URL",
                default="https://connect.visma.com/connect/token",
            ),  # type: ignore[arg-type]
            scope_erp=erp_scope,  # type: ignore[arg-type]
            scope_dm=_env("VISMA_OAUTH_SCOPE_DATAMART") or erp_scope,  # type: ignore[arg-type]
            erp_base=_env(
                "VISMA_ERP_REST_BASE_URL",
                default="https://api.finance.visma.net",
            ),  # type: ignore[arg-type]
            datamart_url=_env(
                "VISMA_DATAMART_GRAPHQL_URL",
                default="https://datamart.visma.net/graphql",
            ),  # type: ignore[arg-type]
            tenant_id=_env(
                "VSV2_PILOT_TENANT_ID",
                default="33f319b9-4d57-11ee-960c-025417856183",
            ),  # type: ignore[arg-type]
            company_id=_env("VSV2_AQUATIQ_DATAMART_COMPANY_ID"),
        )


@dataclass
class _CachedToken:
    access_token: str
    expires_at: float  # epoch seconds


class OAuth:
    """Minimal Visma Connect client_credentials helper for Phase 0."""

    def __init__(self, env: VismaEnv, *, timeout: float = 30.0) -> None:
        self.env = env
        self._client = httpx.Client(timeout=timeout)
        self._cache: dict[tuple[str, str], _CachedToken] = {}

    def token_for(self, *, scope: str, tenant_id: str | None = None) -> str:
        tenant = tenant_id or self.env.tenant_id
        key = (tenant, scope)
        cached = self._cache.get(key)
        now = time.time()
        if cached and cached.expires_at - 60 > now:
            return cached.access_token

        resp = self._client.post(
            self.env.token_url,
            data={
                "grant_type": "client_credentials",
                "client_id": self.env.client_id,
                "client_secret": self.env.client_secret,
                "scope": scope,
                "tenant_id": tenant,
            },
        )
        if resp.status_code >= 400:
            raise RuntimeError(f"OAuth {resp.status_code}: {resp.text}")
        body = resp.json()
        token = body["access_token"]
        ttl = int(body.get("expires_in", 3600))
        self._cache[key] = _CachedToken(token, now + ttl)
        return token

    def close(self) -> None:
        self._client.close()


class DatamartGraphQL:
    """Minimal GraphQL client for the DataMart endpoint."""

    def __init__(self, env: VismaEnv, oauth: OAuth, *, timeout: float = 60.0) -> None:
        self.env = env
        self.oauth = oauth
        self._client = httpx.Client(timeout=timeout)

    def execute(
        self,
        query: str,
        variables: dict[str, Any] | None = None,
        *,
        tenant_id: str | None = None,
    ) -> dict[str, Any]:
        token = self.oauth.token_for(scope=self.env.scope_dm, tenant_id=tenant_id)
        resp = self._client.post(
            self.env.datamart_url,
            headers={"Authorization": f"Bearer {token}"},
            json={"query": query, "variables": variables or {}},
        )
        if resp.status_code >= 400:
            raise RuntimeError(f"DataMart {resp.status_code}: {resp.text}")
        body = resp.json()
        if errors := body.get("errors"):
            raise RuntimeError(f"DataMart GraphQL errors: {errors}")
        return body.get("data") or {}

    def close(self) -> None:
        self._client.close()
