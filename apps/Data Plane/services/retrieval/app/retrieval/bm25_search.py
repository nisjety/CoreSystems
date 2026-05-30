"""BM25 lexical search — Phase 2.3 hybrid retrieval.

BM25 runs on the same Qdrant candidate set as a lightweight re-scorer,
*not* as a separate full-text index. This avoids maintaining a second
index while still boosting keyword-heavy queries.

The workflow:
  1. Pull a larger candidate pool from Qdrant (top_k * 2) to ensure
     sufficient coverage for lexical matching.
  2. Tokenize query + candidate texts with simple whitespace + lowercasing.
  3. Score each candidate with BM25 (Okapi BM25 implementation from rank_bm25).
  4. Return candidates annotated with bm25_score.

The caller (pipeline.py) fuses dense scores + BM25 scores using
Reciprocal Rank Fusion (RRF).
"""
from __future__ import annotations

import logging
import re
from typing import Any, Dict, List

logger = logging.getLogger(__name__)

_BM25_AVAILABLE = False
try:
    from rank_bm25 import BM25Okapi  # type: ignore[import-untyped]

    _BM25_AVAILABLE = True
except ImportError:
    logger.info("rank_bm25 not installed — BM25 search disabled")


def _tokenize(text: str) -> List[str]:
    """Simple whitespace tokenizer with lowercasing and punctuation stripping."""
    return re.findall(r"\w+", text.lower())


def is_available() -> bool:
    return _BM25_AVAILABLE


def bm25_score(
    query: str,
    candidates: List[Dict[str, Any]],
) -> List[Dict[str, Any]]:
    """Score *candidates* against *query* using BM25.

    Returns a copy of each candidate dict with an added ``bm25_score`` field,
    sorted by bm25_score descending.
    """
    if not _BM25_AVAILABLE or not candidates:
        return candidates

    query_tokens = _tokenize(query)
    if not query_tokens:
        return candidates

    corpus = [_tokenize(c.get("text", "")) for c in candidates]
    bm25 = BM25Okapi(corpus)
    scores = bm25.get_scores(query_tokens)

    scored = []
    for candidate, score in zip(candidates, scores):
        annotated = candidate.copy()
        annotated["bm25_score"] = float(score)
        scored.append(annotated)

    scored.sort(key=lambda c: c["bm25_score"], reverse=True)
    logger.debug(
        "bm25_scored candidates=%d top_score=%.3f",
        len(scored),
        scored[0]["bm25_score"] if scored else 0.0,
    )
    return scored


def reciprocal_rank_fusion(
    dense_ranked: List[Dict[str, Any]],
    bm25_ranked: List[Dict[str, Any]],
    k: int = 60,
) -> List[Dict[str, Any]]:
    """Fuse two ranked lists using Reciprocal Rank Fusion (RRF).

    RRF score = sum(1 / (k + rank_i)) across all lists where the item appears.
    Default k=60 follows the original Cormack et al. paper.

    Returns a de-duplicated list sorted by fused RRF score (desc).
    """
    scores: Dict[str, float] = {}
    items: Dict[str, Dict[str, Any]] = {}

    for rank, item in enumerate(dense_ranked):
        kid = item["knowledge_id"]
        scores[kid] = scores.get(kid, 0.0) + 1.0 / (k + rank + 1)
        items[kid] = item

    for rank, item in enumerate(bm25_ranked):
        kid = item["knowledge_id"]
        scores[kid] = scores.get(kid, 0.0) + 1.0 / (k + rank + 1)
        if kid not in items:
            items[kid] = item

    fused = []
    for kid, rrf_score in sorted(scores.items(), key=lambda x: x[1], reverse=True):
        entry = items[kid].copy()
        entry["rrf_score"] = rrf_score
        fused.append(entry)

    return fused
