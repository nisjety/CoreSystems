"""Agentic RAG agents package.

Six agents that together form the agentic retrieval pipeline:

1. PlanningAgent    — decomposes the user query into focused sub-queries
2. RoutingAgent     — decides which retrieval strategy applies to each sub-query
3. RetrievalAgent   — executes retrieval (delegates to existing pipeline.py)
4. RerankingAgent   — cross-encodes and scores candidates (delegates to rerank.py)
5. ReflectionAgent  — evaluates whether the retrieved context is sufficient
6. SynthesisAgent   — synthesises a final answer from the confirmed context
"""

from app.retrieval.agents.planning import PlanningAgent
from app.retrieval.agents.routing import RoutingAgent
from app.retrieval.agents.reflection import ReflectionAgent
from app.retrieval.agents.synthesis import SynthesisAgent

__all__ = [
    "PlanningAgent",
    "RoutingAgent",
    "ReflectionAgent",
    "SynthesisAgent",
]
