"""Org-core entitlement gate — checks Control Plane org quotas before execution.

Calls org-core ``GET /api/v1/orgs/{org_id}/entitlements`` to validate:
1. The org's plan allows agent runs (``agent.enabled`` entitlement)
2. Usage quota hasn't been exceeded for the billing period
3. The specific agent features requested are included in the plan

Falls back gracefully if org-core is unreachable (allows in dev mode,
denies in production to fail safe).
"""

from __future__ import annotations

import logging
from dataclasses import dataclass
from typing import Any

import httpx

from app.config import settings

logger = logging.getLogger(__name__)

_CLIENT: httpx.AsyncClient | None = None


@dataclass(frozen=True)
class EntitlementResult:
    """Result of an entitlement check."""

    allowed: bool
    reason: str = ""
    remaining_quota: int | None = None
    plan_name: str = ""


async def _get_client() -> httpx.AsyncClient:
    global _CLIENT
    if _CLIENT is None or _CLIENT.is_closed:
        _CLIENT = httpx.AsyncClient(timeout=5.0)
    return _CLIENT


async def check_org_entitlements(
    org_id: str,
    feature: str = "agent.enabled",
) -> EntitlementResult:
    """Check if the org is entitled to use a specific feature.

    Args:
        org_id: Organisation ID
        feature: Entitlement key to check (e.g. "agent.enabled", "agent.streaming")

    Returns:
        EntitlementResult with allowed flag and details.
    """
    if not org_id or org_id in ("system", "dev"):
        return EntitlementResult(allowed=True, reason="System/dev org — bypassed")

    org_core_url = settings.org_core_url
    if not org_core_url:
        # No org-core configured — allow in dev, deny in prod
        if settings.environment in ("development", "local", "test"):
            return EntitlementResult(allowed=True, reason="Org-core not configured (dev mode)")
        return EntitlementResult(allowed=False, reason="Org-core not configured")

    client = await _get_client()
    try:
        resp = await client.get(
            f"{org_core_url}/api/v1/orgs/{org_id}/entitlements",
            headers=_auth_headers(),
        )
        if resp.status_code == 404:
            return EntitlementResult(allowed=False, reason=f"Org {org_id} not found")
        resp.raise_for_status()
        data: dict[str, Any] = resp.json()
    except httpx.ConnectError:
        logger.warning("org_core_unreachable", extra={"url": org_core_url})
        if settings.environment in ("development", "local", "test"):
            return EntitlementResult(allowed=True, reason="Org-core unreachable (dev fallback)")
        return EntitlementResult(allowed=False, reason="Org-core unreachable")
    except Exception as exc:
        logger.error("entitlement_check_error", extra={"error": str(exc)})
        return EntitlementResult(allowed=False, reason=f"Entitlement check failed: {exc}")

    # Parse entitlements response from org-core
    entitlements = data.get("entitlements", {})
    plan_name = data.get("plan", {}).get("name", "unknown")

    if not entitlements.get(feature, False):
        return EntitlementResult(
            allowed=False,
            reason=f"Feature '{feature}' not included in plan '{plan_name}'",
            plan_name=plan_name,
        )

    # Check quota if available
    quotas = data.get("quotas", {})
    agent_quota = quotas.get("agent_runs", {})
    if agent_quota:
        used = agent_quota.get("used", 0)
        limit = agent_quota.get("limit", -1)
        if limit > 0 and used >= limit:
            return EntitlementResult(
                allowed=False,
                reason=f"Agent run quota exceeded ({used}/{limit})",
                remaining_quota=0,
                plan_name=plan_name,
            )
        remaining = max(0, limit - used) if limit > 0 else None
        return EntitlementResult(
            allowed=True,
            remaining_quota=remaining,
            plan_name=plan_name,
        )

    return EntitlementResult(allowed=True, plan_name=plan_name)


async def check_org_quota(org_id: str) -> EntitlementResult:
    """Shorthand — check both feature access and quota in one call."""
    return await check_org_entitlements(org_id, feature="agent.enabled")


def _auth_headers() -> dict[str, str]:
    """Build internal auth headers for org-core requests."""
    headers: dict[str, str] = {}
    if settings.internal_api_key:
        headers["x-internal-api-key"] = settings.internal_api_key
    return headers


async def close_entitlement_client() -> None:
    """Shutdown hook."""
    global _CLIENT
    if _CLIENT and not _CLIENT.is_closed:
        await _CLIENT.aclose()
        _CLIENT = None
