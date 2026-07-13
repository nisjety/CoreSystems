"""Imports endpoints derive tenant identity from signed audience tokens."""

import asyncio
import base64
import time
from types import SimpleNamespace

import jwt
import pytest
from cryptography.hazmat.primitives.asymmetric import rsa
from fastapi import HTTPException
from starlette.requests import Request

from app import auth_middleware
from app.auth_middleware import _verify_token, require_internal_auth


def _b64uint(value: int) -> str:
    size = (value.bit_length() + 7) // 8
    return base64.urlsafe_b64encode(value.to_bytes(size, "big")).rstrip(b"=").decode()


def _fixture() -> tuple[object, dict, SimpleNamespace]:
    private_key = rsa.generate_private_key(public_exponent=65537, key_size=2048)
    public = private_key.public_key().public_numbers()
    jwks = {
        "keys": [
            {
                "kid": "test-key",
                "kty": "RSA",
                "alg": "RS256",
                "use": "sig",
                "n": _b64uint(public.n),
                "e": _b64uint(public.e),
            }
        ]
    }
    settings = SimpleNamespace(
        plane_token_issuer="http://auth-core/api/convex-auth",
        ingestion_auth_audience="ingestion",
    )
    return private_key, jwks, settings


def _token(private_key, **overrides) -> str:
    now = int(time.time())
    claims = {
        "iss": "http://auth-core/api/convex-auth",
        "aud": "ingestion",
        "sub": "user-1",
        "org_id": "org-1",
        "user_id": "user-1",
        "principal_type": "user",
        "scopes": [],
        "iat": now,
        "nbf": now,
        "exp": now + 300,
    }
    claims.update(overrides)
    return jwt.encode(claims, private_key, algorithm="RS256", headers={"kid": "test-key"})


def test_valid_token_provides_canonical_tenant_identity() -> None:
    private_key, jwks, settings = _fixture()

    principal = _verify_token(_token(private_key), jwks, settings)

    assert principal.org_id == "org-1"
    assert principal.user_id == "user-1"
    assert principal.principal_type == "user"


@pytest.mark.parametrize(
    "overrides",
    [
        {"aud": "data-plane"},
        {"org_id": ""},
        {"sub": "attacker"},
        {"principal_type": "service", "service_id": "svc", "sub": "svc", "scopes": []},
    ],
)
def test_rejects_wrong_audience_or_noncanonical_identity(overrides: dict) -> None:
    private_key, jwks, settings = _fixture()

    with pytest.raises(ValueError):
        _verify_token(_token(private_key, **overrides), jwks, settings)


def test_shared_tenant_key_is_disabled_by_default(monkeypatch: pytest.MonkeyPatch) -> None:
    settings = SimpleNamespace(
        internal_api_key="legacy-key",
        allow_legacy_tenant_key=False,
        allow_insecure_dev_defaults=False,
        isolated_e2e=False,
    )
    monkeypatch.setattr(auth_middleware, "get_settings", lambda: settings)
    request = Request(
        {
            "type": "http",
            "method": "POST",
            "path": "/api/v1/import/jobs/source",
            "headers": [
                (b"x-internal-api-key", b"legacy-key"),
                (b"x-org-id", b"victim-org"),
            ],
        }
    )

    with pytest.raises(HTTPException) as exc:
        asyncio.run(require_internal_auth(request))

    assert exc.value.status_code == 401
