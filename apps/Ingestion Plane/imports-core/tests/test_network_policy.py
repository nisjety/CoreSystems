"""Outbound connector targets must not reach private infrastructure."""

import asyncio
import socket
from dataclasses import dataclass, field

import pytest

from app.network_policy import (
    PinnedAsyncNetworkBackend,
    PublicHttpTarget,
    UnsafeOutboundTarget,
    create_pinned_urllib_opener,
    resolve_public_http_target,
    validate_public_http_url,
)


@dataclass
class _RecordingNetworkBackend:
    calls: list[tuple[str, int]] = field(default_factory=list)

    async def connect_tcp(self, host: str, port: int, **_kwargs: object) -> object:
        self.calls.append((host, port))
        return object()

    async def connect_unix_socket(self, *_args: object, **_kwargs: object) -> object:
        raise AssertionError("a public connector must not use a Unix socket")

    async def sleep(self, _seconds: float) -> None:
        return None


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


def test_resolved_target_pins_all_vetted_public_addresses(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(
        socket,
        "getaddrinfo",
        lambda *_args, **_kwargs: [
            (socket.AF_INET, socket.SOCK_STREAM, 6, "", ("93.184.216.34", 443)),
            (socket.AF_INET6, socket.SOCK_STREAM, 6, "", ("2606:2800:220:1:248:1893:25c8:1946", 443)),
        ],
    )

    target = asyncio.run(resolve_public_http_target("https://customer.example/data"))

    assert target.url == "https://customer.example/data"
    assert target.hostname == "customer.example"
    assert target.port == 443
    assert target.addresses == ("93.184.216.34", "2606:2800:220:1:248:1893:25c8:1946")


def test_pinned_backend_never_re_resolves_or_connects_to_an_unvetted_host(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(
        socket,
        "getaddrinfo",
        lambda *_args, **_kwargs: [
            (socket.AF_INET, socket.SOCK_STREAM, 6, "", ("93.184.216.34", 443))
        ],
    )
    target = asyncio.run(resolve_public_http_target("https://customer.example/data"))
    delegate = _RecordingNetworkBackend()
    backend = PinnedAsyncNetworkBackend(target, delegate=delegate)

    asyncio.run(backend.connect_tcp("customer.example", 443))

    assert delegate.calls == [("93.184.216.34", 443)]
    with pytest.raises(UnsafeOutboundTarget):
        asyncio.run(backend.connect_tcp("rebound.example", 443))
    assert delegate.calls == [("93.184.216.34", 443)]


def test_pinned_backend_rejects_a_private_address_even_if_constructed_directly() -> None:
    with pytest.raises(UnsafeOutboundTarget):
        PinnedAsyncNetworkBackend(
            PublicHttpTarget(
                url="https://customer.example/data",
                hostname="customer.example",
                port=443,
                addresses=("10.0.0.4",),
            )
        )


def test_pinned_urllib_opener_rejects_a_private_address_even_if_constructed_directly() -> None:
    with pytest.raises(UnsafeOutboundTarget):
        create_pinned_urllib_opener(
            PublicHttpTarget(
                url="https://customer.example/data",
                hostname="customer.example",
                port=443,
                addresses=("10.0.0.4",),
            )
        )
