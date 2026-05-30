# ADR-003: Run State Machine

## Metadata

- **Status:** Accepted
- **Date:** 2025-01-XX
- **Deciders:** architecture working group
- **Supersedes:** none
- **Superseded by:** none
- **Related:** ADR-001 (Service Ownership Matrix), ADR-002 (Event Envelope)

---

## Context

A `run` is the unit of agent execution attached to a `thread`. Today, run
lifecycle is inconsistent across services:

- ai-core, agent-core, and execution-core each mutate `runs.state` directly
  with divergent string values (`"in_progress"`, `"running"`, `"active"`).
- There is no single-writer invariant, so concurrent writers race and
  overwrite terminal states.
- Replay and resume semantics are broken: a crashed executor cannot safely
  re-enter a run because no service agrees on what "resumable" means.
- Human-in-the-loop approvals and async tool callbacks have no formal
  `waiting_*` states, so the executor polls or blocks.
- Context compaction happens in-band during `running`, hiding long pauses
  from observability.

Without a canonical FSM, we cannot build a scheduler, a reliable retry
policy, or an audit trail. This is a Phase 0 HIGH blocker (GAP-003).

---

## Decision

Runs follow the canonical 9-state finite state machine defined in
[`docs/run-state-machine.md`](../run-state-machine.md), which is normative.

**States:** `created`, `queued`, `running`, `waiting_for_approval`,
`waiting_for_tool`, `compacting`, `completed`, `failed`, `cancelled`.

**Terminal states** (`completed`, `failed`, `cancelled`) are immutable.

### Boundary Rules

1. **Single writer.** `agent-core` is the sole service permitted to `UPDATE
   runs SET state`. All other services (ai-core, execution-core, capability
   workers) propose transitions by publishing
   `run.transition.requested` on NATS with an ADR-002 envelope.
2. **Transition table is closed.** Only transitions listed in
   `run-state-machine.md §2` are allowed. Any other request is rejected
   with HTTP 409 and emits `run.transition.rejected`.
3. **Idempotent transitions.** Every transition carries
   `envelope.idempotency_key`. agent-core deduplicates by
   `(run_id, idempotency_key)` so retries are safe.
4. **Terminal immutability.** Once a run enters `completed`, `failed`, or
   `cancelled`, further transition requests are rejected. No resurrection.
5. **Event emission is mandatory.** Every accepted transition emits the
   event listed in the transition table with a full ADR-002 envelope.
   No state change is silent.
6. **Cancel is always legal from non-terminal states** and short-circuits
   pending approvals and tool callbacks.

---

## Consequences

### Positive

- Deterministic replay and resume: a crashed executor can reconstruct run
  state from the event log alone.
- Auditability: every transition has an envelope with `actor`, `trace_id`,
  and `occurred_at`.
- Tractable testing: 9 states + a closed transition table is a finite,
  property-testable system.
- Unblocks Phase 1 scheduler work (which assumes a canonical FSM).
- Human approval and async tool flows have first-class states instead of
  ad-hoc booleans.

### Negative

- Refactor cost across ai-core, agent-core, and execution-core to remove
  direct writes to `runs.state`.
- Added latency from NATS round-trip for cross-service transition
  requests (bounded, typically <5ms intra-cluster).
- Training cost: contributors must learn the transition table before
  editing run-handling code.

### Neutral

- Introduces `waiting_for_approval`, `waiting_for_tool`, `compacting` as
  explicit states — visible in dashboards and metrics.

---

## Alternatives Considered

1. **Per-service state machines.** Each service maintains its own view of
   run state, reconciled eventually. Rejected: current broken behaviour,
   no source of truth, impossible to audit.
2. **CRDT-based state.** Use a last-writer-wins or OR-set CRDT over
   `runs.state`. Rejected: overkill for an inherently sequential lifecycle,
   and terminal immutability is hard to express.
3. **Status strings without an FSM.** Keep free-form status strings with
   documentation-only constraints. Rejected: this is the current state and
   it is the problem.
4. **BPMN / workflow engine (Temporal, Cadence).** Rejected for Phase 0 as
   too heavy; may revisit in Phase 6+ for long-running workflows.

---

## Compliance

- CI lint rejects any SQL matching `UPDATE\s+runs\s+SET\s+state` outside
  `agent-core/`.
- Postgres `runs.state` column uses a `CHECK` constraint enumerating the
  9 canonical values.
- Postgres trigger rejects any update that transitions from a terminal
  state.
- Services document their transition-request events in their README under
  an `## Emitted events` section (per ADR-002 §Compliance).
- Property tests in `agent-core/tests/state_machine_test.go` enumerate
  every cell of the transition table and assert acceptance or rejection.

---

## References

- [`docs/run-state-machine.md`](../run-state-machine.md) — normative FSM spec
- [`docs/event-envelope.md`](../event-envelope.md) — envelope carried on transition events
- [`docs/identifiers.md`](../identifiers.md) — `run_id`, `thread_id`, `approval_id`, `tool_call_id`
- ADR-001 — service ownership; establishes agent-core as run lifecycle owner
- ADR-002 — event envelope; required on every emitted transition event
