"""Synthesis Agent — produces the final grounded answer from retrieved facts.

The Synthesis Agent is the last step in the agentic RAG pipeline.
It receives:
  - The original user question
  - All confirmed (post-reflection) fact chunks and their sources

It produces a grounded answer that:
  1. Answers only from the provided evidence
  2. Cites source document IDs inline
  3. Explicitly states uncertainty when evidence is incomplete
  4. Never hallucates beyond the provided facts
"""

from __future__ import annotations

import logging
from dataclasses import dataclass, field

import httpx

from app.config import settings

logger = logging.getLogger(__name__)

_SYSTEM_PROMPT = """\
You are a precise, grounded question-answering assistant.

Answer the question using ONLY the evidence provided below.
Follow these rules strictly:

1. Base your answer solely on the provided evidence.
2. Cite the source of each claim using [doc_id] inline notation.
3. If the evidence does not contain enough information to fully answer the question,
   clearly state what is missing and what you CAN answer from the evidence.
4. Do NOT invent, infer beyond evidence, or draw on external knowledge.
5. Be concise but complete.

Evidence format:
[doc_id] text snippet

Begin your answer directly. No preamble.
"""

_http: httpx.AsyncClient | None = None


def _client() -> httpx.AsyncClient:
    global _http
    if _http is None:
        _http = httpx.AsyncClient(timeout=60.0)
    return _http


@dataclass
class SynthesisResult:
    answer: str
    sources: list[dict] = field(default_factory=list)
    model_used: str = ""
    input_tokens: int = 0
    output_tokens: int = 0
    low_confidence: bool = False


class SynthesisAgent:
    """Synthesises a final grounded answer from confirmed facts."""

    async def synthesise(
        self,
        question: str,
        facts: list[dict],
        sources: list[dict],
        low_confidence: bool = False,
        org_id: str = "",
    ) -> SynthesisResult:
        """Generate a grounded answer for *question* based on *facts*.

        Falls back gracefully to concatenating fact texts if the LLM is unavailable.
        """
        if not facts:
            return SynthesisResult(
                answer="I was unable to find relevant information to answer this question.",
                sources=[],
                low_confidence=True,
            )

        # Build evidence block with document citations
        evidence_lines = []
        for fact in facts:
            doc_id = fact.get("document_id", "unknown")
            text = fact.get("text", "").strip()
            evidence_lines.append(f"[{doc_id}] {text}")
        evidence_block = "\n\n".join(evidence_lines)

        user_message = f"Question: {question}\n\nEvidence:\n{evidence_block}"

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
                    "temperature": 0.1,
                    "max_tokens": 2048,
                },
            )
            if resp.status_code == 200:
                data = resp.json()
                answer = (data.get("content") or "").strip()
                usage = data.get("metadata") or {}
                logger.info(
                    "synthesis_agent question=%r answer_len=%d sources=%d",
                    question[:60], len(answer), len(sources),
                )
                return SynthesisResult(
                    answer=answer,
                    sources=sources,
                    model_used=data.get("model_used", settings.agentic_rag_model),
                    input_tokens=usage.get("input_tokens", 0),
                    output_tokens=usage.get("output_tokens", 0),
                    low_confidence=low_confidence,
                )
            logger.warning("synthesis_agent_llm_error status=%d", resp.status_code)
        except Exception as exc:
            logger.warning("synthesis_agent_exception err=%s — falling back to fact concat", exc)

        # Fallback: concatenate top facts
        fallback_text = "\n\n".join(f.get("text", "")[:300] for f in facts[:3])
        return SynthesisResult(
            answer=fallback_text,
            sources=sources,
            low_confidence=True,
        )
