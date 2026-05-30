"""WebSearchTool — real Brave Search backend.

U2-16 (velion ui-ux-velion-gap.md §10): replaces the previous stub that
returned `[web_search stub] No search backend configured`. Now hits the
Brave Search API directly using `BRAVE_API_KEY` from the environment.

Future architecture (documented in §11.4): route through Quarry-v2 so
the search call benefits from anti-bot, rate-limiting, robots.txt
compliance, and security scanning. The tool input/output schema stays
identical when we swap — only the implementation of `_perform_search`
changes.
"""

from __future__ import annotations

import logging
import os
from typing import Any

import httpx

from app.tools.base import ToolResult

logger = logging.getLogger(__name__)

BRAVE_SEARCH_ENDPOINT = "https://api.search.brave.com/res/v1/web/search"
DEFAULT_TIMEOUT_SECONDS = 10.0


class WebSearchTool:
    """Search the web for information using the Brave Search API."""

    name = "web_search"
    description = (
        "Search the web for information on a topic. Returns a list of "
        "result items with title, URL, and a short snippet. Use this "
        "when the user asks about current events or anything requiring "
        "information beyond your training data."
    )
    input_schema: dict[str, Any] = {
        "type": "object",
        "properties": {
            "query": {
                "type": "string",
                "description": "Search query.",
            },
            "max_results": {
                "type": "integer",
                "description": "Maximum number of results to return (1-20).",
                "default": 5,
            },
            "country": {
                "type": "string",
                "description": "ISO-3166-1 alpha-2 country code (e.g. 'no', 'us').",
            },
            "freshness": {
                "type": "string",
                "description": "Time-window filter: 'pd' (day), 'pw' (week), 'pm' (month), 'py' (year).",
            },
        },
        "required": ["query"],
    }
    search_hint = "web search internet find information brave google"
    should_defer = False  # Now has a real backend — load eagerly

    def is_read_only(self) -> bool:
        return True

    def is_concurrent_safe(self) -> bool:
        return True

    def is_destructive(self) -> bool:
        return False

    def prompt(self) -> str:
        return (
            "Search the web for current information. Returns a list of "
            "search results — each with `title`, `url`, and `snippet`. "
            "Cite specific URLs in your answer when relevant."
        )

    def validate_input(self, input_data: dict[str, Any]) -> dict[str, Any]:
        query = input_data.get("query")
        if not query or not isinstance(query, str) or not query.strip():
            raise ValueError("'query' is required and must be a non-empty string")
        return input_data

    async def call(self, input_data: dict[str, Any]) -> ToolResult:
        query = input_data["query"].strip()
        max_results = int(input_data.get("max_results", 5))
        max_results = max(1, min(20, max_results))
        country = input_data.get("country")
        freshness = input_data.get("freshness")

        api_key = os.environ.get("BRAVE_API_KEY", "").strip()
        if not api_key:
            logger.warning(
                "web_search_not_configured",
                extra={"reason": "BRAVE_API_KEY env var not set"},
            )
            return ToolResult(
                output=(
                    "Web search is currently unavailable: no Brave API key "
                    "configured. Tell the user the feature is offline."
                ),
                error="search_unavailable",
                metadata={"reason": "no_brave_api_key"},
            )

        params: dict[str, Any] = {
            "q": query,
            "count": max_results,
            # Brave throws 422 on extra_snippets when not paid plan; skip.
        }
        if country:
            params["country"] = country
        if freshness:
            params["freshness"] = freshness

        headers = {
            "Accept": "application/json",
            "X-Subscription-Token": api_key,
            "User-Agent": "agent-core/1.0 (+brave-search)",
        }

        try:
            async with httpx.AsyncClient(timeout=DEFAULT_TIMEOUT_SECONDS) as client:
                response = await client.get(
                    BRAVE_SEARCH_ENDPOINT,
                    params=params,
                    headers=headers,
                )
        except httpx.RequestError as exc:
            logger.warning("web_search_request_failed", extra={"error": str(exc)})
            return ToolResult(
                output="Web search request failed; try again later.",
                error=f"request_failed: {exc}",
                metadata={"query": query},
            )

        if response.status_code != 200:
            body = response.text[:200]
            logger.warning(
                "web_search_http_error",
                extra={"status": response.status_code, "body": body},
            )
            return ToolResult(
                output=f"Search backend returned HTTP {response.status_code}.",
                error=f"http_{response.status_code}",
                metadata={"query": query},
            )

        payload = response.json()
        web = payload.get("web") or {}
        raw_results = web.get("results") or []

        results: list[dict[str, str]] = []
        for item in raw_results[:max_results]:
            results.append(
                {
                    "title": str(item.get("title", "")),
                    "url": str(item.get("url", "")),
                    "snippet": str(
                        item.get("description")
                        or item.get("snippet")
                        or ""
                    ),
                }
            )

        logger.info(
            "web_search_completed",
            extra={"query": query, "result_count": len(results)},
        )
        return ToolResult(
            output={
                "query": query,
                "results": results,
                "source": "brave",
            },
            metadata={
                "query": query,
                "result_count": len(results),
                "backend": "brave",
            },
        )
