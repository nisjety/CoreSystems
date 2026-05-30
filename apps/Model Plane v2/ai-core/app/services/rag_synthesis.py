"""RAG Synthesis Service.

When RAG reflection marks an answer as insufficient, this service re-generates
the answer with an explicit "use only the provided context" system prompt.

Fail-open: any exception returns the original answer unchanged.
"""

from __future__ import annotations

import logging

logger = logging.getLogger(__name__)

_SYNTHESIS_SYSTEM_PROMPT = """\
You are a precise answer synthesiser.  Answer the question using ONLY the
information in the provided context documents.  Do NOT use prior knowledge.
If the context does not contain enough information, say so clearly.
Be concise and cite the relevant document snippets where appropriate.
"""

_instance: RagSynthesisService | None = None


class RagSynthesisService:
    """Re-generate an answer grounded strictly in retrieved context."""

    async def synthesize(
        self,
        *,
        query: str,
        context_docs: list[dict],
        model: str,
        org_id: str = "",
        run_id: str = "",
    ) -> str:
        """Return a synthesised answer string.  Never raises."""
        if not context_docs:
            return ""

        context_text = "\n\n---\n\n".join(
            d.get("content", d.get("text", "")) for d in context_docs[:8]
        )

        user_msg = (
            f"Context documents:\n{context_text[:4000]}\n\n"
            f"Question: {query}"
        )

        try:
            from reasoning_runtime import execute
            from app.domain import CompletionRequest, Message, Provider

            req = CompletionRequest(
                provider=Provider.OPENAI,
                model=model,
                messages=[
                    Message(role="system", content=_SYNTHESIS_SYSTEM_PROMPT),
                    Message(role="user", content=user_msg),
                ],
                temperature=0.2,
                max_tokens=1024,
                org_id=org_id,
                run_id=run_id,
            )
            result = await execute(req)
            logger.debug(
                "rag_synthesis complete tokens_out=%d org_id=%s",
                result.tokens_out, org_id,
            )
            return result.content

        except Exception as exc:
            logger.warning("rag_synthesis_failed error=%s org_id=%s", exc, org_id)
            return ""


def get_rag_synthesis_service() -> RagSynthesisService:
    global _instance
    if _instance is None:
        _instance = RagSynthesisService()
    return _instance
