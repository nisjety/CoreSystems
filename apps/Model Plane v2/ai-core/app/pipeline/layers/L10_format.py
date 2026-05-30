"""Layer 10 — Output formatting.

Applies any format instructions, trims output, and finalises the response.
"""

from __future__ import annotations

import logging

from app.domain import PipelineContext

logger = logging.getLogger(__name__)


async def run(ctx: PipelineContext) -> PipelineContext:
    raw = ctx._raw
    result = raw.get("result")
    if not result:
        return ctx

    content: str = result.content or ""

    # Apply format instructions (simple post-processing)
    fmt = ctx.format_instructions.lower()

    if fmt == "json":
        # Strip markdown code fences if the model wrapped JSON in them
        content = _strip_code_fences(content, "json")
    elif fmt == "markdown":
        pass  # Already markdown — no-op
    elif fmt == "plain":
        # Strip all markdown formatting
        content = _strip_markdown(content)

    # Trim trailing whitespace
    content = content.rstrip()

    # Write back the (potentially modified) content
    result.content = content
    raw["result"] = result

    logger.debug("L10_format format=%s len=%d request_id=%s", fmt or "default", len(content), ctx.request_id)
    return ctx


def _strip_code_fences(text: str, lang: str = "") -> str:
    """Remove ```lang ... ``` wrappers."""
    lines = text.strip().splitlines()
    if not lines:
        return text
    first = lines[0].strip()
    if first.startswith("```"):
        lines = lines[1:]
    if lines and lines[-1].strip() == "```":
        lines = lines[:-1]
    return "\n".join(lines)


def _strip_markdown(text: str) -> str:
    """Minimal markdown stripping (headers, bold, italic, links)."""
    import re

    text = re.sub(r"^#{1,6}\s+", "", text, flags=re.M)  # headers
    text = re.sub(r"\*\*(.+?)\*\*", r"\1", text)  # bold
    text = re.sub(r"\*(.+?)\*", r"\1", text)  # italic
    text = re.sub(r"\[(.+?)\]\(.+?\)", r"\1", text)  # links
    text = re.sub(r"`(.+?)`", r"\1", text)  # inline code
    return text
