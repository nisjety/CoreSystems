"""Fail-closed network policy for customer-controlled connector targets."""

import asyncio
import http.client
import ipaddress
import socket
from dataclasses import dataclass
from http.cookiejar import CookieJar
from typing import Any
from urllib.parse import urlsplit, urlunsplit
from urllib.request import (
    HTTPCookieProcessor,
    HTTPHandler,
    HTTPSHandler,
    ProxyHandler,
    build_opener,
)

import httpcore
import httpx
from httpx._transports.default import AsyncResponseStream, map_httpcore_exceptions


@dataclass(frozen=True)
class PublicHttpTarget:
    """A public URL whose connection addresses were checked immediately before use."""

    url: str
    hostname: str
    port: int
    addresses: tuple[str, ...]


class UnsafeOutboundTarget(ValueError):
    """The requested outbound target could reach a non-public network."""


def _require_public_address(value: str) -> None:
    try:
        address = ipaddress.ip_address(value)
    except ValueError as exc:
        raise UnsafeOutboundTarget("connector target resolved to an invalid address") from exc
    if not address.is_global:
        raise UnsafeOutboundTarget("connector target must resolve only to public addresses")


async def resolve_public_http_target(raw_url: str) -> PublicHttpTarget:
    """Resolve a connector URL once and retain only public connection addresses."""
    if not isinstance(raw_url, str) or not raw_url.strip():
        raise UnsafeOutboundTarget("connector URL is required")

    parsed = urlsplit(raw_url.strip())
    if parsed.scheme not in {"http", "https"}:
        raise UnsafeOutboundTarget("connector URL must use http or https")
    if not parsed.hostname or parsed.username is not None or parsed.password is not None:
        raise UnsafeOutboundTarget("connector URL must have a host and no embedded credentials")

    hostname = parsed.hostname.rstrip(".").lower()
    if hostname == "localhost" or hostname.endswith(".localhost"):
        raise UnsafeOutboundTarget("connector target must be public")

    default_port = 443 if parsed.scheme == "https" else 80
    port = parsed.port or default_port
    try:
        literal = ipaddress.ip_address(hostname)
    except ValueError:
        try:
            answers = await asyncio.to_thread(
                socket.getaddrinfo,
                hostname,
                port,
                type=socket.SOCK_STREAM,
            )
        except (OSError, UnicodeError, ValueError) as exc:
            raise UnsafeOutboundTarget("connector target DNS resolution failed") from exc
        addresses = tuple(dict.fromkeys(str(answer[4][0]) for answer in answers if answer[4]))
        if not addresses:
            raise UnsafeOutboundTarget("connector target DNS resolution returned no addresses")
        for address in addresses:
            _require_public_address(address)
    else:
        _require_public_address(str(literal))
        addresses = (str(literal),)

    normalized_netloc = hostname
    if ":" in hostname:
        normalized_netloc = f"[{hostname}]"
    if parsed.port is not None:
        normalized_netloc = f"{normalized_netloc}:{parsed.port}"
    return PublicHttpTarget(
        url=urlunsplit((parsed.scheme, normalized_netloc, parsed.path or "/", parsed.query, "")),
        hostname=hostname,
        port=port,
        addresses=addresses,
    )


async def validate_public_http_url(raw_url: str) -> str:
    """Compatibility helper for callers that only need the normalized URL."""
    return (await resolve_public_http_target(raw_url)).url


def _normalized_hostname(value: str) -> str:
    return value.rstrip(".").lower()


class PinnedAsyncNetworkBackend:
    """Dial only addresses accepted by one immediate target preflight."""

    def __init__(self, target: PublicHttpTarget, *, delegate: Any | None = None) -> None:
        from httpcore._backends.anyio import AnyIOBackend

        _validate_pinned_target(target)
        self._target = target
        self._delegate = delegate if delegate is not None else AnyIOBackend()

    async def connect_tcp(
        self,
        host: str,
        port: int,
        timeout: float | None = None,
        local_address: str | None = None,
        socket_options: Any = None,
    ) -> Any:
        if _normalized_hostname(host) != self._target.hostname or port != self._target.port:
            raise UnsafeOutboundTarget("connector attempted an unpreflighted outbound connection")

        last_error: Exception | None = None
        for address in self._target.addresses:
            try:
                return await self._delegate.connect_tcp(
                    address,
                    port,
                    timeout=timeout,
                    local_address=local_address,
                    socket_options=socket_options,
                )
            except (httpcore.ConnectError, httpcore.ConnectTimeout) as exc:
                last_error = exc
        if last_error is not None:
            raise last_error
        raise UnsafeOutboundTarget("connector target has no vetted connection addresses")

    async def connect_unix_socket(self, *_args: Any, **_kwargs: Any) -> Any:
        raise UnsafeOutboundTarget("connector must not use a Unix socket")

    async def sleep(self, seconds: float) -> None:
        await self._delegate.sleep(seconds)


