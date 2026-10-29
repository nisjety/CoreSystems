# Phase 8 — Token efficiency and ergonomics

## Scope

Improve operator and runtime token efficiency without hiding system state. Every feature in this phase is **optional, benchmarked, and reversible**.

## Deliverables

1. [Compact transport](./compact-transport.md) — optional TOON-style encoding for large structured outputs.
2. [Compact summaries](./compact-summaries.md) — optional caveman-style condensed memory and context summaries.
3. [Verbosity profiles](./verbosity-profiles.md) — operator-facing knobs controlling log and trace density.
4. [Response-style profiles](./response-style-profiles.md) — presentation-layer styles decoupled from runtime logic.

## Reference inputs

- `TOON` — token-oriented object notation for compact structured payloads.
- `caveman` — ultra-compressed summary style preserving technical substance.

## Acceptance

- Compact formats are documented as **optional** (off by default).
- Compact formats are **benchmarked** against JSON/verbose baselines (token count, parse cost, fidelity).
- Compact formats are **reversible** — every compact payload has a documented decoder returning canonical form.
- No compact mode hides error state, identity, or idempotency keys.

## Non-goals

- Replacing the canonical JSON wire format on any public contract.
- Lossy summaries on anything source-bearing (see Phase 7 contradiction rules).
- Style changes that alter runtime behavior or policy decisions.

## Boundaries

- Transport-level compaction lives at the serializer boundary; handlers stay canonical.
- Summary compaction lives in the memory/context layer; underlying records stay verbatim.
- Verbosity and style profiles are configuration; they never gate correctness.
