"""
Authentication middleware for imports-core.

Validates service-to-service requests via X-Internal-Api-Key header.
All internal services in the Velion SaaS platform use a shared internal
API key for mutual authentication — no JWT/Bearer tokens needed for
intra-platform calls.

Usage in routes:
    from app.auth_middleware import require_internal_auth, get_auth_context

    @app.post("/api/v1/import/jobs/upload")
    async def upload(auth: AuthContext = Depends(require_internal_auth)):
        org_id = auth.org_id  # validated from request headers
"""

import hmac
import logging
from dataclasses import dataclass
from typing import Optional

from fastapi import Depends, HTTPException, Request

from app.config import get_settings

logger = logging.getLogger(__name__)


@dataclass(frozen=True)
class AuthContext:
    """Immutable auth context extracted from validated request."""

    org_id: str
    user_id: Optional[str] = None
    service_name: Optional[str] = None


def _extract_internal_key(request: Request) -> Optional[str]:
    """Extract internal API key from standard headers."""
    return (
        request.headers.get("x-internal-api-key")
        or request.headers.get("x-service-auth")
    )


def _validate_internal_key(provided: str) -> bool:
    """Constant-time comparison of internal API key."""
    settings = get_settings()
    return hmac.compare_digest(provided, settings.internal_api_key)


async def require_internal_auth(request: Request) -> AuthContext:
    """FastAPI dependency: validates X-Internal-Api-Key header.

    Extracts org_id from X-Org-Id header (required) and optional
    X-User-Id / X-Service-Name headers.

    Raises:
        HTTPException 401: Missing or invalid API key.
        HTTPException 400: Missing required X-Org-Id header.
    """
    api_key = _extract_internal_key(request)
    if not api_key:
        raise HTTPException(
            status_code=401,
            detail="Missing X-Internal-Api-Key header",
        )

    if not _validate_internal_key(api_key):
        logger.warning("Invalid internal API key from %s", request.client.host if request.client else "unknown")
        raise HTTPException(
            status_code=401,
            detail="Invalid internal API key",
        )

    org_id = request.headers.get("x-org-id")
    if not org_id:
        raise HTTPException(
            status_code=400,
            detail="Missing required X-Org-Id header",
        )

    return AuthContext(
        org_id=org_id,
        user_id=request.headers.get("x-user-id"),
        service_name=request.headers.get("x-service-name"),
    )


async def optional_internal_auth(request: Request) -> Optional[AuthContext]:
    """FastAPI dependency: validates API key if present, returns None otherwise.

    Use for endpoints that work both authenticated and unauthenticated
    (e.g., health checks with optional metadata).
    """
    api_key = _extract_internal_key(request)
    if not api_key:
        return None

    if not _validate_internal_key(api_key):
        return None

    org_id = request.headers.get("x-org-id")
    if not org_id:
        return None

    return AuthContext(
        org_id=org_id,
        user_id=request.headers.get("x-user-id"),
        service_name=request.headers.get("x-service-name"),
    )
