"""RAG Reflection Service.

After LLM execution, evaluates whether the response is well-grounded in the
retrieved context documents.  If the quality score is below the threshold the
layer signals that synthesis (re-generation) should be attempted.

Fail-open: any exception returns a "sufficient" verdict so the pipeline
continues uninterrupted.
"""

from __future__ import annotations

import json
import logging
from dataclasses import dataclass

logger = logging.getLogger(__name__)

_REFLECTION_SYSTEM_PROMPT = """\
You are a RAG quality evaluator.  Given a question, retrieved context
documents, and a generated answer, evaluate whether the answer is:
1. Grounded in the provided context (not hallucinated).
2. Sufficiently complete given the retrieved documents.

Respond with JSON only, no prose:
{
  "verdict": "sufficient" | "insufficient",
  "score": <0.0-1.0>,
  "reason": "<one sentence>"
}

- sufficient: answer faithfully uses the context; score >= 0.6
- insufficient: answer ignores context, is vague, or contains unsupported claims
"""

_REFLECTION_THRESHOLD = 0.6


@dataclass
class ReflectionResult:
    verdict: str          # "sufficient" | "insufficient" | "skipped"
    score: float
    reason: str


_instance: RagReflectionService | None = None


class RagReflectionService:
    """Evaluate RAG answer quality via LLM reflection."""

    async def evaluate(
        self,
        *,
        query: str,
        context_docs: list[dict],
        answer: str,
        model: str,
        org_id: str = "",
        run_id: str = "",
    ) -> ReflectionResult:
        """Return a ReflectionResult.  Never raises."""
        if not context_docs or not answer:
            return ReflectionResult(verdict="skipped", score=1.0, reason="no context or answer")

        context_text = "\n\n---\n\n".join(
            d.get("content", d.get("text", "")) for d in context_docs[:5]
        )

        user_msg = (
            f"Question: {query}\n\n"
            f"Context documents:\n{context_text[:3000]}\n\n"
            f"Generated answer:\n{answer[:1500]}"
        )

        try:
            from reasoning_runtime import execute
            from app.domain import CompletionRequest, Message, Provider

            req = CompletionRequest(
                provider=Provider.OPENAI,
                model=model,
                messages=[
                    Message(role="system", content=_REFLECTION_SYSTEM_PROMPT),
                    Message(role="user", content=user_msg),
                ],
                temperature=0.0,
                max_tokens=128,
                org_id=org_id,
                run_id=run_id,
            )
            result = await execute(req)
            data = json.loads(result.content)
            verdict = data.get("verdict", "sufficient")
            score = float(data.get("score", 1.0))
            reason = data.get("reason", "")

            logger.debug(
                "rag_reflection verdict=%s score=%.2f org_id=%s",
                verdict, score, org_id,
            )
            return ReflectionResult(verdict=verdict, score=score, reason=reason)

        except Exception as exc:
            logger.warning("rag_reflection_failed error=%s org_id=%s", exc, org_id)
            return ReflectionResult(verdict="sufficient", score=1.0, reason=f"eval_error: {exc}")


def get_rag_reflection_service() -> RagReflectionService:
    global _instance
    if _instance is None:
        _instance = RagReflectionService()
    return _instance
