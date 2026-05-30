"""Layer 9 — Post-execution content safety.

Checks the LLM output for harmful content before returning to the user.

Detection order:
1. Azure AI Content Safety SDK — when configured.
2. Regex patterns on LLM output — always run as fallback.
"""

from __future__ import annotations

import logging
import re
import time

from app.config import get_settings
from app.domain import PipelineContext, SafetyVerdict

logger = logging.getLogger(__name__)

# Output patterns that should be blocked
_BLOCK_PATTERNS: list[re.Pattern[str]] = [
    re.compile(r"\b(password|secret_key|api_key)\s*[:=]\s*\S+", re.I),
    re.compile(r"\b(DROP\s+TABLE|DELETE\s+FROM|TRUNCATE)\b.*\b(WHERE|CASCADE)?\b", re.I),
]

# Output patterns that get flagged
_FLAG_PATTERNS: list[re.Pattern[str]] = [
    re.compile(r"\b(rm\s+-rf|sudo\s+rm|format\s+c:)", re.I),
]


async def run(ctx: PipelineContext) -> PipelineContext:
    settings = get_settings()

    if not settings.content_safety_enabled:
        ctx.post_safety = SafetyVerdict.SAFE
        return ctx

    raw = ctx._raw
    result = raw.get("result")
    if not result:
        ctx.post_safety = SafetyVerdict.SAFE
        return ctx

    content: str = result.content or ""
    t0 = time.monotonic_ns()

    # ── 1. Azure AI Content Safety SDK ──────────────────────────────────────
    if settings.azure_content_safety_endpoint:
        from app.services.content_safety import get_content_safety_service
        from app.services.safety_telemetry import SafetyEventType, get_safety_telemetry

        sdk_result = await get_content_safety_service().analyze_text(content, ctx.org_id)
        latency_ms = (time.monotonic_ns() - t0) // 1_000_000

        raw["post_safety_detail"] = {
            "categories": [
                {"category": c.category, "severity": c.severity}
                for c in sdk_result.categories
            ],
            "max_severity": sdk_result.max_severity,
            "sdk_error": sdk_result.sdk_error,
        }

        event_type = (
            SafetyEventType.API_ERROR if sdk_result.sdk_error else SafetyEventType.OUTPUT_CHECK
        )
        get_safety_telemetry().record_event(
            org_id=ctx.org_id,
            request_id=ctx.request_id,
            event_type=event_type,
            verdict=sdk_result.verdict,
            detail=raw["post_safety_detail"],
            latency_ms=latency_ms,
        )

        if not sdk_result.sdk_error:
            ctx.post_safety = sdk_result.verdict
            if sdk_result.verdict == SafetyVerdict.BLOCKED:
                logger.warning(
                    "L09_safety_post SDK BLOCKED request_id=%s max_severity=%d",
                    ctx.request_id,
                    sdk_result.max_severity,
                )
            return ctx
        # SDK error — fall through to regex

    # ── 2. Regex fallback ────────────────────────────────────────────────────
    for pattern in _BLOCK_PATTERNS:
        if pattern.search(content):
            ctx.post_safety = SafetyVerdict.BLOCKED
            logger.warning(
                "L09_safety_post BLOCKED (regex) request_id=%s pattern=%s",
                ctx.request_id,
                pattern.pattern[:40],
            )
            return ctx

    for pattern in _FLAG_PATTERNS:
        if pattern.search(content):
            ctx.post_safety = SafetyVerdict.FLAGGED
            logger.info(
                "L09_safety_post FLAGGED (regex) request_id=%s pattern=%s",
                ctx.request_id,
                pattern.pattern[:40],
            )
            return ctx

    ctx.post_safety = SafetyVerdict.SAFE
    logger.debug("L09_safety_post safe request_id=%s", ctx.request_id)
    return ctx
