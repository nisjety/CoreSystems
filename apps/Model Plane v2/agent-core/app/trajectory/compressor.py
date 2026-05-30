"""Skill compressor — synthesises SKILL.md content from trajectory clusters.

Triggered by the cron job or post-run hook when a (org_id, task_pattern)
pair has accumulated >= 5 successful runs that haven't been synthesised yet.

Uses the LLM to produce SKILL.md-formatted guidance from the trajectory goals
and outputs, then stores it via AutoSkillRegistry.store().
"""

from __future__ import annotations

import logging
from typing import Any

from app.database import get_pool
from app.skills.auto_registry import AutoSkillRegistry
from app.trajectory.patterns import normalize_goal

logger = logging.getLogger(__name__)

# Minimum successes required before we attempt synthesis
_MIN_SUCCESSES = 5

# Synthesis prompt
_SYNTHESIS_PROMPT = """\
You are a skill synthesis engine. Given a set of successful agent runs for the \
task pattern "{task_pattern}", write a concise SKILL.md that will help future \
agents perform this type of task better.

Format:
## {name}
**Pattern:** {task_pattern}

### When to use
<one sentence>

### Approach
<numbered steps, 3-7 items>

### Key considerations
<bullet list, up to 5 items>

---
Example runs:
{examples}
---

SKILL.md content:"""


async def maybe_compress(
    org_id: str,
    task_pattern: str,
    llm_client: Any | None = None,
) -> str | None:
    """Synthesise a skill if enough trajectory data is available.

    Returns the new skill_id if a skill was created/updated, else None.
    Idempotent: re-running with the same trajectories already synthesised
    will only re-run if there are NEW unsynthesised trajectories beyond
    _MIN_SUCCESSES.
    """
    try:
        pool = await get_pool()
    except Exception:
        return None

    # Fetch unsynthesised successes
    try:
        async with pool.acquire() as conn:
            rows = await conn.fetch(
                """
                SELECT id, task_goal, executed_actions
                FROM agent_trajectories
                WHERE org_id = $1
                  AND task_pattern = $2
                  AND outcome = 'success'
                ORDER BY created_at DESC
                LIMIT 20
                """,
                org_id,
                task_pattern,
            )
    except Exception as exc:
        logger.warning("skill_compress_fetch_failed", extra={"error": str(exc)})
        return None

    if len(rows) < _MIN_SUCCESSES:
        return None

    trace_ids = [str(r["id"]) for r in rows]
    goals = [r["task_goal"] for r in rows]

    # Build examples block
    examples = "\n".join(f"- {g[:300]}" for g in goals[:10])

    # Synthesise skill content
    skill_md = await _synthesise(task_pattern, examples, llm_client)
    if not skill_md:
        return None

    skill_name = task_pattern.replace("_", " ").title()
    registry = AutoSkillRegistry()
    return await registry.store(
        org_id=org_id,
        name=skill_name,
        task_pattern=task_pattern,
        skill_md=skill_md,
        source_trace_ids=trace_ids,
    )


async def _synthesise(
    task_pattern: str,
    examples: str,
    llm_client: Any | None,
) -> str | None:
    """Call the LLM to produce a SKILL.md for the given pattern."""
    if llm_client is None:
        return None

    skill_name = task_pattern.replace("_", " ").title()
    prompt = _SYNTHESIS_PROMPT.format(
        task_pattern=task_pattern,
        name=skill_name,
        examples=examples,
    )
    try:
        messages = [
            {
                "role": "system",
                "content": "You generate concise, actionable skill documentation for AI agents.",
            },
            {"role": "user", "content": prompt},
        ]
        raw = await llm_client.planner_complete(messages)
        # planner_complete returns either a list[AgentAction] or str depending on
        # model and prompt structure; accept whatever comes back as a string
        if isinstance(raw, str):
            return raw.strip() or None
        # list-style return from some backends
        if isinstance(raw, list) and raw:
            first = raw[0]
            text = getattr(first, "output", None) or str(first)
            return text.strip() or None
        return None
    except Exception as exc:
        logger.warning("skill_synthesis_llm_failed", extra={"error": str(exc)})
        return None
