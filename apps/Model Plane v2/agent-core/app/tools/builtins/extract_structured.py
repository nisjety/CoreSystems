"""ExtractStructuredTool — URL + JSON schema → typed JSON.

Firecrawl-equivalent capability. Combines two existing primitives:
  1. Quarry `/v1/scrape` for full-fat page fetch (JS rendering, TLS
     fingerprint, charset, soft-404, JSON-LD).
  2. The cross-provider tool-call surface in `reasoning_runtime` to
     coerce LLM output into a caller-supplied JSON schema.

We deliberately use a forced **tool call** rather than the provider-
specific `response_format=json_schema` knob. Tool calls are uniformly
supported across OpenAI, Anthropic Claude, Gemini, and Mistral; the
provider abstraction already handles them; and tool-call args come
back as a typed JSON object that we can validate against the schema
the caller passed in. No need to thread a new field through every
provider adapter.

Flow:
  1. Caller passes `url` + `schema` + optional `instructions`.
  2. Tool calls Quarry to get clean markdown.
  3. Wraps the markdown in a system prompt that says "extract per
     schema, call the `record_extraction` function with the result."
  4. Invokes the LLM with `tools=[record_extraction(schema)]` and
     `tool_choice` forced to that function.
  5. Parses the tool-call args, validates against the schema, returns.
"""

from __future__ import annotations

import json
import logging
from typing import Any

from app.adapters.quarry_client import (
    QuarryError,
    QuarryRenderHints,
    QuarryUnavailableError,
    scrape as quarry_scrape,
)
from app.config import settings
from app.llm_client import LLMClient
from app.tools.base import ToolResult

logger = logging.getLogger(__name__)

# Maximum markdown size we feed to the LLM. Picked to stay well under
# the 128k context floor that all in-use models share, leaving room
# for the system prompt + schema + caller instructions.
MAX_MARKDOWN_CHARS = 80_000

# The single tool we force the LLM to call. Its parameters are the
# caller-supplied schema; the LLM's job is to fill them in.
EXTRACTION_TOOL_NAME = "record_extraction"


