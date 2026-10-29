# ADR: the agent-loop and tool-execution boundary stays split

**Status:** accepted — 2026-08-14
**Decision owner:** Model Plane (execution-core owns governed execution;
model-gateway owns the inline chat surface).

## Context

`apps/QM_INSPIRED_IMPROVEMENT_PLAN_2026-08-13.md` compared CoreSystem against
QM (`yc-software/qm`), whose agent surface is one `Harness` interface with five
backends sharing a single tool-definition factory (`createPiTools`). Against
that, CoreSystem looked like unmanaged duplication, and the plan recommended:

* **HARN-1** — collapse `execution-core::execute_step_inner` and
  `model-gateway::dispatch_tool` into one crate owning "naming, schemas,
  capability gating, hooks, permission checks, and result formatting."
* **HARN-2** — define one `run_turn(ctx, goal, tools, budget) -> TurnOutcome`
  trait and refactor the loops into named strategies behind it.

Both were rated High impact. Neither had been started, on the stated grounds
that the refactor needed a quiet tree. When the tree became quiet, the premise
was measured before the code was moved. It did not hold.

## What measurement showed

**The two dispatchers share no tools.** `dispatch_tool` has 18 arms, all reads:
`web_search`, `fetch_url`, `knowledge_search`, `recall_memory`,
`social_list_*`, `insights_overview`, `brreg_lookup_organization`,
`shipping_get_quotes`, and so on. `execute_step_inner` has none of them. The
intersection is empty.

They are not two implementations of one thing. `execute_step_inner` is a
**policy pipeline** — capability evaluation with Ed25519 evidence, then hook
evaluation, then permission evaluation — wrapped around a small set of
side-effecting primitives (sandboxed shell, code interpreter, MCP-prefixed
tools, subagent dispatch). `dispatch_tool` is a **read-tool router** for inline
chat, and its first act is to refuse anything side-effecting:
`inline_tool_allowed` rejects with "side-effecting tools require governed
agentic execution and approval."

That refusal *is* the boundary. Merging the two modules would put the governed
path and the deliberately-ungoverned path behind one entry point, and the thing
keeping them apart today is that they are not reachable from each other.

**The two loops differ where the trait would bind them.** `run_rounds`
constructs its own history from a goal plus `AGENT_PREAMBLE` — isolation is the
point, because a subagent's transcript must never reach the parent's context,
only its conclusion. `run_tool_rounds` receives `base_messages` from its caller,
because it continues an existing chat thread, and it carries an SSE sink for
streaming. A `run_turn(ctx, goal, tools, budget)` signature fits the first and
misrepresents the second: inline chat has no single "goal," it has a
conversation.

**The constants the plan read as drift are deliberate and documented.** Both
services define a round budget of 12 with a ceiling of 32 under different names.
execution-core's doc comment names model-gateway's `max_tool_rounds` explicitly
and states the invariant: a deployed agent is the long-horizon surface, so it
must never get less room to work than plain chat. It also records why the
numbers are what they are — they were 4 and 8, below what a self-describing MCP
server needs to discover a schema before acting, and a deployed agent starved on
work chat completed. `MAX_TOOL_CONTEXT_CHARS` likewise says it matches
model-gateway's `MAX_TOOL_OUTPUT_CHARS` so one tool result is not silently
richer on one surface.

## Decision

1. **Do not merge the two dispatchers.** The split follows an authority
   boundary, not an accident of history. Inline chat may run read tools without
   approval precisely because it cannot run anything else; governed execution
   carries capability evidence, hooks, and permission checks precisely because
   it can. One module owning both would make the next accidental cross-call
   compile.

2. **Do not introduce a single turn-loop trait over both loops.** History
   ownership differs by design — one seeds for isolation, one continues a
   thread. A trait that unified them would either force inline chat to
   fabricate a goal or force the agent loop to accept caller-supplied history,
   and the latter is what subagent isolation exists to prevent.

3. **Enforce the documented cross-service relationships with tests, not
   comments.** Three constants claimed a relationship to a sibling service and
   nothing checked it. They are now asserted in
   `execution-core/tests/cross_service_loop_contract.rs`, which reads both
   sources rather than importing either — see that file's header for why a build
   dependency between the two services would be worse coupling than the
   invariant it protects.

4. **Treat genuine duplication where it actually is.** Provenance and screening
   rendering *is* implemented twice (`model-gateway`'s `append_tool_outcomes` /
   `append_provenance_note` and `execution-core`'s `provenance::render`), and
   the two agree today only because they were written from the same spec in the
   same week. That is the extraction worth doing, and it is a formatting
   concern with no authority attached — unlike the dispatchers.

## Consequences

Three hand-written loops remain. That is accepted, with one qualification: they
are three loops because they wrap three different execution authorities, not
because nobody got around to unifying them. A fourth surface (a batch or
offline runner) should ask which authority it runs under and reuse that loop,
rather than copying whichever is nearest — copying is what produced
`deep_research.rs`'s driver, whose own comment says it mirrors
`run_tool_rounds`.

The loops also differ in how far duplicate-call suppression reaches, and this is
worth stating precisely because the difference reads as a gap until you look at
it. `run_tool_rounds` keys a `seen_calls` set on `duplicate_call_signature` for
every tool. `run_rounds` suppresses only exact repeats of *canonical knowledge
retrieval*, via `retrieval_signature`, and its comment says why: agentic
retrieval is expected to reformulate and backtrack across rounds, so a changed
query or `top_k` must stay eligible, and only a byte-identical repeat is
provably unable to add evidence.

Whether the narrower rule should widen is a real question — a deployed agent can
still repeat an identical non-retrieval call until its budget runs out — but it
is a question about one loop's policy, answerable on its own. It is not evidence
that the loops should be one loop; a merged loop would have had to pick one of
the two rules for both surfaces, and neither rule is right for both.

HARN-1 and HARN-2 are **withdrawn** rather than deferred. HARN-3 — this record —
is the part of that recommendation that survived contact with the code.
