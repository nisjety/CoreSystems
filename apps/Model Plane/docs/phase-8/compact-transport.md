# Compact transport (TOON)

## Purpose

Provide an optional, token-efficient transport encoding for large structured outputs (capability listings, memory dumps, schedule snapshots) without replacing the canonical JSON contract.

## When to use

- Response payloads where `len(body_json) > 8 KiB` and the consumer is an LLM or agent runtime.
- Bulk listing endpoints returning homogeneous rows.
- Diagnostic snapshots streamed to operator tooling.

## When NOT to use

- Any public `/v1/*` contract response in its default form.
- Error envelopes (always canonical JSON).
- Payloads smaller than ~1 KiB (overhead dominates savings).
- Payloads containing opaque binary or signed blobs.

## Negotiation

- Clients opt in via `Accept: application/toon` (or `?format=toon` query param for GET-only endpoints).
- Default content type remains `application/json`.
- Servers MUST honor `Accept: application/json` as the canonical form.
- Unknown format values fall back to JSON; no error.

## Reversibility

- Every TOON response has a documented decoder producing a JSON document equivalent to the canonical form.
- Round-trip fidelity: `json_decode(toon_decode(toon_encode(x))) == x` for all documented schemas.
- Reference decoder lives alongside the serializer in the same package.

## Benchmarks (required before enabling)

Each endpoint that supports TOON publishes:

| Metric | Baseline (JSON) | TOON | Ratio |
| --- | --- | --- | --- |
| Bytes on wire | | | |
| LLM tokens (tiktoken cl100k) | | | |
| Encode latency p50/p99 | | | |
| Decode latency p50/p99 | | | |

Benchmarks live under `services/capability-core/internal/transport/toon/bench_test.go` (to be added when the feature ships).

## Fallback and safety

- Feature-flag: `CAPABILITY_CORE_TOON_ENABLED` (default `false`).
- Any encode error returns canonical JSON with a `Warning: 299 - "toon-fallback"` header.
- No compact mode hides idempotency keys, request IDs, or policy decisions.
