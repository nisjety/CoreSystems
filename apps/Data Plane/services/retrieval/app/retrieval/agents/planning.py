"""Planning Agent — decomposes a complex query into focused sub-queries.

Calls ai-core v2 (POST /api/v1/chat) to ask the LLM to decompose a
multi-faceted question into ≤N independent, searchable sub-queries.

If the model returns a non-decomposable question, it passes it through
unchanged as a single-item list.
"""

from __future__ import annotations

import json
import logging

import httpx

from app.config import settings

logger = logging.getLogger(__name__)

_SYSTEM_PROMPT = """\
You are a search query planning assistant.
Your task is to decompose a complex user question into independent, focused sub-queries
that can each be answered with a targeted document search.

Rules:
- Return ONLY a JSON array of strings: ["sub-query 1", "sub-query 2", ...]
- Produce at most {max_subqueries} sub-queries.
- If the question is simple and cannot be meaningfully decomposed, return a single-element array.
- Each sub-query must be self-contained (no pronouns referring to other sub-queries).
- Do NOT include any other text, commentary, or markdown.
"""

_http: httpx.AsyncClient | None = None


def _client() -> httpx.AsyncClient:
    global _http
    if _http is None:
        _http = httpx.AsyncClient(timeout=30.0)
    return _http


class PlanningAgent:
    """Decomposes the user query into up to *max_subqueries* sub-queries."""

    def __init__(self, max_subqueries: int | None = None) -> None:
        self._max = max_subqueries or settings.agentic_rag_max_subqueries

    async def plan(self, query: str, org_id: str = "") -> list[str]:
        """Return a list of sub-queries derived from *query*.

        Falls back to ``[query]`` if the LLM call fails or returns unparseable output.
        """
        system = _SYSTEM_PROMPT.format(max_subqueries=self._max)

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
                        {"role": "system", "content": system},
                        {"role": "user",   "content": query},
                    ],
                    "temperature": 0.0,
                    "max_tokens": 512,
                },
            )
            if resp.status_code != 200:
                logger.warning("planning_agent_llm_error status=%d", resp.status_code)
                return [query]

            content = (resp.json().get("content") or "").strip()
            # Strip markdown code fences if present
            if content.startswith("```"):
                content = content.split("```")[1].lstrip("json").strip()

            subqueries: list[str] = json.loads(content)
            if not isinstance(subqueries, list) or not subqueries:
                return [query]

            cleaned = [s.strip() for s in subqueries if isinstance(s, str) and s.strip()]
            logger.info(
                "planning_agent query=%r decomposed_into=%d subqueries",
                query[:60], len(cleaned),
            )
            return cleaned or [query]

        except Exception as exc:
            logger.warning("planning_agent_error err=%s — falling back to original query", exc)
            return [query]
