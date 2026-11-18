"""The insecure-dev switch opens only for the exact literal 1, as Model Plane and the BFF read it."""

import asyncio

import pytest
from fastapi import HTTPException
from starlette.requests import Request

from app import auth_middleware
from app.config import Settings

# A placeholder shared key of the kind a copied example config carries. The legacy tenant path
# must refuse it unless the insecure-dev switch is exactly 1.
PLACEHOLDER_KEY = "change-me-internal-service-secret"

_SWITCHES = ("ALLOW_INSECURE_DEV_DEFAULTS", "ALLOW_LEGACY_TENANT_KEY", "ISOLATED_E2E")


def _settings(monkeypatch: pytest.MonkeyPatch, **switches: str) -> Settings:
    """Build Settings from the process environment only, so no .env file is read."""
    for name in _SWITCHES:
        monkeypatch.delenv(name, raising=False)
    env = {
        "DATABASE_URL": "postgresql://imports-test.invalid/imports",
        "DOCUMENT_SERVICE_URL": "http://documents.invalid",
        "ORG_SERVICE_URL": "http://org.invalid",
        "INTERNAL_API_KEY": PLACEHOLDER_KEY,
        **switches,
    }
    for name, value in env.items():
        monkeypatch.setenv(name, value)
    return Settings(_env_file=None)


def _legacy_request() -> Request:
    return Request(
        {
            "type": "http",
            "method": "POST",
            "path": "/api/v1/import/jobs/source",
            "headers": [
                (b"x-internal-api-key", PLACEHOLDER_KEY.encode()),
                (b"x-org-id", b"victim-org"),
            ],
        }
    )


@pytest.mark.parametrize(
    ("value", "opened"),
    [
        ("1", True),
        ("true", False),
        ("TRUE", False),
        ("True", False),
        ("yes", False),
        ("on", False),
        ("t", False),
        ("y", False),
        ("0", False),
        ("false", False),
        ("", False),
        (" 1", False),
        ("1 ", False),
    ],
)
def test_allow_insecure_dev_defaults_opens_only_for_the_literal_one(
    monkeypatch: pytest.MonkeyPatch, value: str, opened: bool
) -> None:
    settings = _settings(monkeypatch, ALLOW_INSECURE_DEV_DEFAULTS=value)

    assert settings.allow_insecure_dev_defaults is opened


@pytest.mark.parametrize("value", [None, "0", "true", "TRUE", "yes", "on", "t", "y"])
def test_legacy_tenant_key_refuses_the_placeholder_unless_the_switch_is_the_literal_one(
    monkeypatch: pytest.MonkeyPatch, value: str | None
) -> None:
    switches = {"ALLOW_LEGACY_TENANT_KEY": "1", "ISOLATED_E2E": "1"}
    if value is not None:
        switches["ALLOW_INSECURE_DEV_DEFAULTS"] = value
    settings = _settings(monkeypatch, **switches)
    monkeypatch.setattr(auth_middleware, "get_settings", lambda: settings)

    with pytest.raises(HTTPException) as exc:
        asyncio.run(auth_middleware.require_internal_auth(_legacy_request()))

    assert exc.value.status_code == 401


def test_legacy_tenant_key_is_admitted_when_the_insecure_switch_is_the_literal_one(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    settings = _settings(
        monkeypatch,
        ALLOW_LEGACY_TENANT_KEY="1",
        ALLOW_INSECURE_DEV_DEFAULTS="1",
        ISOLATED_E2E="1",
    )
    monkeypatch.setattr(auth_middleware, "get_settings", lambda: settings)

    context = asyncio.run(auth_middleware.require_internal_auth(_legacy_request()))

    assert context.org_id == "victim-org"
    assert context.service_name == "isolated-e2e"
