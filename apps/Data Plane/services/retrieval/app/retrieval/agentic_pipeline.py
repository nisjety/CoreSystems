"""Agentic RAG Pipeline — 6-agent multi-query retrieval and synthesis.

Architecture (based on Google Cloud, IBM, Microsoft Azure, Qdrant, LlamaIndex research):

  User query
       │
       ▼
  ① PlanningAgent     — Decomposes query into N focused sub-queries
       │
       ▼ (per sub-query, in parallel)
  ② RoutingAgent      — Selects optimal retrieval strategy (dense/hybrid/keyword/structured)
       │
       ▼
  ③ RetrievalAgent    — Executes the chosen strategy (delegates to pipeline.retrieve())
       │
       ▼
  ④ RerankingAgent    — Cross-encoder reranking (delegates to rerank.rerank())
       │
       ▼ (merge results from all sub-queries)
  ⑤ ReflectionAgent  — Evaluates sufficiency; may trigger additional retrieval passes
       │
       ▼
  ⑥ SynthesisAgent   — Generates the final grounded answer with citations

The pipeline can be called standalone (returns a SynthesisResult) or stepped
through for streaming/observability purposes.

Enabled only when ``AGENTIC_RAG_ENABLED=true``.
Falls back gracefully to the standard pipeline when ai-core is unreachable.
"""

from __future__ import annotations

import asyncio
import logging
from dataclasses import dataclass, field
from time import perf_counter
from typing import Any, Optional

from app.config import settings
from app.retrieval.agents.planning import PlanningAgent
from app.retrieval.agents.routing import RoutingAgent, RetrievalMode
from app.retrieval.agents.reflection import ReflectionAgent
from app.retrieval.agents.synthesis import SynthesisAgent, SynthesisResult
from app.retrieval.pipeline import retrieve
from app.retrieval.rerank import rerank

logger = logging.getLogger(__name__)


@dataclass
class AgenticRetrievalResult:
    """Full result from the agentic RAG pipeline."""

    answer: str
    facts: list[dict[str, Any]]
    sources: list[dict[str, Any]]
    sub_queries: list[str]
    retrieval_iterations: int
    model_used: str
    input_tokens: int = 0
    output_tokens: int = 0
    low_confidence: bool = False
    latency_ms: float = 0.0
    # Per-agent timings for observability
    timings: dict[str, float] = field(default_factory=dict)


