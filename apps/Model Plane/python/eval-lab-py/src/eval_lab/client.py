"""Live-stack client: auth-core token minting + model-gateway invoke driving.

The harness talks to the REAL stack the same way production does:

- Tokens come from auth-core's service-to-service issuance endpoint
  (POST /api/model-plane/internal-token, X-Internal-Api-Key gated) — RS256
  JWTs with aud=model-gateway that the gateway's require_auth verifies. This
  keeps eval traffic on the production trust chain (no dev bypass) and lets
  cases run as DIFFERENT orgs, which the isolation probe needs.
- Runs drive POST /v1/invoke/stream (SSE) with opt-in event families
  (usage → cost_usd per Phase 7 B5; tools → tool_call/tool_result;
  citations → grounding). The parser captures everything into InvokeOutcome.
"""

from __future__ import annotations

import json
import os
import time
from dataclasses import dataclass, field

import httpx

from eval_lab.types import CaseSpec, InvokeOutcome, StreamEvent

DEFAULT_AUTH_CORE_URL = "http://localhost:3011"
DEFAULT_MODEL_GATEWAY_URL = "http://localhost:8080"

# Event names whose presence marks a HITL approval pause. The gateway emits
# run-scoped orchestration events on the invoke stream for agentic profiles;
# match permissively on the "paused"/"approval" family so a rename in one
# emitter does not silently blind the metric.
_PAUSE_MARKERS = ("paused", "approval")


@dataclass
class EvalEnv:
    """Where the live stack lives + the internal key for token minting."""

    auth_core_url: str = field(
        default_factory=lambda: os.environ.get(
            "EVAL_AUTH_CORE_URL", DEFAULT_AUTH_CORE_URL
        )
    )
    model_gateway_url: str = field(
        default_factory=lambda: os.environ.get(
            "EVAL_MODEL_GATEWAY_URL", DEFAULT_MODEL_GATEWAY_URL
        )
    )
    internal_api_key: str = field(
        default_factory=lambda: os.environ.get("INTERNAL_API_KEY", "")
    )

    def ready(self) -> tuple[bool, str]:
        if not self.internal_api_key:
            return False, "INTERNAL_API_KEY is not set"
        return True, ""


class VerevonClient:
    """Minimal synchronous client for one eval run."""

    def __init__(self, env: EvalEnv | None = None) -> None:
        self.env = env or EvalEnv()
        self._http = httpx.Client(timeout=30.0)
        self._token_cache: dict[tuple[str, str], str] = {}

    def close(self) -> None:
        self._http.close()

    # ── auth ────────────────────────────────────────────────────────────────

    def mint_token(self, org_id: str, user_id: str) -> str:
        """Mint a real model-gateway JWT for (org, user) via auth-core."""
        cache_key = (org_id, user_id)
        if cache_key in self._token_cache:
            return self._token_cache[cache_key]
        response = self._http.post(
            f"{self.env.auth_core_url}/api/model-plane/internal-token",
            headers={"X-Internal-Api-Key": self.env.internal_api_key},
            json={"orgId": org_id, "userId": user_id, "email": f"{user_id}@eval.verevon.dev"},
        )
        response.raise_for_status()
        body = response.json()
        token = body.get("token") or body.get("data", {}).get("token", "")
        if not token:
            raise RuntimeError(f"auth-core returned no token: {body}")
        self._token_cache[cache_key] = token
        return token

    # ── invoke ──────────────────────────────────────────────────────────────

    def invoke_stream(
        self, token: str, case: CaseSpec, *, idempotency_key: str | None = None
    ) -> InvokeOutcome:
        """Drive one case against /v1/invoke/stream and capture the outcome."""
        request: dict[str, object] = {
            "content": case.prompt,
            "profile": case.profile,
            "features": case.features,
            "zdr": case.zdr,
        }
        if case.model:
            request["model"] = case.model
        if case.max_cost_usd is not None:
            request["max_cost_usd"] = case.max_cost_usd
        if idempotency_key:
            request["idempotency_key"] = idempotency_key

        started = time.perf_counter()
        try:
            events = self._collect_sse(token, request, timeout_s=case.timeout_s)
        except httpx.HTTPError as error:
            return InvokeOutcome(transport_error=f"{type(error).__name__}: {error}")
        latency_ms = (time.perf_counter() - started) * 1000.0
        return _assemble_outcome(events, fallback_latency_ms=latency_ms)

    def invoke_sync(
        self,
        token: str,
        content: str,
        *,
        model: str = "verevon-balance",
        structured_output_schema: str | None = None,
        timeout_s: float = 60.0,
    ) -> dict[str, object]:
        """Plain (non-stream) invoke — used by the LLM judge."""
        request: dict[str, object] = {"content": content, "model": model}
        if structured_output_schema:
            request["structured_output_schema"] = structured_output_schema
        response = self._http.post(
            f"{self.env.model_gateway_url}/v1/invoke",
            headers={"Authorization": f"Bearer {token}"},
            json=request,
            timeout=timeout_s,
        )
        response.raise_for_status()
        return response.json()

    def _collect_sse(
        self, token: str, request: dict[str, object], *, timeout_s: float
    ) -> list[StreamEvent]:
        events: list[StreamEvent] = []
        with self._http.stream(
            "POST",
            f"{self.env.model_gateway_url}/v1/invoke/stream",
            headers={
                "Authorization": f"Bearer {token}",
                "Accept": "text/event-stream",
            },
            json=request,
            timeout=httpx.Timeout(timeout_s, connect=10.0),
        ) as response:
            response.raise_for_status()
            for event in parse_sse_lines(response.iter_lines()):
                events.append(event)
                if event.event in ("done", "error", "stopped"):
                    break
        return events


