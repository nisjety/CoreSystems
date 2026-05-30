"""Authentication middleware package."""

from app.middleware._legacy_internal import InternalAuthMiddleware
from app.middleware.auth import (
    Principal,
    Role,
    close_auth_client,
    get_principal,
    require_role,
)

__all__ = [
    "InternalAuthMiddleware",
    "Principal",
    "Role",
    "close_auth_client",
    "get_principal",
    "require_role",
]
