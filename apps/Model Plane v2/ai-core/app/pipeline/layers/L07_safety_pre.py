"""Layer 7 — Pre-execution content safety.

Checks the incoming user message against content safety rules
before it reaches the LLM.

Detection order:
1. Azure AI Content Safety SDK (severity 0-6 per category) — when configured.
2. Regex patterns (prompt injection, jailbreaks) — always run as fallback.
"""

from __future__ import annotations

import logging
import re
import time

from app.config import get_settings
from app.domain import PipelineContext, SafetyVerdict

logger = logging.getLogger(__name__)

# Patterns that always block (OWASP prompt injection, obvious harmful, etc.)
_BLOCK_PATTERNS: list[re.Pattern[str]] = [
    re.compile(r"ignore\s+(all\s+)?previous\s+instructions", re.I),
    re.compile(r"you\s+are\s+now\s+(DAN|jailbr)", re.I),
    re.compile(r"system\s*:\s*", re.I),  # Raw system prompt injection attempt
]

# Patterns that flag but don't block
_FLAG_PATTERNS: list[re.Pattern[str]] = [
    re.compile(r"\b(hack|exploit|bypass|inject)\b.*\b(system|server|database|auth)\b", re.I),
]


async def run(ctx: PipelineContext) -> PipelineContext:
    settings = get_settings()

    if not settings.content_safety_enabled:
        ctx.pre_safety = SafetyVerdict.SAFE
        return ctx

    raw = ctx._raw
    message: str = raw.get("message", "")
    t0 = time.monotonic_ns()

    # ── 1. Azure AI Content Safety SDK ──────────────────────────────────────
    if settings.azure_content_safety_endpoint:
        from app.services.content_safety import get_content_safety_service
        from app.services.safety_telemetry import SafetyEventType, get_safety_telemetry

        sdk_result = await get_content_safety_service().analyze_text(message, ctx.org_id)
        latency_ms = (time.monotonic_ns() - t0) // 1_000_000

        raw["pre_safety_detail"] = {
            "categories": [
                {"category": c.category, "severity": c.severity}
                for c in sdk_result.categories
            ],
            "max_severity": sdk_result.max_severity,
            "sdk_error": sdk_result.sdk_error,
        }

        event_type = (
            SafetyEventType.API_ERROR if sdk_result.sdk_error else SafetyEventType.INPUT_CHECK
        )
        get_safety_telemetry().record_event(
            org_id=ctx.org_id,
            request_id=ctx.request_id,
            event_type=event_type,
            verdict=sdk_result.verdict,
            detail=raw["pre_safety_detail"],
            latency_ms=latency_ms,
        )

        if not sdk_result.sdk_error:
            ctx.pre_safety = sdk_result.verdict
            if sdk_result.verdict == SafetyVerdict.BLOCKED:
                logger.warning(
                    "L07_safety_pre SDK BLOCKED request_id=%s max_severity=%d",
                    ctx.request_id,
                    sdk_result.max_severity,
                )
            return ctx
        # SDK error — fall through to regex

    # ── 2. Regex fallback ────────────────────────────────────────────────────
    for pattern in _BLOCK_PATTERNS:
        if pattern.search(message):
            ctx.pre_safety = SafetyVerdict.BLOCKED
            logger.warning(
                "L07_safety_pre BLOCKED (regex) request_id=%s pattern=%s",
                ctx.request_id,
                pattern.pattern[:40],
            )
            return ctx

    for pattern in _FLAG_PATTERNS:
        if pattern.search(message):
            ctx.pre_safety = SafetyVerdict.FLAGGED
            logger.info(
                "L07_safety_pre FLAGGED (regex) request_id=%s pattern=%s",
                ctx.request_id,
                pattern.pattern[:40],
            )
            return ctx

    ctx.pre_safety = SafetyVerdict.SAFE
    logger.debug("L07_safety_pre safe request_id=%s", ctx.request_id)
    return ctx