def parse_sse_lines(lines: object) -> list[StreamEvent]:
    """Parse an SSE line iterator into StreamEvents.

    Handles both named events (`event: x` + `data: {...}`) and data-only
    frames whose JSON carries a `type` field. Non-JSON data is preserved
    under {"raw": ...} so nothing is silently dropped.
    """
    events: list[StreamEvent] = []
    current_event = ""
    data_lines: list[str] = []

    def flush() -> None:
        nonlocal current_event, data_lines
        if not data_lines and not current_event:
            return
        raw = "\n".join(data_lines)
        data: dict[str, object]
        try:
            parsed = json.loads(raw) if raw else {}
            data = parsed if isinstance(parsed, dict) else {"raw": parsed}
        except ValueError:
            data = {"raw": raw}
        name = current_event or str(data.get("type", "")) or "message"
        events.append(StreamEvent(event=name, data=data))
        current_event = ""
        data_lines = []

    for line in lines:  # type: ignore[attr-defined]
        if isinstance(line, bytes):
            line = line.decode("utf-8", errors="replace")
        if line == "":
            flush()
            continue
        if line.startswith(":"):
            continue  # SSE comment / keepalive
        if line.startswith("event:"):
            current_event = line[len("event:") :].strip()
        elif line.startswith("data:"):
            data_lines.append(line[len("data:") :].strip())
    flush()
    return events


def _assemble_outcome(
    events: list[StreamEvent], *, fallback_latency_ms: float
) -> InvokeOutcome:
    """Fold the raw event list into the InvokeOutcome the metrics score."""
    text_parts: list[str] = []
    request_id: str | None = None
    run_id: str | None = None
    model_used: str | None = None
    input_tokens: int | None = None
    output_tokens: int | None = None
    cost_usd: float | None = None
    latency_ms: float | None = None
    paused = False
    tool_calls: list[str] = []
    tool_errors = 0
    error_events = 0
    citations: list[dict[str, object]] = []

    for event in events:
        data = event.data
        name = event.event
        if request_id is None:
            rid = data.get("request_id") or data.get("requestId")
            if isinstance(rid, str) and rid:
                request_id = rid
        if run_id is None:
            rid = data.get("run_id") or data.get("runId")
            if isinstance(rid, str) and rid:
                run_id = rid

        if name == "chunk":
            delta = data.get("delta") or data.get("text") or ""
            if isinstance(delta, str):
                text_parts.append(delta)
        elif name == "usage":
            cost = data.get("cost_usd")
            if isinstance(cost, (int, float)):
                cost_usd = float(cost)
            for key, sink in (("input_tokens", "in"), ("output_tokens", "out")):
                value = data.get(key)
                if isinstance(value, int):
                    if sink == "in":
                        input_tokens = value
                    else:
                        output_tokens = value
            lat = data.get("latency_ms")
            if isinstance(lat, (int, float)):
                latency_ms = float(lat)
        elif name == "tool_call":
            tool = data.get("name") or data.get("tool") or ""
            if isinstance(tool, str) and tool:
                tool_calls.append(tool)
        elif name == "tool_result":
            status = str(data.get("status", ""))
            if data.get("error") or status == "error":
                tool_errors += 1
        elif name in ("citation", "grounding"):
            citations.append(dict(data))
        elif name == "error":
            error_events += 1
        elif name == "done":
            model = data.get("model_used") or data.get("model")
            if isinstance(model, str) and model:
                model_used = model
            final = data.get("content")
            if isinstance(final, str) and final and not text_parts:
                text_parts.append(final)

        lowered = name.lower()
        if any(marker in lowered for marker in _PAUSE_MARKERS):
            paused = True
        else:
            status = str(data.get("status", "")).lower()
            if any(marker in status for marker in _PAUSE_MARKERS):
                paused = True

    return InvokeOutcome(
        text="".join(text_parts),
        events=events,
        request_id=request_id,
        run_id=run_id,
        model_used=model_used,
        input_tokens=input_tokens,
        output_tokens=output_tokens,
        cost_usd=cost_usd,
        latency_ms=latency_ms if latency_ms is not None else fallback_latency_ms,
        paused_for_approval=paused,
        tool_calls=tool_calls,
        tool_errors=tool_errors,
        error_events=error_events,
        citations=citations,
    )
