"""Outbound connector targets must not reach private infrastructure."""

import asyncio
import socket

import pytest

from app.network_policy import UnsafeOutboundTarget, validate_public_http_url


@pytest.mark.parametrize(
    "url",
    [
        "http://127.0.0.1/admin",
        "http://[::1]/admin",
        "http://169.254.169.254/latest/meta-data",
        "file:///etc/passwd",
        "https://user:secret@example.com/data",
        "https://localhost/data",
    ],
)
def test_rejects_unsafe_connector_targets(url: str) -> None:
    with pytest.raises(UnsafeOutboundTarget):
        asyncio.run(validate_public_http_url(url))


def test_rejects_hostname_resolving_to_private_address(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(
        socket,
        "getaddrinfo",
        lambda *_args, **_kwargs: [(socket.AF_INET, socket.SOCK_STREAM, 6, "", ("10.0.0.4", 443))],
    )

    with pytest.raises(UnsafeOutboundTarget):
        asyncio.run(validate_public_http_url("https://customer.example/data"))


def test_accepts_hostname_only_when_all_addresses_are_public(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(
        socket,
        "getaddrinfo",
        lambda *_args, **_kwargs: [
            (socket.AF_INET, socket.SOCK_STREAM, 6, "", ("93.184.216.34", 443))
        ],
    )

    normalized = asyncio.run(validate_public_http_url("https://customer.example/data"))

    assert normalized == "https://customer.example/data"
