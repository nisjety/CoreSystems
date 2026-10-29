# 0001 — HARN-1/2: unifying the two tool-dispatch loops (withdrawn)

**Date:** 2026-08-14
**Status:** Withdrawn on measurement, before any code moved
**Source:** `git show ac2be529` (`docs(model-plane): withdraw HARN-1/2 on
measurement, guard what was real`)

## What was proposed

Two related refactors, both rated High impact and deferred pending a quiet
tree:

- **HARN-1** — collapse `execution-core`'s `execute_step_inner` and
  `model-gateway`'s `dispatch_tool` into one module.
- **HARN-2** — unify the two turn loops (`run_rounds` and
  `run_tool_rounds`) behind one `run_turn(ctx, goal, ...)` trait.

## Why it looked right at the time

Two tool-dispatch implementations and two turn loops read, from the outside,
like duplication — the kind of thing a "one canonical owner per capability"
pass should flag and consolidate. The tree went quiet enough to actually
attempt it, so the premise was measured against the real code before any
merge happened.

## What the measurement found

**The two dispatchers share ZERO tools.** `dispatch_tool` is an 18-arm
read-tool router whose first act is to refuse anything side-effecting;
`execute_step_inner` is a capability/hook/permission pipeline around
sandboxed execution. **That refusal boundary IS the authority boundary** —
one module owning both dispatch paths would make the next accidental
cross-call between "read-only chat" and "gated execution" compile silently
instead of failing to build.

**The two loop signatures fit only their own caller.** `run_rounds` seeds
its own fresh message history because subagent isolation depends on
starting clean; `run_tool_rounds` receives the caller's existing thread
because it continues an ongoing conversation. A single `run_turn(ctx, goal,
...)` shape cannot honestly represent both without one of them faking the
other's contract.

**A related concern (cross-service round-budget constants) was real, but
the fix was NOT unification.** `execution-core`'s round-budget constant and
`model-gateway`'s had drifted once already — silently: the shared invariant
("a deployed agent must never get less room to work than plain chat") was
enforced only by a comment naming the other side, and it had already broken
in production (the budget was 4/8, below what a self-describing MCP server
needs to even discover its own tool schema before acting — a deployed agent
starved on real work while plain chat completed fine, with nothing erroring
to surface it).

## Root cause

Treating "these two things look similar" as evidence they should become one
thing, without first checking whether the similarity is structural (a real
duplicate capability) or coincidental (two different capabilities that
happen to rhyme in shape). The actual duplication here was in three
loosely-coupled constants, not in the two dispatch paths or the two loops.

## Guardrail landed instead

`cross_service_loop_contract.rs` — the three previously comment-only
cross-service invariants are now asserted at runtime, verified by
perturbing both sides and confirming the assertions fail with legible
messages (not merely that they pass when untouched). It reads both sources
rather than importing either: a build dependency between two independently
deployed services would be heavier coupling than the invariant it protects,
and widening the constants to `pub` would export loop internals as API.

**What survived from the original proposal:** HARN-3, the ADR documenting
this reasoning, plus a correction to a claim drafted along the way —
`run_rounds` does have duplicate-call suppression (scoped to canonical
knowledge retrieval on purpose), it was never absent.

## The guardrail this leaves behind

Before proposing to merge two things because they look alike: check what
they actually share (tools, callers, contracts), not just their names or
their position in the architecture diagram. If the real problem is a
cross-service invariant enforced only by comment, fix THAT — with a runtime
assertion that can fail loudly — rather than reaching for a structural
merge the invariant doesn't actually require.

See also: `apps/Model%20Plane/CLAUDE.md`'s "Two tool-dispatch loops exist on
purpose and must **stay separate**" rule, which exists because of this.
