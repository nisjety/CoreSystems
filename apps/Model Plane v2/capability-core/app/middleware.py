"""Internal API key authentication middleware."""

from __future__ import annotations

import logging

from fastapi import Request, Response
from starlette.middleware.base import BaseHTTPMiddleware, RequestResponseEndpoint

from app.config import settings

logger = logging.getLogger(__name__)

_PUBLIC_PATHS = frozenset({"/health", "/docs", "/openapi.json"})


class InternalAuthMiddleware(BaseHTTPMiddleware):
    async def dispatch(
        self, request: Request, call_next: RequestResponseEndpoint
    ) -> Response:
        if request.url.path in _PUBLIC_PATHS:
            return await call_next(request)

        if not settings.internal_api_key:
            return await call_next(request)

        provided = request.headers.get("X-Internal-Api-Key", "")
        if provided != settings.internal_api_key:
            logger.warning("auth_rejected", extra={"path": request.url.path})
            return Response(
                content='{"error":"unauthorized"}',
                status_code=401,
                media_type="application/json",
            )

        return await call_next(request)
