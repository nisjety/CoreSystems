"""Cross-plane authentication middleware.

Supports two modes:
1. **Bearer JWT** — validates against Control Plane auth-core session endpoint.
   Returns a Principal with user_id, org_id, and roles from the JWT/session.
2. **Internal API key** — service-to-service calls using X-Internal-Api-Key header.
   Returns a system-level Principal with full permissions.

Pattern ported from Ingestion Plane's ``requireInternalOrBearerAuth()`` middleware.
"""

from __future__ import annotations

import logging
from dataclasses import dataclass, field
from enum import Enum
from typing import Any

import httpx
from fastapi import Depends, HTTPException, Request

from app.config import settings

logger = logging.getLogger(__name__)

_AUTH_SESSION_URL = ""
_AUTH_CLIENT: httpx.AsyncClient | None = None


class Role(str, Enum):
    OWNER = "owner"
    ADMIN = "admin"
    MEMBER = "member"
    VIEWER = "viewer"
    SYSTEM = "system"

    @property
    def rank(self) -> int:
        return {"system": 100, "owner": 40, "admin": 30, "member": 20, "viewer": 10}[
            self.value
        ]

    def __ge__(self, other: "Role") -> bool:  # type: ignore[override]
        return self.rank >= other.rank

    def __gt__(self, other: "Role") -> bool:  # type: ignore[override]
        return self.rank > other.rank

    def __le__(self, other: "Role") -> bool:  # type: ignore[override]
        return self.rank <= other.rank

    def __lt__(self, other: "Role") -> bool:  # type: ignore[override]
        return self.rank < other.rank


@dataclass(frozen=True)
class Principal:
    """Authenticated identity extracted from JWT or internal key."""

    user_id: str
    org_id: str
    roles: tuple[Role, ...] = (Role.MEMBER,)
    is_internal: bool = False

    @property
    def highest_role(self) -> Role:
        return max(self.roles, default=Role.VIEWER)

    def has_role(self, required: Role) -> bool:
        return self.highest_role >= required


def _get_auth_url() -> str:
    """Resolve auth-core session validation URL."""
    global _AUTH_SESSION_URL
    if not _AUTH_SESSION_URL:
        base = settings.auth_core_url.rstrip("/") if settings.auth_core_url else ""
        _AUTH_SESSION_URL = f"{base}/api/auth/get-session"
    return _AUTH_SESSION_URL


async def _get_http_client() -> httpx.AsyncClient:
    global _AUTH_CLIENT
    if _AUTH_CLIENT is None or _AUTH_CLIENT.is_closed:
        _AUTH_CLIENT = httpx.AsyncClient(timeout=5.0)
    return _AUTH_CLIENT


async def _validate_bearer_token(token: str) -> Principal:
    """Validate a Bearer token against auth-core and return a Principal."""
    url = _get_auth_url()
    if not url or url.startswith("//"):
        raise HTTPException(
            status_code=503,
            detail="Auth service not configured (AUTH_CORE_URL missing)",
        )

    client = await _get_http_client()
    try:
        resp = await client.get(
            url,
            headers={
                "Authorization": f"Bearer {token}",
                "Cookie": f"better-auth.session_token={token}",
            },
        )
    except httpx.ConnectError:
        logger.error("auth_core_unreachable", extra={"url": url})
        raise HTTPException(status_code=503, detail="Auth service unreachable")

    if resp.status_code != 200:
        raise HTTPException(status_code=401, detail="Invalid or expired token")

    data: dict[str, Any] = resp.json()
    session = data.get("session", {})
    user = data.get("user", {})

    user_id = user.get("id", "")
    if not user_id:
        raise HTTPException(status_code=401, detail="Invalid session: no user")

    # Resolve org from active organization membership
    active_org_id = session.get("activeOrganizationId", "")
    roles_raw: list[str] = []
    for membership in user.get("memberships", []):
        if membership.get("organizationId") == active_org_id:
            roles_raw.append(membership.get("role", "member"))

    roles = tuple(
        Role(r) for r in roles_raw if r in Role.__members__.values()
    ) or (Role.MEMBER,)

    return Principal(
        user_id=user_id,
        org_id=active_org_id,
        roles=roles,
        is_internal=False,
    )


def _validate_internal_key(provided_key: str) -> Principal:
    """Validate an internal API key for service-to-service calls."""
    if not settings.internal_api_key:
        raise HTTPException(
            status_code=503, detail="Internal auth not configured"
        )

    if provided_key != settings.internal_api_key:
        raise HTTPException(status_code=401, detail="Invalid internal API key")

    return Principal(
        user_id="system",
        org_id="system",
        roles=(Role.SYSTEM,),
        is_internal=True,
    )


async def get_principal(request: Request) -> Principal:
    """FastAPI dependency — extract Principal from request headers.

    Priority:
    1. Authorization: Bearer <jwt> — validate against auth-core
    2. X-Internal-Api-Key: <key> — service-to-service
    3. No auth configured (dev mode) — allow with default principal
    """
    auth_header = request.headers.get("Authorization", "")
    internal_key = request.headers.get("X-Internal-Api-Key", "")

    # Bearer token takes precedence
    if auth_header.startswith("Bearer "):
        token = auth_header[7:].strip()
        if token:
            principal = await _validate_bearer_token(token)
            # Store on request.state for downstream access
            request.state.principal = principal
            request.state.org_id = principal.org_id
            return principal

    # Internal API key fallback
    if internal_key:
        principal = _validate_internal_key(internal_key)
        # For internal calls, accept X-Org-Id header as org override
        org_override = request.headers.get("X-Org-Id", "")
        if org_override:
            principal = Principal(
                user_id=principal.user_id,
                org_id=org_override,
                roles=principal.roles,
                is_internal=True,
            )
        request.state.principal = principal
        request.state.org_id = principal.org_id
        return principal

    # Dev mode: no auth configured → allow through
    if not settings.internal_api_key and not settings.auth_core_url:
        principal = Principal(
            user_id="dev-user",
            org_id=request.headers.get("X-Org-Id", "dev"),
            roles=(Role.OWNER,),
            is_internal=True,
        )
        request.state.principal = principal
        request.state.org_id = principal.org_id
        return principal

    raise HTTPException(
        status_code=401,
        detail="Missing Authorization header or X-Internal-Api-Key",
    )


def require_role(min_role: Role):
    """FastAPI dependency factory — reject requests below the required role."""

    async def _check(principal: Principal = Depends(get_principal)) -> Principal:
        if not principal.has_role(min_role):
            raise HTTPException(
                status_code=403,
                detail=f"Requires at least {min_role.value} role",
            )
        return principal

    return _check


async def close_auth_client() -> None:
    """Shutdown hook — close the httpx client."""
    global _AUTH_CLIENT
    if _AUTH_CLIENT and not _AUTH_CLIENT.is_closed:
        await _AUTH_CLIENT.aclose()
        _AUTH_CLIENT = None
