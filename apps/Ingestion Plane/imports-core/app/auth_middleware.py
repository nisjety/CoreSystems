"""Audience-scoped authentication for imports-core HTTP boundaries."""

import asyncio
import hmac
import logging
import time
from dataclasses import dataclass, field, replace
from typing import Any, Optional

import httpx
import jwt
from fastapi import HTTPException, Request

from app.config import get_settings

logger = logging.getLogger(__name__)
_JWKS_TTL_SECONDS = 300.0
_jwks_cache: tuple[dict[str, Any], float] | None = None
_jwks_lock = asyncio.Lock()
_jwks_last_forced_at = 0.0


@dataclass(frozen=True)
class AuthContext:
    """Canonical immutable identity derived from a verified token."""

    org_id: str
    user_id: Optional[str] = None
    service_name: Optional[str] = None
    principal_type: str = "user"
    scopes: tuple[str, ...] = ()
    bearer_token: Optional[str] = field(default=None, repr=False, compare=False)

    def has_scope(self, scope: str) -> bool:
        return scope in self.scopes


class UnknownSigningKey(ValueError):
    pass


def _verify_token(token: str, jwks: dict[str, Any], settings: Any) -> AuthContext:
    """Verify RS256 signature and canonical ingestion-audience claims."""
    try:
        header = jwt.get_unverified_header(token)
    except jwt.PyJWTError as exc:
        raise ValueError("token header is invalid") from exc
    if header.get("alg") != "RS256" or not isinstance(header.get("kid"), str):
        raise ValueError("token signing metadata is invalid")

    matching = [
        key
        for key in jwks.get("keys", [])
        if isinstance(key, dict)
        and key.get("kid") == header["kid"]
        and key.get("kty") == "RSA"
        and key.get("alg") == "RS256"
        and key.get("use", "sig") == "sig"
    ]
    if len(matching) != 1:
        raise UnknownSigningKey("token signing key is unknown")

    try:
        key = jwt.PyJWK.from_dict(matching[0]).key
        claims = jwt.decode(
            token,
            key,
            algorithms=["RS256"],
            issuer=settings.plane_token_issuer,
            audience=settings.ingestion_auth_audience,
            leeway=30,
            options={"require": ["exp", "iat", "nbf", "iss", "aud", "sub"]},
        )
    except (jwt.PyJWTError, ValueError, TypeError) as exc:
        raise ValueError("token verification failed") from exc

    org_id = claims.get("org_id")
    principal_type = claims.get("principal_type")
    subject = claims.get("sub")
    scopes_value = claims.get("scopes", [])
    if (
        not isinstance(org_id, str)
        or not org_id.strip()
        or principal_type not in {"user", "service"}
        or not isinstance(subject, str)
        or not subject.strip()
        or not isinstance(scopes_value, list)
        or any(not isinstance(scope, str) or not scope for scope in scopes_value)
    ):
        raise ValueError("token identity claims are invalid")

    scopes = tuple(scopes_value)
    if principal_type == "user":
        user_id = claims.get("user_id")
        if not isinstance(user_id, str) or user_id != subject:
            raise ValueError("token user identity is not canonical")
        return AuthContext(
            org_id=org_id.strip(),
            user_id=user_id,
            principal_type="user",
            scopes=scopes,
        )

    service_id = claims.get("service_id")
    if (
        not isinstance(service_id, str)
        or service_id != subject
        or not ({"imports:read", "imports:write"} & set(scopes))
    ):
        raise ValueError("token service identity is not canonical")
    return AuthContext(
        org_id=org_id.strip(),
        service_name=service_id,
        principal_type="service",
        scopes=scopes,
    )


