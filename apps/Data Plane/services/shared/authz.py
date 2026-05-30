"""
Data Plane authorization layer.

After auth middleware resolves a user, this module checks org membership,
role, permissions, and entitlements via org-core's CheckOrgAccess gRPC.

Results are cached in Redis (5 min TTL) and invalidated by NATS events
for member changes, plan changes, and entitlement updates.
"""
from __future__ import annotations

import json
import logging
from dataclasses import dataclass
from typing import Optional

import grpc

from shared.auth_middleware import AuthContext

logger = logging.getLogger(__name__)

# ── Org access result ──────────────────────────────────────────────────────────

@dataclass(frozen=True, slots=True)
class OrgAccess:
    allowed: bool
    role: str             # "owner" | "admin" | "member" | "viewer" | custom
    plan: str             # "free" | "professional" | "enterprise"
    permissions: tuple[str, ...]
    entitlements: tuple[str, ...]
    org_name: str
    org_status: str

    def has_permission(self, perm: str) -> bool:
        return perm in self.permissions

    def has_entitlement(self, key: str) -> bool:
        return key in self.entitlements


# ── Permission requirements per route ──────────────────────────────────────────

# Maps (method, path_prefix) → required permission
ROUTE_PERMISSIONS: dict[tuple[str, str], str] = {
    ("POST",   "/v1/documents"):  "resources:create",
    ("GET",    "/v1/documents"):   "resources:read",
    ("DELETE", "/v1/documents"):   "resources:delete",
    ("POST",   "/v1/retrieve"):    "resources:read",
}

# The knowledge_base entitlement is required for all data-plane operations
REQUIRED_ENTITLEMENT = "feature.knowledge_base"


# ── gRPC stub (lazy singleton) ────────────────────────────────────────────────

_org_grpc_channel: Optional[grpc.aio.Channel] = None
_org_grpc_stub: Optional[object] = None


async def _get_org_stub(org_grpc_url: str, internal_api_key: str):
    """Lazy-init async gRPC channel + stub for OrgAccessService."""
    global _org_grpc_channel, _org_grpc_stub
    if _org_grpc_stub is not None:
        return _org_grpc_stub

    from shared.grpc_runtime import load_org_access_proto_modules
    org_pb2, org_pb2_grpc = load_org_access_proto_modules()

    _org_grpc_channel = grpc.aio.insecure_channel(org_grpc_url)
    _org_grpc_stub = org_pb2_grpc.OrgAccessServiceStub(_org_grpc_channel)
    return _org_grpc_stub


async def close_org_channel() -> None:
    global _org_grpc_channel, _org_grpc_stub
    if _org_grpc_channel:
        await _org_grpc_channel.close()
        _org_grpc_channel = None
        _org_grpc_stub = None


# ── Redis cache helpers ────────────────────────────────────────────────────────

_AUTHZ_CACHE_TTL = 300  # 5 minutes


def _authz_cache_key(user_id: str, org_id: str) -> str:
    return f"authz:{user_id}:{org_id}"


async def _get_cached_access(redis_client, user_id: str, org_id: str) -> Optional[OrgAccess]:
    try:
        raw = await redis_client.get(_authz_cache_key(user_id, org_id))
        if raw is None:
            return None
        data = json.loads(raw)
        return OrgAccess(
            allowed=data["allowed"],
            role=data["role"],
            plan=data["plan"],
            permissions=tuple(data["permissions"]),
            entitlements=tuple(data["entitlements"]),
            org_name=data["org_name"],
            org_status=data["org_status"],
        )
    except Exception as exc:
        logger.debug("authz cache read failed (non-fatal): %s", exc)
        return None


async def _set_cached_access(redis_client, user_id: str, org_id: str, access: OrgAccess) -> None:
    try:
        payload = {
            "allowed": access.allowed,
            "role": access.role,
            "plan": access.plan,
            "permissions": list(access.permissions),
            "entitlements": list(access.entitlements),
            "org_name": access.org_name,
            "org_status": access.org_status,
        }
        await redis_client.set(
            _authz_cache_key(user_id, org_id),
            json.dumps(payload),
            ex=_AUTHZ_CACHE_TTL,
        )
    except Exception as exc:
        logger.debug("authz cache write failed (non-fatal): %s", exc)


# ── Core authorization check ──────────────────────────────────────────────────


async def check_org_access(
    auth_ctx: AuthContext,
    org_grpc_url: str,
    internal_api_key: str,
    redis_client=None,
) -> OrgAccess:
    """
    Check whether the authenticated user has access to the org in auth_ctx.org_id.

    1. Check Redis cache
    2. Call org-core CheckOrgAccess gRPC
    3. Cache result on success
    """
    if not auth_ctx.org_id:
        return OrgAccess(
            allowed=False,
            role="",
            plan="",
            permissions=(),
            entitlements=(),
            org_name="",
            org_status="",
        )

    # Cache lookup
    if redis_client:
        cached = await _get_cached_access(redis_client, auth_ctx.user_id, auth_ctx.org_id)
        if cached is not None:
            return cached

    # gRPC call to org-core
    try:
        stub = await _get_org_stub(org_grpc_url, internal_api_key)
        from shared.grpc_runtime import load_org_access_proto_modules
        org_pb2, _ = load_org_access_proto_modules()

        metadata = [("x-service-auth", internal_api_key)]
        resp = await stub.CheckOrgAccess(
            org_pb2.CheckOrgAccessRequest(
                user_id=auth_ctx.user_id,
                org_id=auth_ctx.org_id,
            ),
            metadata=metadata,
            timeout=5.0,
        )
    except grpc.aio.AioRpcError as exc:
        logger.error("org-core gRPC unreachable: %s", exc)
        # Fail closed — deny access if org-core is down
        return OrgAccess(
            allowed=False,
            role="",
            plan="",
            permissions=(),
            entitlements=(),
            org_name="",
            org_status="",
        )

    access = OrgAccess(
        allowed=resp.allowed,
        role=resp.role,
        plan=resp.plan,
        permissions=tuple(resp.permissions),
        entitlements=tuple(resp.entitlements),
        org_name=resp.org_name,
        org_status=resp.org_status,
    )

    # Cache successful result
    if redis_client and access.allowed:
        await _set_cached_access(redis_client, auth_ctx.user_id, auth_ctx.org_id, access)

    return access


def check_route_permission(
    access: OrgAccess,
    method: str,
    path: str,
) -> Optional[str]:
    """
    Check if the user's org role has the required permission for the route.

    Returns None if authorized, or a human-readable denial reason string.
    """
    if not access.allowed:
        return "User is not a member of this organization"

    if access.org_status != "active":
        return f"Organization is {access.org_status}"

    # Check entitlements
    if REQUIRED_ENTITLEMENT and not access.has_entitlement(REQUIRED_ENTITLEMENT):
        return (
            f"Organization does not have the '{REQUIRED_ENTITLEMENT}' entitlement. "
            "Contact your admin to enable this feature."
        )

    # Check route-level permission
    for (req_method, req_prefix), perm in ROUTE_PERMISSIONS.items():
        if method == req_method and path.startswith(req_prefix):
            if not access.has_permission(perm):
                return (
                    f"Permission '{perm}' required. "
                    f"Your role '{access.role}' does not have this permission."
                )
            break

    return None  # Authorized
