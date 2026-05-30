# CLI / TUI / Web Shell

## Product role

The primary interactive surfaces for operators and developers. All three shells
share a single command vocabulary and session model; the difference is render
target (terminal stream, ink-style TUI, or browser DOM).

## Transport

- **Wire**: HTTPS + WebSocket for streaming deltas.
- **Ingress**: `model-gateway` terminates client TLS and auth.
- **Event stream**: server → client over WS; client → server over HTTPS POST.
- **Auth**: bearer token (per-user) or mTLS (per-host); scoped by org.

## Backend ownership

| Concern | Owner |
|---------|-------|
| Request ingress, rate limit, tenant routing | `model-gateway` |
| Provider selection, streaming tokens | `inference-core` |
| Multi-step orchestration, tool calls | `execution-core` |
| Model catalog, policy, quotas, metadata | `capability-core` |

## `capability-core` responsibilities

The shell queries `capability-core` (GET-only HTTP, per Phase 1) for:

- **Catalog**: available models, modalities, version pins.
- **Policy**: org-level allow/deny, redaction rules, safety tier.
- **Scheduling hints**: preferred region, cost class, latency budget.
- **Metadata**: model cards, capability flags, deprecation notices.

The shell never writes through `capability-core`; mutations go through the
appropriate control-plane workflow (Phase 0 Temporal contracts).

## Reference inputs

- `claude-code-fork` — REPL loop, command registry, history model.
- `openai/codex` — CLI UX for long-running agent sessions.

## Out of scope

- Theme / color packages.
- Telemetry UI (lives in the platform observability plane, not here).
