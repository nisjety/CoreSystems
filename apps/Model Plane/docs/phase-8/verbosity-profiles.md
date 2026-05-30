# Verbosity profiles

## Purpose

Operator-facing knobs that control log, trace, and diagnostic density without altering runtime behavior or policy outcomes.

## Profiles

| Profile | Logs | Traces | Metrics | Use case |
| --- | --- | --- | --- | --- |
| `silent` | error+ | off | counters only | prod steady-state, cost-sensitive |
| `standard` | info+ | sampled 1% | full | default prod |
| `debug` | debug+ | sampled 100% | full + histograms | incident response |
| `trace` | trace+ | 100% + payloads | full + per-request | short-lived forensic window |

## Configuration

- Environment variable: `CAPABILITY_CORE_VERBOSITY` (default `standard`).
- Runtime override: operator-scoped HTTP GET endpoint (see Phase 4 control surface).
- Per-request override via header: `X-Verbosity: debug` (allowed only for authenticated operators).

## Invariants

- Verbosity NEVER gates correctness. A silent-mode request and a trace-mode request produce identical business outcomes.
- Verbosity NEVER hides error states. `silent` still surfaces errors at `error` level.
- Idempotency keys, request IDs, and policy decisions are logged at all levels.
- Payloads are redacted per the existing redaction layer at every level.

## Boundaries

- Profile changes take effect within one request cycle (no restart).
- Runtime override requires operator auth; request-scoped override is bounded to that request.
- `trace` profile is rate-limited and has a hard max-duration (default 15 min) to prevent accidental PII capture.

## Benchmarks

Per profile, publish:

- CPU overhead (% vs `silent`).
- Log volume (bytes/sec at 100 RPS reference workload).
- Trace export cost.
