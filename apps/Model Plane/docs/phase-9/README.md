# Phase 9 — Verification and Cutover

Foundation-only documentation phase. Extends [VERIFICATION.md](../VERIFICATION.md) and [CUTOVER.md](../CUTOVER.md) beyond Phases 0–8 to cover the full planned shell.

<!-- foundation-only: not yet implemented -->

## Scope

Extend verification and cutover docs beyond current foundation gates to cover the orchestration shell, capability platform, multimodal surfaces, memory stack, task/cron/coordinator durability, bridge/voice/channel surfaces, and compact transport correctness.

## Deliverables

- **[VERIFICATION.md](../VERIFICATION.md) — Phase 9 gates** (line 364+): 7 uniform gates, each with Scope / Invariants preserved / Lint evidence / Test evidence / Wire-up / Gaps.
- **[CUTOVER.md](../CUTOVER.md) — Phase 9 Staged Cutover**: scope ladder, 6-step cutover procedure, HTTP + proto migration tables, invariants block.

## Gates

1. Orchestration shell parity
2. Capability platform parity
3. Multimodal breadth
4. Graph/wiki memory correctness
5. Tasks/cron/coordinator durability
6. Bridge/voice/channel surfaces
7. Compact transport correctness

## Invariants (preserved across phases)

- **Canonical IdemPrefix fixture**
  - Formula: `blake3("model-gateway|INGRESS_ACCEPTED|thread/abc|req-1")`
  - Hex: `fbc1d94e94d756ede12c527b3b59e2204f58a623e6bd5a3d679eb03d93f22637`
- Org: `"triodelab"`.
- HTTP GET-only on control surfaces (405 + `Allow: GET` on non-GET).
- Go control plane; Rust hot paths (`unsafe_code = forbid`).
- NATS subjects routed `mp.v1.*` via `go/pkg/natsx/compat.go` + Rust mirror `rust/crates/mp-events/src/subjects.rs`.

## Evidence status

Lint and test evidence for Phase 9 gates is **foundation-only TBD**, pending the Phase 9 implementation PRs that deliver the orchestration shell, capability platform, and remaining surfaces listed in [PLAN.md](../PLAN.md#phase-9--verification-and-cutover).

## Acceptance

`VERIFICATION.md` and `CUTOVER.md` cover the new shell, not just the original migration foundation work — ✅ authored, awaiting implementation to populate evidence rows.
