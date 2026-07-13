"""Fail-closed network policy for customer-controlled connector targets."""

import asyncio
import ipaddress
import socket
from urllib.parse import urlsplit, urlunsplit


class UnsafeOutboundTarget(ValueError):
    """The requested outbound target could reach a non-public network."""


def _require_public_address(value: str) -> None:
    try:
        address = ipaddress.ip_address(value)
    except ValueError as exc:
        raise UnsafeOutboundTarget("connector target resolved to an invalid address") from exc
    if not address.is_global:
        raise UnsafeOutboundTarget("connector target must resolve only to public addresses")


async def validate_public_http_url(raw_url: str) -> str:
    """Validate scheme, authority, credentials, and every current DNS answer."""
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

    try:
        literal = ipaddress.ip_address(hostname)
    except ValueError:
        try:
            answers = await asyncio.to_thread(
                socket.getaddrinfo,
                hostname,
                parsed.port or (443 if parsed.scheme == "https" else 80),
                type=socket.SOCK_STREAM,
            )
        except (OSError, UnicodeError, ValueError) as exc:
            raise UnsafeOutboundTarget("connector target DNS resolution failed") from exc
        addresses = {str(answer[4][0]) for answer in answers if answer[4]}
        if not addresses:
            raise UnsafeOutboundTarget("connector target DNS resolution returned no addresses")
        for address in addresses:
            _require_public_address(address)
    else:
        _require_public_address(str(literal))

    normalized_netloc = hostname
    if ":" in hostname:
        normalized_netloc = f"[{hostname}]"
    if parsed.port is not None:
        normalized_netloc = f"{normalized_netloc}:{parsed.port}"
    return urlunsplit((parsed.scheme, normalized_netloc, parsed.path or "/", parsed.query, ""))
