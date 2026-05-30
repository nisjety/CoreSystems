"""Routing Agent — selects retrieval strategy for each sub-query.

Decides which combination of retrieval modes to use:
  - "dense"        — pure vector search (semantic similarity)
  - "hybrid"       — BM25 + dense (the default pipeline mode)
  - "keyword"      — BM25-dominant (exact terminology, codes)
  - "structured"   — metadata-first (date ranges, document type filters)

The routing decision is made via an LLM call when enabled, or falls back to
a heuristic (keyword-rich → hybrid, question-form → dense).
"""

from __future__ import annotations

import logging
import re
from typing import Literal

import httpx

from app.config import settings

logger = logging.getLogger(__name__)

RetrievalMode = Literal["dense", "hybrid", "keyword", "structured"]

_SYSTEM_PROMPT = """\
You are a retrieval strategy router for a document search system.

Given a search sub-query, classify it into exactly one of these retrieval modes:
- "dense"      : Open-ended questions, conceptual topics, semantic similarity
- "hybrid"     : Mixed questions with both keywords and conceptual elements (default)
- "keyword"    : Exact terms, codes, IDs, names, technical jargon, abbreviations
- "structured" : Queries with date/time, status, type, or other structured filters

Respond with ONLY the mode string (one of: dense, hybrid, keyword, structured).
No explanation, no punctuation.
"""

_http: httpx.AsyncClient | None = None


def _client() -> httpx.AsyncClient:
    global _http
    if _http is None:
        _http = httpx.AsyncClient(timeout=15.0)
    return _http


# ── Heuristic fallback ────────────────────────────────────────────────────────

_KEYWORD_PATTERNS = re.compile(
    r"\b([A-Z]{2,8}-\d+|[A-Z]{3,}\d+|ISO\s?\d+|RFC\s?\d+|CVE-\d{4}-\d+)\b"
)
_STRUCTURED_PATTERNS = re.compile(
    r"\b(before|after|between|since|until|from \d|in \d{4}|status:|type:)\b", re.I
)


def _heuristic_route(query: str) -> RetrievalMode:
    if _KEYWORD_PATTERNS.search(query):
        return "keyword"
    if _STRUCTURED_PATTERNS.search(query):
        return "structured"
    # Questions are semantic → dense; statements with multiple nouns → hybrid
    if query.strip().endswith("?") or query.lower().startswith(("what ", "why ", "how ", "explain")):
        return "dense"
    return "hybrid"


class RoutingAgent:
    """Routes each sub-query to the best retrieval strategy."""

    async def route(self, query: str, org_id: str = "") -> RetrievalMode:
        """Return the retrieval mode for *query*."""
        try:
            resp = await _client().post(
                f"{settings.ai_core_url}/api/v1/chat",
                headers={
                    "x-internal-key": settings.internal_api_key,
                    "Content-Type": "application/json",
                },
                json={
                    "model": settings.agentic_rag_model,
                    "org_id": org_id,
                    "messages": [
                        {"role": "system", "content": _SYSTEM_PROMPT},
                        {"role": "user",   "content": query},
                    ],
                    "temperature": 0.0,
                    "max_tokens": 16,
                },
            )
            if resp.status_code == 200:
                mode_str = (resp.json().get("content") or "").strip().lower()
                if mode_str in ("dense", "hybrid", "keyword", "structured"):
                    logger.debug("routing_agent query=%r mode=%s", query[:40], mode_str)
                    return mode_str  # type: ignore[return-value]
        except Exception as exc:
            logger.debug("routing_agent_llm_unavailable err=%s — using heuristic", exc)

        # Heuristic fallback
        mode = _heuristic_route(query)
        logger.debug("routing_agent_heuristic query=%r mode=%s", query[:40], mode)
        return mode