async def _load_jwks(settings: Any, *, force: bool = False) -> dict[str, Any]:
    global _jwks_cache, _jwks_last_forced_at
    now = time.monotonic()
    if force and _jwks_cache is not None and now - _jwks_last_forced_at < 30.0:
        return _jwks_cache[0]
    if not force and _jwks_cache is not None and now < _jwks_cache[1]:
        return _jwks_cache[0]

    async with _jwks_lock:
        now = time.monotonic()
        if force and _jwks_cache is not None and now - _jwks_last_forced_at < 30.0:
            return _jwks_cache[0]
        if not force and _jwks_cache is not None and now < _jwks_cache[1]:
            return _jwks_cache[0]
        try:
            async with httpx.AsyncClient(timeout=5.0, follow_redirects=False) as client:
                response = await client.get(settings.auth_core_jwks_url.strip())
                response.raise_for_status()
                if len(response.content) > 1024 * 1024:
                    raise ValueError("JWKS response exceeds 1 MiB")
                document = response.json()
        except (httpx.HTTPError, ValueError) as exc:
            raise RuntimeError("auth verification keys unavailable") from exc
        if not isinstance(document, dict) or not isinstance(document.get("keys"), list):
            raise RuntimeError("auth verification keys are invalid")
        _jwks_cache = (document, now + _JWKS_TTL_SECONDS)
        if force:
            _jwks_last_forced_at = now
        return document


async def authentication_ready() -> bool:
    try:
        await _load_jwks(get_settings())
        return True
    except RuntimeError:
        return False


def _legacy_auth_enabled(settings: Any) -> bool:
    return bool(
        settings.allow_legacy_tenant_key
        and settings.allow_insecure_dev_defaults
        and settings.isolated_e2e
    )


def _legacy_context(request: Request, settings: Any) -> AuthContext | None:
    if not _legacy_auth_enabled(settings):
        return None
    provided = request.headers.get("x-internal-api-key") or request.headers.get("x-service-auth")
    if not provided or not hmac.compare_digest(provided, settings.internal_api_key):
        return None
    org_id = request.headers.get("x-org-id", "").strip()
    if not org_id:
        return None
    return AuthContext(
        org_id=org_id,
        service_name=request.headers.get("x-service-name") or "isolated-e2e",
        principal_type="service",
        scopes=("imports:read", "imports:write"),
    )


async def _bearer_context(request: Request, settings: Any) -> AuthContext:
    authorization = request.headers.get("authorization", "")
    scheme, separator, token = authorization.partition(" ")
    if separator != " " or scheme.lower() != "bearer" or not token.strip():
        raise HTTPException(status_code=401, detail="Bearer token required")

    try:
        jwks = await _load_jwks(settings)
        try:
            principal = _verify_token(token.strip(), jwks, settings)
        except UnknownSigningKey:
            principal = _verify_token(token.strip(), await _load_jwks(settings, force=True), settings)
    except RuntimeError as exc:
        raise HTTPException(status_code=503, detail="Authentication service unavailable") from exc
    except ValueError as exc:
        raise HTTPException(status_code=401, detail="Invalid bearer token") from exc

    supplied_org = request.headers.get("x-org-id")
    supplied_user = request.headers.get("x-user-id")
    if supplied_org and supplied_org.strip() != principal.org_id:
        raise HTTPException(status_code=403, detail="Tenant header conflicts with signed identity")
    if supplied_user and supplied_user.strip() != (principal.user_id or principal.service_name):
        raise HTTPException(status_code=403, detail="Actor header conflicts with signed identity")

    if principal.principal_type == "service":
        required = "imports:read" if request.method in {"GET", "HEAD", "OPTIONS"} else "imports:write"
        if not principal.has_scope(required) and not principal.has_scope("imports:write"):
            raise HTTPException(status_code=403, detail=f"Missing required scope: {required}")
    return replace(principal, bearer_token=token.strip())


async def require_internal_auth(request: Request) -> AuthContext:
    """Require a verified ingestion token; legacy shared keys are E2E-only."""
    settings = get_settings()
    if request.headers.get("authorization"):
        return await _bearer_context(request, settings)
    legacy = _legacy_context(request, settings)
    if legacy is not None:
        return legacy
    raise HTTPException(status_code=401, detail="Bearer token required")


async def optional_internal_auth(request: Request) -> Optional[AuthContext]:
    try:
        return await require_internal_auth(request)
    except HTTPException:
        return None
