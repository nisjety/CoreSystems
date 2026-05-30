"""Quarry-v2 edge client.

Wraps the `/v1/scrape` endpoint so agent-core tools (`web_fetch`,
`extract_structured`, future page-action workflows) share one auth +
error-handling path. Returns the parsed `NormalizedOutput` envelope as
a plain dict so callers don't import Quarry's Rust types.

Why not just use `httpx` inline:
- Centralises the bearer-token plumbing.
- Centralises the dev/prod fallback (no `quarry_edge_url` → return
  None so the caller can degrade to a stub).
- Adds Quarry-aware error mapping (timeout / SSRF block / soft-404 /
  HTTP 4xx-5xx) so tool-call results expose useful errors to the LLM
  instead of a generic 500.

Forward compatibility:
- The `prefer_http3` / `render` knobs map straight onto Quarry's
  `ScrapeRequest`, so a tool that wants SPA-aware waits or QUIC just
  passes them through.
"""

from __future__ import annotations

import logging
from dataclasses import dataclass
from typing import Any

import httpx

logger = logging.getLogger(__name__)


@dataclass(frozen=True)
class QuarryRenderHints:
    """Browser-driver render hints. Ignored by static fetches."""

    wait_for_selector: str | None = None
    wait_for_timeout_ms: int | None = None


@dataclass(frozen=True)
class QuarryScrapeResult:
    """Normalised projection of Quarry's `/v1/scrape` envelope.

    Only the fields the Model Plane currently uses are surfaced. The
    full Quarry envelope is also kept under `raw` for tools that need
    it (e.g. branding pickup, JSON-LD).
    """

    url: str
    final_url: str
    status: int
    content_type: str | None
    title: str | None
    markdown: str
    text: str
    fingerprint: str
    language: str | None
    raw: dict[str, Any]


class QuarryUnavailableError(RuntimeError):
    """Quarry edge is not wired (no `quarry_edge_url` in settings)."""


class QuarryError(RuntimeError):
    """Quarry returned a typed error (HTTP 4xx / 5xx)."""

    def __init__(self, code: str, message: str, status_code: int) -> None:
        super().__init__(f"{code}: {message}")
        self.code = code
        self.message = message
        self.status_code = status_code


async def scrape(
    url: str,
    *,
    edge_url: str,
    token: str,
    timeout_seconds: float = 30.0,
    render: QuarryRenderHints | None = None,
    prefer_http3: bool = False,
    org_id: str | None = None,
) -> QuarryScrapeResult:
    """Call Quarry `/v1/scrape` and return a normalised result.

    Raises:
        QuarryUnavailableError: when ``edge_url`` is empty.
        QuarryError: on HTTP 4xx / 5xx from Quarry.
        httpx.RequestError: on transport-level failure (timeout, DNS).
    """

    if not edge_url:
        raise QuarryUnavailableError("quarry_edge_url is not configured")

    payload: dict[str, Any] = {"url": url}
    if render is not None and render.wait_for_selector:
        payload["render"] = {
            "waitForSelector": render.wait_for_selector,
            "waitForTimeoutMs": render.wait_for_timeout_ms,
        }
    if prefer_http3:
        payload["prefer_http3"] = True

    headers: dict[str, str] = {
        "Content-Type": "application/json",
        "Accept": "application/json",
    }
    if token:
        headers["Authorization"] = f"Bearer {token}"
    # The edge derives org from the JWT; this header is informational
    # for cross-plane tracing only.
    if org_id:
        headers["X-Quarry-Org"] = org_id

    endpoint = edge_url.rstrip("/") + "/v1/scrape"
    async with httpx.AsyncClient(timeout=timeout_seconds) as client:
        response = await client.post(endpoint, json=payload, headers=headers)

    if response.status_code >= 400:
        # Quarry's error envelope is `{ "ok": false, "error": { "code", "message" } }`.
        try:
            body = response.json()
            err = body.get("error") or {}
            code = err.get("code") or f"HTTP_{response.status_code}"
            message = err.get("message") or response.text[:200]
        except ValueError:
            code = f"HTTP_{response.status_code}"
            message = response.text[:200]
        raise QuarryError(code, message, response.status_code)

    body = response.json()
    data = body.get("data") or {}
    return _project(url, data)


def _project(requested_url: str, data: dict[str, Any]) -> QuarryScrapeResult:
    formats = data.get("formats") or {}
    metadata = data.get("metadata") or {}
    url_triple = data.get("url") or {}

    markdown = formats.get("markdown") or ""
    # Quarry also ships a `text` projection; fall back to markdown.
    text = formats.get("text") or markdown

    return QuarryScrapeResult(
        url=requested_url,
        final_url=url_triple.get("final") or requested_url,
        status=int(data.get("status") or 0),
        content_type=data.get("content_type"),
        title=metadata.get("title"),
        markdown=markdown,
        text=text,
        fingerprint=data.get("fingerprint") or "",
        language=metadata.get("lang"),
        raw=data,
    )
