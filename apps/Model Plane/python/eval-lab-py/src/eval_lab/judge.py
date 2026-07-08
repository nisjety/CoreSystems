"""LLM judge: scores an answer against a rubric via the live stack itself.

Judging goes through model-gateway /v1/invoke (velion-balance) with a
structured-output schema, so the judge rides the same governed, priced,
observable path as everything else — no side-channel vendor calls.
"""

from __future__ import annotations

import json

from eval_lab.client import VelionClient

_JUDGE_SCHEMA = json.dumps(
    {
        "type": "object",
        "properties": {
            "score": {"type": "number", "minimum": 0, "maximum": 5},
            "rationale": {"type": "string"},
        },
        "required": ["score", "rationale"],
    }
)


def make_judge(client: VelionClient, token: str):
    """Build a JudgeFn bound to a client + token. Returns
    (score_0_to_5, rationale); raises on transport failure so callers can
    decide to skip-with-reason."""

    def judge(answer: str, rubric: str) -> tuple[float, str]:
        prompt = (
            "You are a strict evaluation judge. Score the ANSWER against the "
            "RUBRIC. Respond ONLY with JSON {\"score\": <0-5>, \"rationale\": <short>}.\n\n"
            f"RUBRIC:\n{rubric}\n\nANSWER:\n{answer}"
        )
        response = client.invoke_sync(
            token, prompt, model="velion-balance", structured_output_schema=_JUDGE_SCHEMA
        )
        content = str(response.get("content", ""))
        try:
            parsed = json.loads(content)
            score = float(parsed.get("score", 0.0))
            rationale = str(parsed.get("rationale", ""))[:300]
        except (ValueError, TypeError):
            # A judge that cannot produce parseable output scores 0 loudly
            # rather than silently passing.
            return 0.0, f"judge output unparseable: {content[:120]!r}"
        return max(0.0, min(score, 5.0)), rationale

    return judge
