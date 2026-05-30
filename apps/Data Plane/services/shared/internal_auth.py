"""Shared dependency for internal service-to-service authentication."""

from __future__ import annotations

import hmac
import logging
from dataclasses import dataclass
from typing import Optional

from fastapi import Header, HTTPException, status

logger = logging.getLogger(__name__)


@dataclass(frozen=True, slots=True)
class InternalAuthContext:
    """Verified internal caller identity."""

    org_id: str
    service_name: str = "unknown"


def _validate_key(provided: str, expected: str) -> bool:
    """Constant-time comparison to prevent timing attacks."""
    return hmac.compare_digest(provided.encode(), expected.encode())


async def require_internal_auth(
    x_internal_api_key: Optional[str] = Header(None, alias="X-Internal-Api-Key"),
    x_service_auth: Optional[str] = Header(None, alias="X-Service-Auth"),
    x_org_id: Optional[str] = Header(None, alias="X-Org-Id"),
    x_service_name: Optional[str] = Header(None, alias="X-Service-Name"),
) -> InternalAuthContext:
    """FastAPI dependency — validates internal service auth headers."""
    from app.config import settings

    if not settings.internal_api_key:
        raise HTTPException(
            status_code=status.HTTP_503_SERVICE_UNAVAILABLE,
            detail="Internal auth not configured",
        )

    provided = x_internal_api_key or x_service_auth
    if not provided or not _validate_key(provided, settings.internal_api_key):
        raise HTTPException(
            status_code=status.HTTP_403_FORBIDDEN,
            detail="Invalid internal auth key",
        )

    if not x_org_id:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="X-Org-Id header required",
        )

    return InternalAuthContext(
        org_id=x_org_id,
        service_name=x_service_name or "unknown",
    )