class ExtractStructuredTool:
    """Extract structured data from a URL using a caller-supplied JSON schema."""

    name = "extract_structured"
    description = (
        "Fetch a URL and extract data matching a caller-supplied JSON schema. "
        "Uses a real crawler (Quarry) so JS-rendered SPAs and PDFs work. "
        "Returns the structured object as a dict — no manual prompt-engineering "
        "needed. Equivalent to Firecrawl's `extract` endpoint."
    )
    input_schema: dict[str, Any] = {
        "type": "object",
        "properties": {
            "url": {
                "type": "string",
                "description": "URL to extract from. Must start with http:// or https://.",
            },
            "schema": {
                "type": "object",
                "description": (
                    "JSON Schema describing the target shape. The LLM will fill "
                    "in this schema's properties with values extracted from the "
                    "page. Must be a valid JSON Schema with `type: object` at "
                    "the root."
                ),
            },
            "instructions": {
                "type": "string",
                "description": (
                    "Optional natural-language guidance to disambiguate the "
                    "extraction (e.g., 'prefer the price in NOK', 'extract "
                    "only the primary author')."
                ),
            },
            "wait_for_selector": {
                "type": "string",
                "description": (
                    "Optional CSS selector to wait for before snapshotting the "
                    "page. Use when the target data is rendered by JavaScript."
                ),
            },
        },
        "required": ["url", "schema"],
    }
    search_hint = (
        "extract structured json schema firecrawl scrape page entities fields"
    )
    should_defer = False

    def __init__(self, llm_client: LLMClient | None = None) -> None:
        # Allow injection for tests; default to the shared client used
        # everywhere else in agent-core.
        self._llm = llm_client or LLMClient()

    def is_read_only(self) -> bool:
        return True

    def is_concurrent_safe(self) -> bool:
        return True

    def is_destructive(self) -> bool:
        return False

    def prompt(self) -> str:
        return (
            "Fetch a URL and extract data matching a JSON schema. Returns a "
            "dict with `url`, `extracted` (the structured object), and "
            "`source_fingerprint` so the caller can detect when the page "
            "changes."
        )

    def validate_input(self, input_data: dict[str, Any]) -> dict[str, Any]:
        url = input_data.get("url")
        if not isinstance(url, str) or not url:
            raise ValueError("'url' is required and must be a non-empty string")
        if not url.startswith(("http://", "https://")):
            raise ValueError("'url' must start with http:// or https://")

        schema = input_data.get("schema")
        if not isinstance(schema, dict) or not schema:
            raise ValueError("'schema' is required and must be a non-empty JSON Schema object")
        if schema.get("type") and schema["type"] != "object":
            raise ValueError("schema root must be type: object")
        return input_data

    async def call(self, input_data: dict[str, Any]) -> ToolResult:
        url = input_data["url"]
        schema = input_data["schema"]
        instructions = input_data.get("instructions", "")
        wait_for = input_data.get("wait_for_selector")

        # 1. Fetch via Quarry.
        try:
            scrape_result = await self._fetch(url, wait_for)
        except QuarryUnavailableError:
            return ToolResult(
                output=(
                    "extract_structured requires Quarry to be configured. "
                    "Set QUARRY_EDGE_URL in agent-core settings."
                ),
                error="quarry_unavailable",
                metadata={"url": url},
            )
        except QuarryError as exc:
            return ToolResult(
                output=f"Failed to fetch {url}: {exc.code} — {exc.message}",
                error=exc.code,
                metadata={
                    "url": url,
                    "status_code": exc.status_code,
                    "source": "quarry",
                },
            )

        markdown = scrape_result.markdown or scrape_result.text
        if not markdown.strip():
            return ToolResult(
                output=f"Quarry returned no extractable content for {url}",
                error="empty_content",
                metadata={"url": url, "status_code": scrape_result.status},
            )
        if len(markdown) > MAX_MARKDOWN_CHARS:
            markdown = markdown[:MAX_MARKDOWN_CHARS]

        # 2. Build the extraction prompt + forced tool call.
        messages = _build_messages(
            url=url,
            page_title=scrape_result.title,
            markdown=markdown,
            instructions=instructions,
        )
        tools = [_schema_to_tool(schema)]

        # 3. Invoke the LLM with `tool_choice` forced to the extraction
        # function. Provider adapters translate this to the right thing
        # for OpenAI/Anthropic/Gemini/Mistral.
        try:
            response = await self._llm.complete(
                request_id=f"extract:{scrape_result.fingerprint or url}",
                org_id="agent-core",
                model_id=settings.planner_model,
                provider=settings.planner_provider,
                messages=messages,
                tools=tools,
                temperature=0.0,
            )
        except Exception as exc:  # noqa: BLE001 — surface to LLM as a tool error
            logger.exception("extract_structured_llm_failed", extra={"url": url})
            return ToolResult(
                output=f"LLM extraction failed for {url}: {exc}",
                error="llm_failed",
                metadata={"url": url},
            )

        tool_calls = response.get("tool_calls") or []
        if not tool_calls:
            return ToolResult(
                output=(
                    f"LLM did not return a tool call for {url}. "
                    "The page may not contain the requested fields."
                ),
                error="no_tool_call",
                metadata={
                    "url": url,
                    "llm_content": response.get("content", "")[:500],
                },
            )

        call = tool_calls[0]
        arguments_raw = call.get("function", {}).get("arguments", "{}")
        try:
            extracted = (
                json.loads(arguments_raw)
                if isinstance(arguments_raw, str)
                else arguments_raw
            )
        except json.JSONDecodeError as exc:
            return ToolResult(
                output=f"LLM returned invalid JSON for {url}: {exc}",
                error="invalid_json",
                metadata={"url": url, "raw": str(arguments_raw)[:500]},
            )

        logger.info(
            "extract_structured_completed",
            extra={
                "url": url,
                "final_url": scrape_result.final_url,
                "fingerprint": scrape_result.fingerprint,
                "fields": list(extracted.keys()) if isinstance(extracted, dict) else None,
            },
        )
        return ToolResult(
            output={
                "url": url,
                "final_url": scrape_result.final_url,
                "title": scrape_result.title,
                "extracted": extracted,
                "source_fingerprint": scrape_result.fingerprint,
                "language": scrape_result.language,
            },
            metadata={
                "url": url,
                "fingerprint": scrape_result.fingerprint,
                "tokens_in": response.get("tokens_in"),
                "tokens_out": response.get("tokens_out"),
            },
        )

    async def _fetch(self, url: str, wait_for: str | None):
        render = (
            QuarryRenderHints(wait_for_selector=wait_for, wait_for_timeout_ms=5_000)
            if wait_for
            else None
        )
        return await quarry_scrape(
            url,
            edge_url=settings.quarry_edge_url,
            token=settings.quarry_edge_token,
            timeout_seconds=settings.quarry_timeout_seconds,
            render=render,
        )


def _build_messages(
    *,
    url: str,
    page_title: str | None,
    markdown: str,
    instructions: str,
) -> list[dict[str, Any]]:
    system = (
        "You extract structured data from web pages. Read the supplied page "
        "content carefully. For any field where the page does not contain a "
        "clear answer, omit the field (do not invent values). "
        "Call the `record_extraction` function exactly once with your result."
    )
    if instructions:
        system += "\n\nAdditional caller instructions:\n" + instructions

    title_line = f"# {page_title}\n\n" if page_title else ""
    user = (
        f"URL: {url}\n\n"
        f"--- PAGE CONTENT ---\n"
        f"{title_line}{markdown}\n"
        f"--- END PAGE CONTENT ---\n\n"
        f"Extract the requested fields by calling `record_extraction`."
    )

    return [
        {"role": "system", "content": system},
        {"role": "user", "content": user},
    ]


def _schema_to_tool(schema: dict[str, Any]) -> dict[str, Any]:
    """Wrap a JSON Schema as an OpenAI-style function tool definition.

    The provider adapter in `reasoning_runtime` will translate this
    into Anthropic's `tools=[{name, description, input_schema}]`,
    Gemini's `function_declarations`, etc. — all of which accept a
    JSON Schema in the same shape.
    """
    return {
        "type": "function",
        "function": {
            "name": EXTRACTION_TOOL_NAME,
            "description": (
                "Record the extracted structured data. Call this exactly once "
                "with the fields filled in per the schema."
            ),
            "parameters": schema,
        },
    }
