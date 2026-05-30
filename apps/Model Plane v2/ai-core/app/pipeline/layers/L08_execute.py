"""Layer 8 — LLM execution.

Dispatches the request to reasoning_runtime.execute() and stores
the CompletionResponse back into the pipeline context.

NOTE: For streaming, the runner bypasses this layer and calls
execute_stream() directly. This layer handles the non-streaming path.
"""

from __future__ import annotations

import logging

from reasoning_runtime import execute
from reasoning_runtime.domain import CompletionRequest, Message

from app.domain import PipelineContext, Provider

logger = logging.getLogger(__name__)


async def run(ctx: PipelineContext) -> PipelineContext:
    raw = ctx._raw
    message: str = raw.get("message", "")

    messages: list[Message] = []
    if ctx.system_prompt:
        messages.append(Message(role="system", content=ctx.system_prompt))

    # Inject RAG context documents into the prompt
    if ctx.context_documents:
        docs_text = "\n\n".join(
            f"[Document {i+1}] {doc.get('title', '')}\n{doc.get('content', '')}"
            for i, doc in enumerate(ctx.context_documents)
        )
        messages.append(
            Message(role="system", content=f"Context documents:\n{docs_text}")
        )

    messages.append(Message(role="user", content=message))

    req = CompletionRequest(
        request_id=ctx.request_id,
        provider=ctx.resolved_provider or Provider.OPENAI,
        model_id=ctx.resolved_model or "gpt-4o-mini",
        messages=messages,
        temperature=raw.get("temperature", 0.7),
        max_tokens=raw.get("max_tokens"),
        tools=raw.get("tools") or None,
        stream=False,
        org_id=ctx.org_id,
        run_id=ctx.run_id or ctx.request_id,
    )

    result = await execute(req)

    # Stash the CompletionResponse for downstream layers
    raw["result"] = result

    logger.info(
        "L08_execute model=%s tokens_in=%d tokens_out=%d request_id=%s",
        result.model_used,
        result.tokens_in,
        result.tokens_out,
        ctx.request_id,
    )
    return ctx