class PinnedAsyncHTTPTransport(httpx.AsyncBaseTransport):
    """HTTPX direct transport that retains the URL host for TLS and Host headers."""

    def __init__(self, target: PublicHttpTarget) -> None:
        self._pool = httpcore.AsyncConnectionPool(
            network_backend=PinnedAsyncNetworkBackend(target),
            retries=0,
        )

    async def handle_async_request(self, request: httpx.Request) -> httpx.Response:
        assert isinstance(request.stream, httpx.AsyncByteStream)
        core_request = httpcore.Request(
            method=request.method,
            url=httpcore.URL(
                scheme=request.url.raw_scheme,
                host=request.url.raw_host,
                port=request.url.port,
                target=request.url.raw_path,
            ),
            headers=request.headers.raw,
            content=request.stream,
            extensions=request.extensions,
        )
        with map_httpcore_exceptions():
            response = await self._pool.handle_async_request(core_request)
        assert hasattr(response.stream, "__aiter__")
        return httpx.Response(
            status_code=response.status,
            headers=response.headers,
            stream=AsyncResponseStream(response.stream),
            extensions=response.extensions,
        )

    async def aclose(self) -> None:
        await self._pool.aclose()


def create_pinned_async_http_client(
    target: PublicHttpTarget, *, timeout: float
) -> httpx.AsyncClient:
    """Create a proxy-free direct client that cannot resolve the host again."""
    return httpx.AsyncClient(
        timeout=timeout,
        follow_redirects=False,
        trust_env=False,
        transport=PinnedAsyncHTTPTransport(target),
    )


class _PinnedHTTPConnection(http.client.HTTPConnection):
    def __init__(self, *args: Any, target: PublicHttpTarget, **kwargs: Any) -> None:
        self._target = target
        super().__init__(*args, **kwargs)

    def connect(self) -> None:
        _require_pinned_origin(self.host, self.port, self._target)
        self.sock = _connect_to_vetted_address(self._target, self.timeout, self.source_address)


class _PinnedHTTPSConnection(http.client.HTTPSConnection):
    def __init__(self, *args: Any, target: PublicHttpTarget, **kwargs: Any) -> None:
        self._target = target
        super().__init__(*args, **kwargs)

    def connect(self) -> None:
        _require_pinned_origin(self.host, self.port, self._target)
        self.sock = _connect_to_vetted_address(self._target, self.timeout, self.source_address)
        if self._tunnel_host:
            self._tunnel()
        self.sock = self._context.wrap_socket(self.sock, server_hostname=self.host)


def _require_pinned_origin(host: str, port: int, target: PublicHttpTarget) -> None:
    if _normalized_hostname(host) != target.hostname or port != target.port:
        raise UnsafeOutboundTarget("connector attempted an unpreflighted outbound connection")


def _validate_pinned_target(target: PublicHttpTarget) -> None:
    if not target.addresses:
        raise UnsafeOutboundTarget("connector target has no vetted connection addresses")
    if target.port < 1 or target.port > 65535:
        raise UnsafeOutboundTarget("connector target has an invalid port")
    for address in target.addresses:
        _require_public_address(address)


def _connect_to_vetted_address(
    target: PublicHttpTarget, timeout: float | object, source_address: Any
) -> socket.socket:
    last_error: OSError | None = None
    for address in target.addresses:
        try:
            return socket.create_connection((address, target.port), timeout, source_address)
        except OSError as exc:
            last_error = exc
    if last_error is not None:
        raise last_error
    raise UnsafeOutboundTarget("connector target has no vetted connection addresses")


def create_pinned_urllib_opener(target: PublicHttpTarget):
    """Create an Odoo-compatible, proxy-free opener pinned to the target addresses."""
    _validate_pinned_target(target)

    class PinnedHTTPHandler(HTTPHandler):
        def http_open(self, request: Any) -> Any:
            return self.do_open(
                lambda host, **kwargs: _PinnedHTTPConnection(host, target=target, **kwargs),
                request,
            )

    class PinnedHTTPSHandler(HTTPSHandler):
        def https_open(self, request: Any) -> Any:
            return self.do_open(
                lambda host, **kwargs: _PinnedHTTPSConnection(host, target=target, **kwargs),
                request,
            )

    return build_opener(
        ProxyHandler({}),
        HTTPCookieProcessor(CookieJar()),
        PinnedHTTPHandler(),
        PinnedHTTPSHandler(),
    )