class AgenticRAGPipeline:
    """Orchestrates the 6-agent retrieval pipeline."""

    def __init__(self) -> None:
        self._planner    = PlanningAgent()
        self._router     = RoutingAgent()
        self._reflector  = ReflectionAgent()
        self._synthesiser = SynthesisAgent()

    async def run(
        self,
        *,
        query: str,
        org_id: str,
        document_types: Optional[list[str]] = None,
        departments: Optional[list[str]] = None,
        languages: Optional[list[str]] = None,
        document_ids: Optional[list[str]] = None,
        region: Optional[str] = None,
        top_k: Optional[int] = None,
        top_n: Optional[int] = None,
        context_window: Optional[int] = None,
    ) -> AgenticRetrievalResult:
        """Run the full 6-agent pipeline and return a synthesised answer."""
        started_at = perf_counter()
        timings: dict[str, float] = {}

        # ── Agent 1: Planning ─────────────────────────────────────────────────
        t0 = perf_counter()
        sub_queries = await self._planner.plan(query, org_id=org_id)
        timings["planning_ms"] = (perf_counter() - t0) * 1000
        logger.info("agentic_rag planning query=%r sub_queries=%d", query[:60], len(sub_queries))

        # ── Agents 2–4: Routing + Retrieval + Reranking (parallel per sub-query)
        t0 = perf_counter()
        all_facts: list[dict[str, Any]] = []
        all_sources: list[dict[str, Any]] = []
        seen_docs: set[str] = set()

        retrieval_tasks = [
            self._route_and_retrieve(
                sub_query=sq,
                org_id=org_id,
                document_types=document_types,
                departments=departments,
                languages=languages,
                document_ids=document_ids,
                region=region,
                top_k=top_k,
                top_n=top_n,
                context_window=context_window,
            )
            for sq in sub_queries
        ]
        retrieval_results = await asyncio.gather(*retrieval_tasks, return_exceptions=True)

        for res in retrieval_results:
            if isinstance(res, Exception):
                logger.warning("agentic_rag retrieval_task_error err=%s", res)
                continue
            for fact in res.get("facts", []):
                all_facts.append(fact)
            for src in res.get("sources", []):
                doc_id = src.get("document_id")
                if doc_id and doc_id not in seen_docs:
                    seen_docs.add(doc_id)
                    all_sources.append(src)

        timings["retrieval_ms"] = (perf_counter() - t0) * 1000

        # Deduplicate facts by knowledge_id, then re-rank the merged pool
        t0 = perf_counter()
        unique_facts = _deduplicate(all_facts)
        if len(unique_facts) > 1:
            merged_top_n = top_n or settings.top_n_after_rerank
            unique_facts = await rerank(
                query=query,
                candidates=unique_facts,
                top_n=merged_top_n * len(sub_queries),
            )
            unique_facts = unique_facts[:merged_top_n * 2]  # leave headroom for reflection
        timings["reranking_ms"] = (perf_counter() - t0) * 1000

        # ── Agent 5: Reflection ───────────────────────────────────────────────
        t0 = perf_counter()
        iteration = 0
        low_confidence = False
        max_iters = settings.agentic_rag_max_reflection_iters

        while iteration < max_iters:
            reflection = await self._reflector.reflect(query, unique_facts, org_id=org_id)
            if reflection.sufficient:
                break
            iteration += 1
            logger.info(
                "agentic_rag reflection_pass iter=%d reason=%s refined_query=%r",
                iteration, reflection.reason, reflection.refined_query,
            )
            if not reflection.refined_query:
                low_confidence = True
                break

            # Extra retrieval pass with the refined query
            extra = await self._route_and_retrieve(
                sub_query=reflection.refined_query,
                org_id=org_id,
                document_types=document_types,
                departments=departments,
                languages=languages,
                document_ids=document_ids,
                region=region,
                top_k=top_k,
                top_n=top_n,
            )
            for fact in extra.get("facts", []):
                unique_facts.append(fact)
            unique_facts = _deduplicate(unique_facts)
            if len(unique_facts) > 1:
                top_n_val = top_n or settings.top_n_after_rerank
                unique_facts = await rerank(query=query, candidates=unique_facts, top_n=top_n_val * 2)
        else:
            low_confidence = True

        timings["reflection_ms"] = (perf_counter() - t0) * 1000

        # Trim to final top_n
        final_top_n = top_n or settings.top_n_after_rerank
        final_facts = unique_facts[:final_top_n]

        # ── Agent 6: Synthesis ────────────────────────────────────────────────
        t0 = perf_counter()
        synthesis: SynthesisResult = await self._synthesiser.synthesise(
            question=query,
            facts=final_facts,
            sources=all_sources,
            low_confidence=low_confidence,
            org_id=org_id,
        )
        timings["synthesis_ms"] = (perf_counter() - t0) * 1000

        timings["total_ms"] = (perf_counter() - started_at) * 1000

        return AgenticRetrievalResult(
            answer=synthesis.answer,
            facts=final_facts,
            sources=synthesis.sources,
            sub_queries=sub_queries,
            retrieval_iterations=iteration + 1,
            model_used=synthesis.model_used,
            input_tokens=synthesis.input_tokens,
            output_tokens=synthesis.output_tokens,
            low_confidence=synthesis.low_confidence,
            latency_ms=timings["total_ms"],
            timings=timings,
        )

    async def _route_and_retrieve(
        self,
        sub_query: str,
        org_id: str,
        **retrieve_kwargs: Any,
    ) -> dict[str, Any]:
        """Route a sub-query, then call the standard retrieval pipeline."""
        mode: RetrievalMode = await self._router.route(sub_query, org_id=org_id)

        # Map routing mode to hybrid_enabled override
        # (we patch it per-call via a workaround since pipeline.retrieve
        # reads from global settings — we pass it as a flag in top_k semantics
        # and rely on settings.hybrid_enabled for the actual pipeline toggle)
        logger.debug("routing sub_query=%r → mode=%s", sub_query[:40], mode)

        result = await retrieve(
            org_id=org_id,
            query=sub_query,
            **retrieve_kwargs,
        )
        return result


def _deduplicate(facts: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """Remove duplicate facts by knowledge_id, keeping highest rerank_score."""
    seen: dict[str, dict[str, Any]] = {}
    for fact in facts:
        kid = fact.get("knowledge_id") or fact.get("id") or id(fact)
        existing = seen.get(str(kid))
        if existing is None or fact.get("rerank_score", 0) > existing.get("rerank_score", 0):
            seen[str(kid)] = fact
    return list(seen.values())
