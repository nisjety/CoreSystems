"""Skills registry — match skills to a user's goal or context.

Hybrid matching strategy:
1. **Keyword matching** — trigger_keywords + file patterns (fast, deterministic)
2. **Semantic matching** — embedding cosine similarity via reasoning_runtime (deep)

Results are combined: ``0.4 × keyword_score + 0.6 × semantic_score``.
Falls back to keyword-only if embeddings are unavailable.

Matched results are cached in Redis for 5 minutes to reduce re-computation.
"""

from __future__ import annotations

import hashlib
import json
import logging
import math
from typing import Any

from app.skills.domain import SkillConfig, SkillMatch
from app.skills.loader import load_skills_for_org

logger = logging.getLogger(__name__)

# Redis TTL for cached skill matches (seconds)
_MATCH_CACHE_TTL = 300

# Embedding cache: skill_id → embedding vector (populated on first match call)
_EMBEDDING_CACHE: dict[str, list[float]] = {}


async def match_skills(
    org_id: str,
    goal: str,
    file_paths: list[str] | None = None,
    redis: Any | None = None,
) -> list[SkillMatch]:
    """Match skills relevant to the goal and optional file context.

    Uses hybrid matching: keyword + semantic (embedding cosine similarity).
    Falls back to keyword-only if embedding generation fails.

    Args:
        org_id: Organisation identifier.
        goal: The agent goal / user prompt.
        file_paths: Optional list of file paths in context (for pattern matching).
        redis: Optional Redis client for result caching.
    """
    from app.config import settings

    threshold = settings.skill_match_threshold

    # Try Redis cache
    if redis is not None:
        cache_key = _make_cache_key(org_id, goal, file_paths or [])
        cached = await redis.get(cache_key)
        if cached:
            try:
                raw = json.loads(cached)
                return [SkillMatch(**item) for item in raw]
            except Exception:
                pass  # Cache miss on parse error

    skills = await load_skills_for_org(org_id)
    if not skills:
        return []

    goal_lower = goal.lower()

    # Try to get goal embedding for semantic matching
    goal_embedding = await _get_embedding(goal)

    matches: list[SkillMatch] = []

    for skill in skills:
        keyword_score = _compute_keyword_score(skill, goal_lower, file_paths or [])

        if goal_embedding is not None:
            skill_embedding = await _get_skill_embedding(skill)
            semantic_score = _cosine_similarity(goal_embedding, skill_embedding) if skill_embedding else 0.0
            # Hybrid: 40% keyword + 60% semantic
            score = 0.4 * keyword_score + 0.6 * semantic_score
        else:
            score = keyword_score

        if score >= threshold:
            matches.append(
                SkillMatch(
                    skill_id=skill.id,
                    name=skill.name,
                    content=skill.content,
                    tool_restrictions=skill.tool_restrictions,
                    match_score=score,
                )
            )

    # Sort by relevance
    matches.sort(key=lambda m: m.match_score, reverse=True)

    # Cache result
    if redis is not None:
        try:
            cache_key = _make_cache_key(org_id, goal, file_paths or [])
            await redis.setex(
                cache_key,
                _MATCH_CACHE_TTL,
                json.dumps([m.model_dump() for m in matches]),
            )
        except Exception as exc:
            logger.debug("skill_match_cache_write_error", extra={"error": str(exc)})

    return matches


# ── Embedding helpers ───────────────────────────────────────────────────


async def _get_embedding(text: str) -> list[float] | None:
    """Generate an embedding vector via reasoning_runtime.

    Returns None if embedding generation fails (graceful fallback).
    """
    try:
        from reasoning_runtime import execute
        from reasoning_runtime.domain import CompletionRequest, Message, Provider
        from uuid import uuid4

        # Use a lightweight embedding call
        req = CompletionRequest(
            request_id=uuid4().hex,
            model_id="text-embedding-3-small",
            provider=Provider.OPENAI,
            api_endpoint="",
            messages=[Message(role="user", content=text)],
            stream=False,
            org_id="system",
            run_id=uuid4().hex,
            embedding_mode=True,
        )
        result = await execute(req)

        if hasattr(result, "embedding") and result.embedding:
            return result.embedding
    except Exception as exc:
        logger.debug("embedding_generation_failed", extra={"error": str(exc)})

    return None


async def _get_skill_embedding(skill: SkillConfig) -> list[float] | None:
    """Get or compute cached embedding for a skill."""
    if skill.id in _EMBEDDING_CACHE:
        return _EMBEDDING_CACHE[skill.id]

    # Build representational text from skill metadata
    parts = [skill.name]
    if skill.description:
        parts.append(skill.description)
    if skill.trigger_keywords:
        parts.append(" ".join(skill.trigger_keywords))
    text = " | ".join(parts)

    embedding = await _get_embedding(text)
    if embedding is not None:
        _EMBEDDING_CACHE[skill.id] = embedding
    return embedding


def _cosine_similarity(a: list[float], b: list[float]) -> float:
    """Compute cosine similarity between two vectors."""
    if len(a) != len(b) or not a:
        return 0.0

    dot = sum(x * y for x, y in zip(a, b))
    norm_a = math.sqrt(sum(x * x for x in a))
    norm_b = math.sqrt(sum(x * x for x in b))

    if norm_a == 0 or norm_b == 0:
        return 0.0

    return max(0.0, dot / (norm_a * norm_b))


# ── Key and score helpers ───────────────────────────────────────────────


def _make_cache_key(org_id: str, goal: str, file_paths: list[str]) -> str:
    key_data = f"{org_id}::{goal}::{','.join(sorted(file_paths))}"
    digest = hashlib.sha256(key_data.encode()).hexdigest()[:16]
    return f"skills:match:{org_id}:{digest}"


def _compute_keyword_score(
    skill: SkillConfig,
    goal_lower: str,
    file_paths: list[str],
) -> float:
    """Compute a 0.0-1.0 keyword relevance score for a skill against the goal."""
    score = 0.0

    # Keyword matching
    if skill.trigger_keywords:
        matched_keywords = sum(
            1 for kw in skill.trigger_keywords if kw.lower() in goal_lower
        )
        if matched_keywords > 0:
            score += 0.5 * (matched_keywords / len(skill.trigger_keywords))

    # File pattern matching
    if skill.trigger_file_patterns and file_paths:
        import fnmatch

        matched_files = 0
        for pattern in skill.trigger_file_patterns:
            for fp in file_paths:
                if fnmatch.fnmatch(fp, pattern):
                    matched_files += 1
                    break
        if matched_files > 0:
            score += 0.3 * (matched_files / len(skill.trigger_file_patterns))

    # Description similarity (simple word overlap)
    if skill.description:
        desc_words = set(skill.description.lower().split())
        goal_words = set(goal_lower.split())
        overlap = len(desc_words & goal_words)
        if overlap > 0 and len(desc_words) > 0:
            score += 0.2 * (overlap / len(desc_words))

    return min(score, 1.0)
