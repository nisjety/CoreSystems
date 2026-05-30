"""WebFetchTool — fetch a URL via Quarry-v2 with httpx fallback.

Primary path (production): calls Quarry `/v1/scrape`, which gives the
LLM JS-rendered HTML, SSRF/robots guards, TLS-fingerprint emulation,
charset detection, soft-404 filtering, and clean markdown extraction.

Fallback path (dev / no Quarry): plain httpx + selectolax. Same output
shape so callers see no behavioural change.

The output dict is stable across both paths:
    {url, final_url, title, content, content_type, status_code,
     fingerprint, language, source: "quarry" | "httpx"}
"""

from __future__ import annotations

import logging
from typing import Any

import httpx

try:  # selectolax is a pure-C fast HTML parser (lxml-equivalent perf, no lxml dep)
    from selectolax.parser import HTMLParser  # type: ignore
except ImportError:
    HTMLParser = None  # type: ignore[assignment]

from app.adapters.quarry_client import (
    QuarryError,
    QuarryRenderHints,
    QuarryUnavailableError,
    scrape as quarry_scrape,
)
from app.config import settings
from app.tools.base import ToolResult

logger = logging.getLogger(__name__)

# Generous default but bounded to avoid runaway memory on huge pages.
MAX_BODY_BYTES = 1_024 * 1024  # 1 MB
DEFAULT_MAX_CHARS = 8_000
DEFAULT_TIMEOUT_SECONDS = 15.0


class WebFetchTool:
    """Fetch a URL and return its content as text (HTML→text when needed)."""

    name = "web_fetch"
    description = (
        "Fetch the content of a URL and return it as text. Strips HTML "
        "boilerplate (nav, scripts, styles) so the result is focused on "
        "the main content. Use to read a specific page after `web_search` "
        "surfaces it."
    )
    input_schema: dict[str, Any] = {
        "type": "object",
        "properties": {
            "url": {
                "type": "string",
                "description": "URL to fetch (must start with http:// or https://).",
            },
            "max_chars": {
                "type": "integer",
                "description": "Maximum characters to return (default 8000, max 50000).",
                "default": DEFAULT_MAX_CHARS,
            },
            "wait_for_selector": {
                "type": "string",
                "description": (
                    "Optional CSS selector to wait for before snapshotting the "
                    "page. Only honoured when the page is JS-rendered via Quarry's "
                    "browser driver."
                ),
            },
        },
        "required": ["url"],
    }
    search_hint = "fetch url http web page content download read"
    should_defer = False  # Now has a real backend

    def is_read_only(self) -> bool:
        return True

    def is_concurrent_safe(self) -> bool:
        return True

    def is_destructive(self) -> bool:
        return False

    def prompt(self) -> str:
        return (
            "Fetch a web page and return its content. Returns a dict with "
            "`url`, `title`, `content`, and `content_type`. The content "
            "is plain text with HTML markup stripped."
        )

    def validate_input(self, input_data: dict[str, Any]) -> dict[str, Any]:
        url = input_data.get("url")
        if not url or not isinstance(url, str):
            raise ValueError("'url' is required and must be a non-empty string")
        if not url.startswith(("http://", "https://")):
            raise ValueError("'url' must start with http:// or https://")
        return input_data

    async def call(self, input_data: dict[str, Any]) -> ToolResult:
        url = input_data["url"]
        max_chars = int(input_data.get("max_chars", DEFAULT_MAX_CHARS))
        max_chars = max(500, min(50_000, max_chars))
        wait_for = input_data.get("wait_for_selector")

        if settings.quarry_edge_url:
            result = await _call_via_quarry(url, max_chars, wait_for)
            if result is not None:
                return result
            # Fall through to httpx on Quarry failure — production
            # operators should monitor logs for repeated fallback.

        return await _call_via_httpx(url, max_chars)


async def _call_via_quarry(
    url: str, max_chars: int, wait_for: str | None
) -> ToolResult | None:
    """Return a ToolResult, or None if Quarry failed and the caller
    should retry via the httpx fallback."""

    render = (
        QuarryRenderHints(wait_for_selector=wait_for, wait_for_timeout_ms=5_000)
        if wait_for
        else None
    )
    try:
        result = await quarry_scrape(
            url,
            edge_url=settings.quarry_edge_url,
            token=settings.quarry_edge_token,
            timeout_seconds=settings.quarry_timeout_seconds,
            render=render,
        )
    except QuarryUnavailableError:
        return None
    except QuarryError as exc:
        # Quarry returned a typed error — surface it directly to the
        # LLM so the model knows whether to retry, change strategy, or
        # give up. No fallback: Quarry already determined this URL is
        # unreachable / blocked, and httpx would hit the same wall.
        return ToolResult(
            output=(
                f"Fetched {url} via Quarry but got {exc.code} "
                f"(HTTP {exc.status_code}): {exc.message}"
            ),
            error=exc.code,
            metadata={"url": url, "status_code": exc.status_code, "source": "quarry"},
        )
    except httpx.RequestError as exc:
        # Transport failure to Quarry itself (DNS, refused connection).
        # Fall through to httpx fallback — Quarry might be down.
        logger.warning(
            "web_fetch_quarry_transport_failed",
            extra={"url": url, "error": str(exc)},
        )
        return None

    content = result.text or result.markdown
    if len(content) > max_chars:
        content = content[:max_chars]

    logger.info(
        "web_fetch_completed_via_quarry",
        extra={
            "url": url,
            "final_url": result.final_url,
            "status_code": result.status,
            "content_length": len(content),
        },
    )
    return ToolResult(
        output={
            "url": url,
            "final_url": result.final_url,
            "title": result.title or "",
            "content": content,
            "content_type": result.content_type or "text/plain",
            "status_code": result.status,
            "fingerprint": result.fingerprint,
            "language": result.language,
            "source": "quarry",
        },
        metadata={
            "url": url,
            "fetched_chars": len(content),
            "content_type": result.content_type,
            "source": "quarry",
        },
    )


