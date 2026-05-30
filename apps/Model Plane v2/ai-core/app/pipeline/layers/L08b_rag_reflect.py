"""Layer 8b — RAG Reflection + Synthesis.

Runs after L08 (execute) when context documents were injected by L05.
1. Evaluates whether the LLM answer is grounded in the retrieved context.
2. If insufficient and synthesis is enabled, re-generates a grounded answer.

Skips silently when:
- No context_documents on ctx (non-RAG request)
- No LLM result in ctx._raw["result"]
- rag_reflection_enabled is False in settings

Always fail-open — never blocks the pipeline.
"""

from __future__ import annotations

import logging

from app.config import get_settings
from app.domain import PipelineContext

logger = logging.getLogger(__name__)


async def run(ctx: PipelineContext) -> PipelineContext:
    raw = ctx._raw  # type: ignore[attr-defined]
    settings = get_settings()

    # Skip if reflection is disabled or no context docs present
    if not settings.rag_reflection_enabled:
        ctx.rag_reflection_verdict = "skipped"
        return ctx

    result = raw.get("result")
    if not result or not result.content:
        ctx.rag_reflection_verdict = "skipped"
        return ctx

    if not ctx.context_documents:
        ctx.rag_reflection_verdict = "skipped"
        return ctx

    query: str = raw.get("message", "")
    model: str = settings.intent_model  # reuse cheap fast model

    from app.services.rag_reflection import get_rag_reflection_service

    reflection = await get_rag_reflection_service().evaluate(
        query=query,
        context_docs=ctx.context_documents,
        answer=result.content,
        model=model,
        org_id=ctx.org_id,
        run_id=ctx.run_id,
    )

    ctx.rag_reflection_score = reflection.score
    ctx.rag_reflection_verdict = reflection.verdict

    if reflection.verdict == "insufficient" and settings.rag_synthesis_enabled:
        logger.info(
            "rag_reflect insufficient score=%.2f — synthesising request_id=%s",
            reflection.score, ctx.request_id,
        )
        from app.services.rag_synthesis import get_rag_synthesis_service

        synthesised = await get_rag_synthesis_service().synthesize(
            query=query,
            context_docs=ctx.context_documents,
            model=ctx.resolved_model or model,
            org_id=ctx.org_id,
            run_id=ctx.run_id,
        )

        if synthesised:
            # Patch result content in-place on the raw dict
            # (result is a CompletionResponse dataclass — replace whole object)
            from dataclasses import replace as dc_replace

            raw["result"] = dc_replace(result, content=synthesised)
            ctx.rag_synthesis_used = True
            logger.debug("rag_synthesis patched result request_id=%s", ctx.request_id)

    return ctx
