"""Reflection Agent — evaluates whether retrieved context is sufficient.

After each retrieval pass, the Reflection Agent asks the LLM:
"Can the following question be answered confidently given only this evidence?"

If the answer is "no" and the max iteration count has not been reached, the
agentic pipeline triggers another retrieval pass with a refined query.

Reflection focuses on:
  1. Completeness  — does the evidence cover all aspects of the question?
  2. Relevance     — is the evidence actually about the question?
  3. Confidence    — is there enough evidence to answer without hallucination?
"""

from __future__ import annotations

import logging
from dataclasses import dataclass

import httpx

from app.config import settings

logger = logging.getLogger(__name__)

_SYSTEM_PROMPT = """\
You are a retrieval quality evaluator for a Retrieval-Augmented Generation (RAG) system.

Given a question and retrieved evidence snippets, assess whether the evidence is
sufficient to answer the question accurately and completely.

Respond with a JSON object with two fields:
  {
    "sufficient": true | false,
    "reason": "brief explanation (1-2 sentences)",
    "refined_query": "a better query to fill the gap (only when sufficient=false, else null)"
  }

Be strict: if ANY important aspect of the question cannot be answered from the evidence,
set sufficient=false and suggest a refined_query to retrieve the missing information.
"""

_http: httpx.AsyncClient | None = None


def _client() -> httpx.AsyncClient:
    global _http
    if _http is None:
        _http = httpx.AsyncClient(timeout=30.0)
    return _http


@dataclass
class ReflectionResult:
    sufficient: bool
    reason: str
    refined_query: str | None  # Only set when sufficient=False


class ReflectionAgent:
    """Evaluates retrieval quality and suggests query refinements."""

    def __init__(self, threshold: float | None = None) -> None:
        self._threshold = threshold or settings.agentic_rag_reflection_threshold

    async def reflect(
        self,
        question: str,
        facts: list[dict],
        org_id: str = "",
    ) -> ReflectionResult:
        """Evaluate whether *facts* are sufficient to answer *question*.

        Falls back to a confidence-score heuristic when the LLM call fails.
        """
        # Heuristic fast-path: if top rerank_score is above threshold and we
        # have ≥ 2 facts, skip the LLM call.
        if facts:
            top_score = facts[0].get("rerank_score", 0.0)
            if top_score >= self._threshold and len(facts) >= 2:
                return ReflectionResult(sufficient=True, reason="heuristic_pass", refined_query=None)

        # Build evidence block (top 5 facts max to stay within context)
        evidence_parts = []
        for i, fact in enumerate(facts[:5]):
            evidence_parts.append(f"[{i+1}] {fact.get('text', '')[:400]}")
        evidence_block = "\n\n".join(evidence_parts) if evidence_parts else "(no evidence retrieved)"

        user_message = (
            f"Question: {question}\n\n"
            f"Evidence:\n{evidence_block}"
        )

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
                        {"role": "user",   "content": user_message},
                    ],
                    "temperature": 0.0,
                    "max_tokens": 256,
                },
            )
            if resp.status_code != 200:
                logger.warning("reflection_agent_llm_error status=%d", resp.status_code)
                return self._heuristic_result(facts)

            content = (resp.json().get("content") or "").strip()
            # Strip markdown fences
            if content.startswith("```"):
                content = content.split("```")[1].lstrip("json").strip()

            import json
            data = json.loads(content)
            result = ReflectionResult(
                sufficient=bool(data.get("sufficient", True)),
                reason=data.get("reason", ""),
                refined_query=data.get("refined_query"),
            )
            logger.info(
                "reflection_agent question=%r sufficient=%s reason=%s",
                question[:60], result.sufficient, result.reason,
            )
            return result

        except Exception as exc:
            logger.warning("reflection_agent_exception err=%s — using heuristic", exc)
            return self._heuristic_result(facts)

    def _heuristic_result(self, facts: list[dict]) -> ReflectionResult:
        """Fallback: mark as sufficient if any fact has score above threshold."""
        if not facts:
            return ReflectionResult(
                sufficient=False,
                reason="no_facts_retrieved",
                refined_query=None,
            )
        top_score = facts[0].get("rerank_score", 0.0)
        sufficient = top_score >= self._threshold
        return ReflectionResult(
            sufficient=sufficient,
            reason=f"heuristic top_score={top_score:.3f} threshold={self._threshold}",
            refined_query=None,
        )