async def _call_via_httpx(url: str, max_chars: int) -> ToolResult:
    headers = {
        "User-Agent": (
            "agent-core/1.0 (+web_fetch tool; LLM-assisted browsing)"
        ),
        "Accept": "text/html,application/xhtml+xml,text/plain;q=0.9,*/*;q=0.5",
    }

    try:
        async with httpx.AsyncClient(
            timeout=DEFAULT_TIMEOUT_SECONDS,
            follow_redirects=True,
            limits=httpx.Limits(max_keepalive_connections=4),
        ) as client:
            response = await client.get(url, headers=headers)
    except httpx.RequestError as exc:
        logger.warning(
            "web_fetch_request_failed",
            extra={"url": url, "error": str(exc)},
        )
        return ToolResult(
            output=f"Failed to fetch {url}: {exc}",
            error=f"request_failed: {exc}",
            metadata={"url": url, "source": "httpx"},
        )

    if response.status_code >= 400:
        return ToolResult(
            output=(
                f"Fetched {url} but got HTTP {response.status_code}. "
                "The page may not exist or may be blocked."
            ),
            error=f"http_{response.status_code}",
            metadata={
                "url": url,
                "status_code": response.status_code,
                "source": "httpx",
            },
        )

    body_bytes = response.content[:MAX_BODY_BYTES]
    content_type = response.headers.get("content-type", "").lower()

    # Decode using the response's declared encoding when available;
    # fall back to utf-8 with replacement.
    try:
        body_text = body_bytes.decode(
            response.encoding or "utf-8", errors="replace"
        )
    except (UnicodeDecodeError, LookupError):
        body_text = body_bytes.decode("utf-8", errors="replace")

    title = ""
    content = body_text

    if "text/html" in content_type or "application/xhtml" in content_type:
        content, title = _extract_html_text(body_text)

    if len(content) > max_chars:
        content = content[:max_chars]

    logger.info(
        "web_fetch_completed",
        extra={
            "url": url,
            "status_code": response.status_code,
            "content_length": len(content),
        },
    )
    return ToolResult(
        output={
            "url": url,
            "title": title,
            "content": content,
            "content_type": content_type or "text/plain",
            "status_code": response.status_code,
            "source": "httpx",
        },
        metadata={
            "url": url,
            "fetched_chars": len(content),
            "content_type": content_type,
            "source": "httpx",
        },
    )


def _extract_html_text(html: str) -> tuple[str, str]:
    """Pull the visible text and <title> out of HTML.

    Uses selectolax when available (fast C parser); falls back to a tiny
    hand-rolled stripper. The output is whitespace-normalized; callers
    can trim/slice as needed.
    """
    if HTMLParser is None:
        return _strip_html_naive(html), ""

    parser = HTMLParser(html)
    # Title.
    title_node = parser.css_first("title")
    title = title_node.text(strip=True) if title_node else ""

    # Strip script/style/nav/footer/header before extracting text.
    for selector in ("script", "style", "noscript", "nav", "footer", "header", "aside"):
        for node in parser.css(selector):
            node.decompose()

    # Walk the body and join meaningful text.
    body_node = parser.css_first("body") or parser.root
    text = body_node.text(separator=" ", strip=True) if body_node else ""

    # Normalize whitespace.
    text = " ".join(text.split())
    return text, title


def _strip_html_naive(html: str) -> str:
    """Minimal HTML→text fallback when selectolax isn't installed."""
    import re

    # Drop script + style blocks first.
    html = re.sub(r"<script[^>]*>.*?</script>", " ", html, flags=re.DOTALL | re.IGNORECASE)
    html = re.sub(r"<style[^>]*>.*?</style>", " ", html, flags=re.DOTALL | re.IGNORECASE)
    # Strip every remaining tag.
    text = re.sub(r"<[^>]+>", " ", html)
    # Collapse whitespace.
    return " ".join(text.split())
