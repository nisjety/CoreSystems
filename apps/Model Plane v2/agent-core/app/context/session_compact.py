"""Session compaction — extract facts and write back to memory.

Mirrors CC's sessionMemoryCompact.ts: after auto-compaction, extract
reusable facts from the summarized conversation and persist them to
the agent_memory table so future runs benefit from learned context.

This is the "write" side of the memory system — Phase F was the "read" side.
"""

from __future__ import annotations

import json
import logging
from typing import Any

from app.database import get_pool

logger = logging.getLogger(__name__)

FACT_EXTRACTION_PROMPT = (
    "You are a fact extractor. Given a conversation summary, extract ONLY "
    "reusable facts that would help a future agent working on the same project.\n\n"
    "Categories of facts to extract:\n"
    "- File paths and what they contain\n"
    "- Project conventions and patterns discovered\n"
    "- Important decisions made and their rationale\n"
    "- Errors encountered and their solutions\n"
    "- User preferences and habits\n\n"
    "Output a JSON array of objects:\n"
    '[{{"key": "<short_identifier>", "content": "<the fact>"}}]\n\n'
    "Rules:\n"
    "- Keys should be lowercase, hyphenated (e.g. 'auth-pattern', 'db-schema')\n"
    "- Each fact should be 1-3 sentences max\n"
    "- Output 3-10 facts. Skip trivial or ephemeral information\n"
    "- Output ONLY the JSON array, no markdown"
)


async def extract_and_store_facts(
    org_id: str,
    session_id: str | None,
    summary_text: str,
    llm_client: Any,
) -> int:
    """Extract facts from a conversation summary and upsert to agent_memory.

    Args:
        org_id: Organization ID for scoping.
        session_id: Optional session ID for session-scoped facts.
        summary_text: The compacted summary to extract from.
        llm_client: LLM client for fact extraction.

    Returns:
        Number of facts stored.
    """
    if not summary_text or not org_id:
        return 0

    facts = await _extract_facts(summary_text, llm_client)
    if not facts:
        return 0

    stored = await _upsert_facts(org_id, session_id, facts)

    logger.info(
        "session_facts_stored",
        extra={
            "org_id": org_id,
            "session_id": session_id,
            "extracted": len(facts),
            "stored": stored,
        },
    )

    return stored


async def _extract_facts(
    summary_text: str,
    llm_client: Any,
) -> list[dict[str, str]]:
    """Use LLM to extract reusable facts from a summary."""
    messages = [
        {"role": "system", "content": FACT_EXTRACTION_PROMPT},
        {"role": "user", "content": summary_text},
    ]

    try:
        raw = await llm_client.planner_complete(messages)
    except Exception as exc:
        logger.warning("fact_extraction_failed", extra={"error": str(exc)})
        return []

    # Parse JSON array
    raw = raw.strip()
    if raw.startswith("```"):
        lines = raw.split("\n")
        raw = "\n".join(lines[1:-1])

    try:
        facts = json.loads(raw)
    except json.JSONDecodeError:
        logger.warning("fact_extraction_parse_failed", extra={"raw": raw[:200]})
        return []

    if not isinstance(facts, list):
        return []

    # Validate structure
    valid: list[dict[str, str]] = []
    for f in facts:
        if isinstance(f, dict) and "key" in f and "content" in f:
            key = str(f["key"]).strip()[:100]
            content = str(f["content"]).strip()[:2000]
            if key and content:
                valid.append({"key": key, "content": content})

    return valid[:10]  # Cap at 10 facts


async def _upsert_facts(
    org_id: str,
    session_id: str | None,
    facts: list[dict[str, str]],
) -> int:
    """Upsert facts into agent_memory table.

    Uses INSERT ... ON CONFLICT to update existing facts by key.
    """
    pool = await get_pool()
    stored = 0

    async with pool.acquire() as conn:
        for fact in facts:
            try:
                await conn.execute(
                    """
                    INSERT INTO agent_memory (id, org_id, session_id, key, content)
                    VALUES (gen_random_uuid()::text, $1, COALESCE($2, ''), $3, $4)
                    ON CONFLICT (org_id, COALESCE(session_id, ''), key)
                    DO UPDATE SET content = EXCLUDED.content,
                                  updated_at = NOW()
                    """,
                    org_id,
                    session_id,
                    fact["key"],
                    fact["content"],
                )
                stored += 1
            except Exception as exc:
                logger.warning(
                    "fact_upsert_failed",
                    extra={"key": fact["key"], "error": str(exc)},
                )

    return stored
