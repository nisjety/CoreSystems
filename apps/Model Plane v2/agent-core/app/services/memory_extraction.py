"""Memory extraction service — auto-extract facts from conversation turns.

At the end of each turn the LLM output is scanned for:
  [MEMORY: <fact>] annotations placed by the agent or user.
  Natural-language sentences that look like stable facts worth persisting.

Extracted facts are forwarded to the context/memory subsystem for storage.

Design goals:
- Fast path: regex scan costs ~µs per turn
- Slow path: LLM extraction via gpt-4o-mini, only when enabled
- Fail-open: if extraction fails, nothing breaks upstream
"""

from __future__ import annotations

import logging
import re
from typing import Any

logger = logging.getLogger(__name__)

# ---------------------------------------------------------------------------
# Constants
# ---------------------------------------------------------------------------

_MEMORY_TAG_RE = re.compile(
    r"\[MEMORY:\s*(.+?)\]",
    re.IGNORECASE | re.DOTALL,
)

_LLM_EXTRACTION_PROMPT = """\
You are a memory extraction assistant.
Given a conversation turn, identify facts that are worth remembering long-term:
- Preferences ("the user prefers…")
- Project decisions ("we decided to use…")
- Entity relationships ("X is the owner of Y")
- Constraints ("always use Python 3.12")

Return a JSON array of short factual strings. Return [] if nothing is worth storing.
Only return the JSON array — no other text.

Turn:
{turn_text}
"""

# ---------------------------------------------------------------------------
# Fast-path extraction — regex scan
# ---------------------------------------------------------------------------


def extract_tagged_memories(text: str) -> list[str]:
    """Find all [MEMORY: <fact>] annotations in *text*.

    Returns a list of de-duplicated, stripped fact strings.
    """
    matches = _MEMORY_TAG_RE.findall(text)
    seen: set[str] = set()
    facts: list[str] = []
    for raw in matches:
        fact = raw.strip()
        if fact and fact not in seen:
            seen.add(fact)
            facts.append(fact)
    return facts


# ---------------------------------------------------------------------------
# Slow-path extraction — LLM
# ---------------------------------------------------------------------------


async def extract_memories_llm(
    turn_text: str,
    *,
    model: str = "gpt-4o-mini",
    openai_api_key: str | None = None,
) -> list[str]:
    """Use an LLM to extract memorable facts from *turn_text*.

    Returns an empty list on any failure (fail-open).
    """
    if not turn_text.strip():
        return []

    try:
        import json
        import openai

        client = openai.AsyncOpenAI(api_key=openai_api_key)
        resp = await client.chat.completions.create(
            model=model,
            messages=[
                {
                    "role": "user",
                    "content": _LLM_EXTRACTION_PROMPT.format(
                        turn_text=turn_text[:4000]  # cap context for cost
                    ),
                }
            ],
            max_tokens=512,
            temperature=0.0,
        )
        raw = resp.choices[0].message.content or "[]"
        facts = json.loads(raw)
        if isinstance(facts, list):
            return [str(f).strip() for f in facts if f]
        return []
    except Exception:
        logger.debug("memory_extraction_llm_failed", exc_info=True)
        return []


# ---------------------------------------------------------------------------
# Combined extractor
# ---------------------------------------------------------------------------


async def extract_memories(
    turn_text: str,
    *,
    llm_enabled: bool = False,
    model: str = "gpt-4o-mini",
    openai_api_key: str | None = None,
) -> list[str]:
    """Extract memorable facts from *turn_text*.

    Always runs the fast regex path.
    Optionally runs the LLM path if ``llm_enabled=True``.
    Deduplicates across both paths.
    """
    tagged = extract_tagged_memories(turn_text)
    llm_facts: list[str] = []

    if llm_enabled:
        llm_facts = await extract_memories_llm(
            turn_text, model=model, openai_api_key=openai_api_key
        )

    seen: set[str] = set()
    merged: list[str] = []
    for fact in tagged + llm_facts:
        if fact and fact not in seen:
            seen.add(fact)
            merged.append(fact)

    if merged:
        logger.info(
            "memories_extracted",
            extra={"count": len(merged), "llm_used": llm_enabled},
        )
    return merged


# ---------------------------------------------------------------------------
# Storage stub — forwards to context subsystem
# ---------------------------------------------------------------------------


async def store_memories(
    facts: list[str],
    *,
    session_id: str,
    run_id: str,
    context_manager: Any | None = None,
) -> None:
    """Persist extracted facts via the context manager (if available).

    Falls back to a structured log entry when no context manager is wired.
    """
    if not facts:
        return

    if context_manager is not None:
        try:
            for fact in facts:
                await context_manager.add_memory(
                    text=fact,
                    session_id=session_id,
                    run_id=run_id,
                    source="auto_extraction",
                )
            return
        except Exception:
            logger.debug("memory_store_failed", exc_info=True)

    # Fallback: structured log
    logger.info(
        "memory_facts_extracted",
        extra={
            "session_id": session_id,
            "run_id": run_id,
            "facts": facts,
        },
    )
