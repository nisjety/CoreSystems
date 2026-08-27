# Harness Adoption — Execution Plan

**Date:** 2026-08-22 · **Completes goal #1:** *harness-capability audit,
feature-parity matrix, and adoption plan.* The audit and matrix live in
`claude-hermes-deepseek.md` (kept current through today, including its
2026-08-22 five-row correction notice); UI-side wiring specifics live in
`model-plane-to-verevon-parity.md`. This file is the plan that finishes the
adoption — every remaining item, sequenced by the standing priority order:

1. Industry-standard gaps every audited harness has — **must implement**
2. Hermes memory parity — **mandatory, no exceptions**
3. Claude Code as the benchmark (**shapes only** — leaked/unlicensed, zero code)
4. pi as the closest-parity inspiration (MIT)
5. DeepSeek's observability/tracking UI (MIT)

---

## Execution status — 2026-08-24

**All 25 items resolved.** 2.4, 3.1, 4.3, 1.1's cold resume, 3.4 and 3.5's
approval control landed 2026-08-24. Two are deliberate non-builds with their
reasoning recorded: **3.3** (the reference harness rejected fork itself, and the
half worth having was already built and tested) and **4.5** (decided as *wait*,
pending a real need).

The plan is complete. What each item's entry now records is not only what shipped
but what the item's original premise got wrong — nine of them turned out to be
built-but-unreachable rather than missing, and three were better not built at
all.

| Workstream | Complete | Remaining |
|---|---|---|
| WS-1 industry-standard gaps | **1.1**, 1.2, 1.3, 1.4 | — |
| WS-2 Hermes memory | 2.1, 2.2, 2.3, **2.4** | — |
| WS-3 Claude Code shapes | **3.1**, 3.2, **3.4**, **3.5** · 3.3 *rejected, see below* | — |
| WS-4 pi parity | 4.1, 4.2, **4.3**, 4.4 | 4.5 |
| WS-5 observability UI | 5.1, 5.2, 5.3, 5.4, 5.5, 5.6, 5.7 | — |

Both items previously recorded as "deliberately not built" — browser screenshots
(5.5) and the `diff` card (5.4) — are now **done**: the screenshot chain existed
in Quarry-v2 all along and I had looked in the wrong plane, and the diff card has
two real producers (MCP tool patches detected from output, and artifact revision
diffs computed from history already held client-side).

**Verification at close:** 2,184 Model Plane Rust tests · 1,104 Quarry-v2 tests
across 31 binaries (was 81 — see below) · 428 Verevon gateway tests · 1,154
frontend tests across 154 files · `tsc -b` clean · frontend build succeeds · Go
(letta-bridge, capability-core) build/vet/tests clean · lint at the pre-existing
baseline (1 error in `tests/e2e/space-shell.spec.ts`, pre-existing).

**A permanently-failing test was hiding most of the Quarry-v2 suite.**
`quarry-browser`'s loopback-egress test failed on every machine and could never
have passed where it lived — it needed the real `PinnedBrowserEgressProxy`, which
is in quarry-runtime, and quarry-runtime depends on quarry-browser, so importing
it back would be circular. Because `cargo test` stops at a failing test binary,
that one failure aborted the run before the rest of the workspace executed: the
suite reported **81 passing**; fixed, it reports **1,104**. The live proof now
lives in `quarry-runtime/tests/browser_egress_boundary.rs` as two tests — a real
Chromium refusing a loopback navigation behind the real pinned proxy, and a real
TCP request through that proxy proving the transport half CDP interception is only
defence in depth for. Both carry positive controls, because "the request log is
empty" is also what a broken witness produces. See the ledger entry for the full
reasoning.

### The recurring finding

**Nine items turned out to be built-but-unreachable rather than missing**, and
in most cases the audit's own description of the gap was wrong in a way that
would have sent the work to the wrong place:

- `reasoning_delta` — a fully-built SPA consumer, a documented event, **zero
  producers** (3.2)
- `save_memory`/`recall_memory` — on the loop, **refused at the capability gate**,
  and with no prompt guidance telling the model they existed (1.4, 4.4)
- `AttachSubagent` — implemented in session-core, an `unimplemented` stub in
  execution-core, so **every real subagent was absent from the lineage graph**
  (1.1)
- `listPlans`/`listTodos` — API clients with **no caller anywhere in the app**
  (5.5)
- `ToolCallCard` — **no caller**; tool args and output were collected every turn
  and rendered nowhere (5.4)
- `GetContextAssembly` — itemized, already called to build prompts, **never
  exposed** (3.5)
- `RichEventSink` — used at **1 of 56** emission sites (5.7)
- The rich-event resume buffer stored **text only**, so a reconnect lost every
  tool call, citation and title (5.7)
- The "4 unwired approval events" — **all 14 were wired**; the claim was stale
  (5.5)

Verifying before building was the single highest-value habit here. The
corresponding rule now has teeth: **12 contract tests**, all mutation-verified,
pin the invariants that were being held by comment or by a caller's discipline.

### Bugs found in this session's own earlier work

Worth recording, because each was invisible to the tests that existed when it
shipped:

- `save_memory`/`recall_memory` had **no capability binding** — advertised to the
  model, refused on first use. Every runtime-loop test injects an `Allow`
  capability-policy double that never consults the mapping (1.4).
- `turnsToTranscript` persisted neither `memoryRecallCount` nor `stopReason`, so
  the truncation warning — *"this answer may be cut off"* — **vanished on
  reload**, leaving an answer that looked complete. Caught by 5.3's three-path
  equivalence gate on its first run.
- `onUnknownEvent` logged `undefined` for an unnamed frame (`event.event` is
  optional). Caught by `tsc -b`.

### Product decisions — ANSWERED 2026-08-22

All six were decided by the owner. Where the answer was "look at what the best
harnesses do", the design below is grounded in `deepseek-harness` (MIT — port
freely) and cross-checked against our own existing vocabulary. Nothing here is
taken from `claude-code-fork` (leaked/unlicensed — shapes only, zero code).

1. **1.1 cold resume — DECIDED.** The child's answer is persisted on the
   **child run's own thread**, not the parent's. A resumed parent learns *that*
   its child finished; it learns *what* it concluded only when the user asks for
   it and grants access. So the conclusion is a **permission-gated read**, not an
   automatic injection — which also keeps ZDR per-run rather than smearing child
   content into the parent's retention scope.

2. **4.3 queued-input — DECIDED, and it is neither `steer` nor `followUp`.**
   Injected at the **tool-round boundary as a pause**, so the agent sees the new
   context and classifies it: if it changes the view, act on it now; if it is a
   continuation of, or comes after, the current task, wait for a natural stop.
   That is a third semantic pi does not have — pi forces the caller to choose the
   mode up front, and this makes the *model* decide from the content.

3. **2.4 memory provenance — DECIDED: yes, show which memories were recalled.**
   The owner's reasoning stands on its own: showing the user what we already know
   about them is transparency, not a GDPR exposure, and it is the only way to see
   how much the agent remembers and whether the memory needs correcting.

   **Grounded in DeepSeek's `context-provenance.ts`.** Its model is worth copying
   closely: provenance is *projected from the durable source record alone*, and
   the client keeps **no table of known producer ids** — "a renamed or newly
   mounted producer must never need a client release to stay identifiable". Two
   roles (`recall` for material lifted from another session, `inject` for
   everything else), a **label naming the specific producer** (the session
   titles, the file paths, the skill name — not the plugin id), and any
   unreadable shape degrades to `inject` with whatever name survives rather than
   dropping the row. The same no-allowlist property already applied to our SSE
   relay.

4. **3.1 compaction reattachment — DECIDED: need-driven, and it rides on 2.4.**
   If the user needs something compaction dropped, the model reattaches it. The
   provenance record from 2.4 is what makes that possible: you cannot decide
   whether to re-attach a segment until you know what it *was* and where it came
   from.

5. **3.5 autonomy levels + "fresh context" — DECIDED by convergence.**
   *Autonomy levels:* DeepSeek's `SandboxMode` ladder is
   `read-only → workspace-write → danger-full-access`, with a **strictly-wider**
   escalation table, a **required justification** paired with every escalation
   request ("an approval prompt without a reason, or a reason driving nothing, is
   a malformed ask"), and the check applied **per call at execution** — never
   baked into a tool schema, "because schemas are registry-global while the
   effective mode is per-call truth". That is the same three-rung vocabulary our
   `MpSandboxPolicy` already has, so the plan-approval control expresses the
   ladder we own rather than inventing a scale.

   *"Fresh context" means drop history.* DeepSeek is unambiguous and had already
   rejected the alternative: a fresh child gets "a distinct session with no seed
   while preserving the parent's cwd, so the shared working tree is the durable
   authority and neither parent conversation nor prior child history enters the
   request". The only handoff is the immutable objective, the round counter, a
   workspace-as-authority instruction, and **one bounded, semantically-validated
   structured report**. They explicitly rejected fork for this because "inherited
   completed turns are implicit, growing handoff state and violate the
   fresh-context contract", and oversized handoffs **fail rather than being
   silently truncated**. So: not "keep citations" — carry a typed report, and let
   the workspace be the authority.

6. **3.4 concurrent-agent isolation — a better idea, from the same source.**
   DeepSeek does **not** isolate per-agent filesystems for its fresh-agent loop:
   children share the parent's cwd deliberately, because the working tree is the
   durable authority. What it isolates is **context**, and what it adds against
   runaway concurrency is a **per-run child ceiling** (`maxTotalAgents`), kept
   deliberately separate from the loop's round budget "so the fixed loop's round
   budget and the generic runaway-child backstop cannot disagree".

   We already have the context isolation (`run_subagent` builds a fresh
   `LoopContext` with no parent transcript). What we lack is the second,
   independent ceiling. That is a far smaller and better-targeted change than the
   git-worktree port the item originally imagined — and it is what the reference
   actually does.

### The original questions (for the record)

Each blocked exactly one item. None needed engineering input first.

1. **1.1 cold resume** — where does a subagent's *answer* live durably?
   `RecordTerminalOutcome` is content-free by design (that is what makes it
   ZDR-safe), so a resumed parent can learn *that* its child finished but not
   *what it concluded*. Both branches have ZDR consequences.
2. **2.4 memory provenance badges** — should chat show *which* memories were
   recalled, not just how many? The `stated|inferred` provenance exists on the
   wire; surfacing recalled memory content inline is a privacy call.
3. **3.1 compaction reattachment** — which compacted-away state is worth
   re-attaching, and against what budget (§7.5).
4. **3.3 fork-semantics subagent** — needs 1.1's resume decision first.
5. **3.4 concurrent-agent isolation** — scope to the Space-workspace design, or
   something narrower?
6. **3.5 compound plan-approval control** — what *are* the autonomy levels, and
   what does "fresh context" mean operationally (drop history? re-plan from the
   goal? keep citations?).
7. **4.3 queued-input semantics** — `steer` (inject after the current tool round)
   vs `followUp` (wait for a natural stop), and what a send-during-stream with
   neither specified should do.
8. **4.5 session-as-tree** — deliberately parked pending the chat-design call.

---

House rules that bind every item: license gate per
`docs/external-ideas-harvest.md` §1; nothing "done" without source + test
evidence; check `docs/decisions/ledger.md` before re-proposing anything
rejected; the two tool-dispatch loops stay separate
(`docs/postmortem/0001-…`); verify claimed gaps against source before building
(five matrix rows were wrong once — the correction notice explains the method
error).

---

## 0. Scorecard — adoption items already SHIPPED (evidence in the audit doc)

| Item | Source | Shipped |
|---|---|---|
| Parallel tool-call dispatch (3-phase, atomic shared subagent budget) | DeepSeek | §7.2, 08-22 |
| AllowDomains fail-closed egress (the real §7.1 gap) | — | §7.1, 08-22 |
| Leaf/orchestrator subagent role split | Hermes | §7.3, 08-22 |
| Compaction-recall eval harness (`seed_turns`, 2 gold cases) | Hermes | §7.4, 08-22 |
| Postmortem + decision ledger, seeded with real history | DeepSeek | §7.6, 08-22 |
| `stop_reason` on streamed chunks incl. `stream_incomplete` | pi (no-throw envelope) | §13.5-1, 08-22 |
| Server-side plan-mode enforcement (closed Claude Code's exact hole) | CC shape | §13.5-3, 08-22 |
| Wire-identity tenancy chokepoint | DeepSeek | verified already satisfied |
| Tool-call retry, both loops, mutation-tested drift contract | DeepSeek/all | 08-22 |
| Thread `origin` dimension + chat-leak fixes + pin unification | (chat design I2) | 08-22 |
| Hermes memory hooks (see WS-2 scorecard) | Hermes | 08-22 |

---

## WS-1 · Industry-standard gaps (priority 1)

After re-verification, the honest "everyone has it, we don't" list is short —
three of the five originally claimed gaps were audit errors (plugin registry,
MCP registry, voice I/O all exist and are hardened).

**1.1 Durable/continuable subagent lifecycle.** ✅ **DONE** — durable lineage
2026-08-22, cold resume 2026-08-24. Both halves are recorded below; the lineage
section's correction of the original premise is kept because it is still the
reason the item was bigger than it looked.

**A worse gap than the item described.** The premise was that we have lineage
but no durability. In fact `subagent_edges` and `GetSubagentLineage` were only
ever written by the *orchestration* surface. The live in-loop delegation ran the
child under the parent's own `run_id` with prefixed step ids and called
`AttachSubagent` nowhere — execution-core had it solely as an `unimplemented`
test stub. So for **every subagent a real user triggered, the lineage endpoint
returned nothing.** The graph was real; the production path was not in it.

**Landed:** `run_subagent` now registers each delegation as a durable **managed**
child run via `StartManagedRun` (`parent_run_id` set, `agent_id` = the subagent
label, carrying the delegated goal), records the `parent → child` edge via
`AttachSubagent`, and settles the child's terminal obligation on **every**
outcome including refusals — an unsettled managed obligation is force-failed by
the deadline watchdog, so every path that creates one must close it.
`start_key` is `<parent_run_id>:<parent_step_id>`: identifiers only, per the
contract's "must not be derived from prompt or tool content", which also makes a
retried parent step reuse its child run instead of forking the lineage.

**Deliberately non-regressive:** the child loop still *executes* under the
parent's `run_id`. Moving step attribution to the child run would relocate
delegated steps out from under the prefix the Agent Run Console follows — a
console-visible change, and a separate decision. What the child run carries is
the delegation's own identity, goal, parent edge, and terminal receipt.

**Two checks worth noting.** Cost double-counting was investigated and is not a
risk: usage is published by model-gateway keyed by org, not per run, so a child
run is a bookkeeping record and not a billing unit. And an existing invariant
test fired — `managed_terminal_outcomes.len() == 1`, "a nested loop must not add
a second managed receipt". That assertion read as "one receipt per run" but
actually encoded "nested loops terminalize nothing", so it was **tightened, not
relaxed**: exactly one receipt for the parent run, plus a no-run-terminalized-
twice check across all runs. Strictly stronger than the count it replaced.

**Cold resume** ✅ **DONE 2026-08-24**, on the decision recorded above: the
answer lives on the **child run's own record**, and the parent reads it only with
the user's permission.

The place to put it already existed and nothing wrote to it. `runs.final_output`
has been in session-core's schema since `0001_init.sql` and `GetRun` has always
returned it — **no code ever set it.** Same built-but-unreachable shape as
`reasoning_delta`, `AttachSubagent` and the rest of this pass.

- **`RecordRunOutput`** (new RPC on `ManagedRunLifecycle`), deliberately separate
  from `RecordTerminalOutcome`: that receipt stays metadata-only, which is what
  makes it safe on every run. This is the explicit, separately authorized,
  separately auditable act of writing content — **refused outright for a
  zero-retention caller**, owner-checked against `runs.org_id`/`user_id`, bounded
  at 16 KB, and reporting `stored_chars` so truncation is reported rather than
  assumed away.
- **execution-core** writes the child's answer before settling its receipt, so a
  run reported complete already has a readable answer. ZDR is checked on **both**
  sides on purpose: the caller knows the posture without a round trip, the server
  fails closed for a ZDR credential anyway, and neither is allowed to depend on
  the other being right.
- **Two tools, and the split IS the decision.** `list_subagent_results` is
  content-free — label, goal, status, whether an answer exists — so a resumed run
  can orient itself with nobody interrupted. `read_subagent_result` returns the
  answer and is approval-gated on **every** posture.
- **Why a new permission predicate rather than `is_risky_tool`.** The obvious
  implementation is silently a no-op where it matters: ordinary chat runs use the
  `auto` posture, where a risk-based gate never fires, so the conclusion would
  have flowed into the parent's context with nobody asked. Reading is genuinely
  not risky — the gate is about **disclosure**, so it is its own predicate
  (`permission::requires_consent_to_disclose`) and a test asserts the tool must
  NOT be classed risky, or the predicate would be dead on `ask`.
- **Scope is server-verified.** A model-supplied run id is untrusted input; the
  read is confined to direct children of the calling run via
  `RunDetail.parent_run_id`, with the same refusal text for "not yours" and
  "does not exist" so the tool is not an oracle for which runs a tenant has.
- **A delegated subagent is refused with its reason.** `MAX_DEPTH == 1` means it
  has no children, so an empty list would read to it as "my delegations found
  nothing" — a claim about work it never did.
- **Two honesty fixes the work surfaced.** Every approval was reported as
  `APPROVAL_KIND_DESTRUCTIVE` with the reason `"tool 'X' requires approval"`;
  telling someone that reading a finding is destructive is how a prompt stops
  meaning anything, so a disclosure is now `APPROVAL_KIND_PERMISSION` with a
  reason naming what would be disclosed and where it would go. And the
  capability-seed contract test scanned **all** `*.sql`, so an id named only in a
  `*.down.sql` DELETE looked seeded — it now scans `*.up.sql` only, verified by
  removing the new up-migration and watching it fail.
- **Capability**: `cap.agent.lineage.read`, seeded by
  `0013_subagent_result_read_capability.up.sql`, risk `low` (classing it high
  would produce a *second* approval prompt on top of the consent gate — two
  prompts for one decision), starting `unavailable` until a health authority
  attests it, exactly like every row in 0008.

*Status:* source-verified, not live-verified. 2 237 Model Plane Rust tests pass;
Go bindings regenerated with `buf.gen.go-only.yaml` (no generator churn);
capability-core builds, vets and tests clean. Four invariants mutation-verified:
dropping the ZDR guard, writing the answer onto the parent run, removing the
up-migration, and the two approval-label mutations. The new RPC and the
`runs.final_output` write want a staging pass before production trust.

*Status:* source-verified, not live-verified — 447 execution-core tests pass,
including a new one proving a delegation still completes and still returns its
answer when `StartManagedRun` is unavailable (lineage is bookkeeping; the
delegated work is the product), and that no receipt is written for a child run
that was never created. Registering managed child runs is a live behaviour
change touching run counts, RUN_COMPLETED (so one learning review per subagent —
`LEARNING_RUN_EVENTS_ENABLED` is the lever) and GDPR purge counts; it wants a
staging pass before it is trusted in production.

**1.2 Generated-docs freshness CI gate.** ✅ **DONE 2026-08-22**, with the
target narrowed on inspection. `docs/endpoint-map.md` cannot be the artifact:
it is **curated**, not generated — its `v3` relevance ratings and "do not wire
yet" flags are human judgment, and a generator would overwrite precisely the
part that carries the value. There is no generator for it, and writing one would
be a doc-destroying refactor dressed as a CI gate.

What *is* mechanical is the thing the stale claim was actually about: the SSE
chat-event surface. So the gate covers that, and covers a second drift with
teeth:

- `apps/Model Plane/scripts/gen-sse-event-taxonomy.py` parses `ChatEvent`'s
  `name()` and `family()` arms out of `sse_events.rs` and the `case '...'`
  labels out of the SPA's `chat-client.ts`, then writes
  `apps/Model Plane/docs/sse-event-taxonomy.md` (14 events with their opt-in
  family). `--check` fails on drift with a unified diff.
- **It also asserts producer→consumer coverage.** A backend event with no case
  in the SPA client is parsed and silently discarded — the stream succeeds, the
  feature simply never appears, and nothing fails. That is now a build error
  rather than something only the runtime `onUnknownEvent` log would hint at.
- It further asserts the five non-`ChatEvent` names the client handles
  (`connected`/`chunk`/`done` transport frames, `citations`/`search_results`
  legacy aliases) are *still* handled, so silently deleting one is caught too.
- `.github/workflows/sse-taxonomy.yml` runs `--check` on any touch to the four
  relevant files. Zero dependencies on purpose — it parses source text, so it
  keeps working without the Rust or pnpm toolchains.
- The workflow **also proves the checker can fail**, every run: it drops one
  documented event and requires `--check` to reject it. A freshness check that
  cannot fail is worse than none, because it reads as coverage.

*Acceptance met, both ways:* a deliberate doc drift fails with a legible diff
(verified — the removed `memory_recall` row shows as a one-line `+`), and
renaming one SPA `case` fails with a message naming the dropped event and the
file to fix.

**1.3 Eval harness in CI.** ✅ **DONE — was already done** (2026-08-22
verification). `AGENT_QUALITY_PLAN` P6's "not in CI" claim is **stale**:
`.github/workflows/eval-lab.yml` runs the credential-free `pytest tests/` on
every push/PR touching the harness, and goes further than this plan asked — it
parses the JUnit XML and asserts `ran >= 10`, so a suite that silently skipped
*everything* fails instead of passing vacuously. Verified locally with the
compaction cases added earlier: 18 offline ran, 14 live self-skipped, assertion
passes. **No work needed.**

**1.4 Single shared permission↔capability schema check** ✅ **DONE 2026-08-22.**
Scoping this item relocated the drift, which is not in `sandbox.rs`/`policy.rs`:
those own `MpSandboxPolicy`/`MpNetworkPolicy`, self-contained enums with no
second vocabulary to disagree with. The real coupling is **tool name →
capability id**, and it is load-bearing: `execute_step_inner` evaluates
capability policy *before any dispatch*, and `trusted_capability_id` returning
`None` becomes a hard refusal there.

**It had already drifted — on work from earlier in this same session.**
`save_memory` and `recall_memory` were added to `offered_tool_defs()` (the WS-2
memory work) with no arm in `trusted_capability_id`. They were advertised to the
model and refused on first use. The entire runtime-loop suite passed, because
every one of those tests injects an `Allow` capability-policy double that never
consults the mapping — so no existing test could have caught it, and none did.
Both are now bound to `cap.memory.index` / `cap.memory.search`, which are
capability-core's own seeded ids (`RiskLow`, scope `workspace`) rather than new
names minted at the call site.

Two contract tests in `runtime_loop/agent.rs`, beside the tool list they guard
(it is module-private, so an integration test cannot reach it — and an
API-level assertion beats text matching):
1. every tool in `offered_tool_defs()` resolves a capability binding;
2. every id it binds to is actually **seeded** by capability-core — binding to
   a plausible-but-unseeded id fails closed at runtime and looks identical to no
   binding at all. This reads across service boundaries the way
   `cross_service_loop_contract.rs` does, for the same reason (separate
   deployment, no build dependency either way).

Test 2's first draft reported **ten false positives** by assuming `registry.go`
was the only seeding source; capabilities come from `registry.go` *and* the
`capabilities` INSERTs in `migrations/*.up.sql`. It now scans both, and asserts
it found migrations at all so a path error cannot pass vacuously.

*Acceptance met:* mutation-tested both ways — deleting the `save_memory`
binding, and repointing it at an unseeded `cap.memory.write` — each fails the
corresponding test with a diagnostic naming the fix. Full suite: 446 pass.

## WS-2 · Hermes memory — full parity or better (priority 2)

Scorecard of the ~20-member `MemoryProvider`/`MemoryManager` surface:

| Hermes member | Status |
|---|---|
| `on_pre_compress` | ✅ 08-22 (`summary_prompt_with_memory`, anti-fabrication framing, eval harness measures it) |
| `prefetch` (async, timeout-bounded) + `system_prompt_block` | ✅ 08-22 — SSE chat path now prefetches (400ms bound), injects the shared `Relevant memory:` block |
| `is_trivial_prompt` skip | ✅ 08-22 — ported + **Norwegian-first extension** |
| `recall_status` | ✅ backend (`memory_recall` SSE event, `memory` family) · ❌ UI indicator → WS-5 |
| `get_tool_schemas` / `handle_tool_call` | ✅ 08-22 — `save_memory`/`recall_memory` on the governed loop; ZDR-refused with honest text; blocked in plan mode + delegated subagents (`is_restricted_context_write`, Hermes `DELEGATE_BLOCKED_TOOLS` parity); dead gateway arm deleted |
| `on_delegation` | ✅ 08-22 — bounded fire-and-forget delegation record, non-ZDR only |
| `sync_turn` / `on_session_end` | ≙ **G7 learning loop** — built, **inert**: nothing publishes RUN_COMPLETED to NATS in production. → **2.1** |
| `on_session_switch` | N/A by architecture (durable threads; writes synchronous server-side) — documented |
| `on_memory_write` (multi-provider fanout) | N/A — session-core MemoryService *is* the provider |
| one-external-provider cap · reserved-tool-name guard · durability-classed `flush_pending` | structural (boot-selected backend; static tools; synchronous RPC writes) — documented |
| `backup_paths` | ops concern → Postgres backup policy, out of harness scope |

Remaining work, in order:
- **2.1 Turn G7 on.** ✅ **DONE 2026-08-22.** The publisher exists and is wired:
  `session-core/src/learning_events.rs` enqueues a canonical `RUN_COMPLETED`
  envelope onto the existing `session_audit_outbox` inside the terminalization
  transaction, from **both** terminal paths — the managed one
  (`terminalization.rs:1092`, which is where a plain chat turn ends up via
  model-gateway's `GatewayDirect` → `RecordTerminalOutcome`) and the legacy
  unmanaged one (`grpc.rs:2580`). The `audit_publisher` drainer routes it by its
  own subject (`classify_outbox_subject` → `OutboxRoute::RunEvent`), a SAVEPOINT
  keeps a learning-loop failure from ever rolling back a user's turn, and
  `LEARNING_RUN_EVENTS_ENABLED` is the default-ON cost lever. The consumer half
  was already live (`capability-core/cmd/main.go:636`).

  **What this item actually needed was the ACL proof, and that was the real
  risk.** NATS reports a publish-permission denial *only* to the publisher's
  async error handler and drops the message — `Publish()` still returns nil. So
  a missing grant leaves both services logging success, the drainer burning
  attempts to `terminal_at`, and no skill ever learned; no in-process test can
  see it. The grant is present in `deploy/nats.conf`, and it is now **pinned
  live**: `scripts/tests/learning-acl-grant-test.sh` boots a real `nats-server`
  on the **production config with the real principals** and
  `TestRunEventACLGrant_ProductionConfig` asserts session-core-runtime's publish
  is delivered to capability-core-runtime and parses through the real
  `ParseRunCompleted`. It carries a **negative control** (the same principal
  publishing a subject it does not hold must be denied) so a wide-open server
  cannot make the positive assertion vacuous. Mutation-verified: deleting only
  that one grant line fails the test with a diagnostic naming the fix.

  Also corrected a stale comment in `consumer_test.go` asserting
  `pkg/envelope.Envelope` has no `zdr` field — it does now (`Zdr *bool`), and
  the reason to keep injecting the flag into raw JSON is different (reaching the
  *absent* wire state, which `omitempty` hides).

  *Remaining for full acceptance:* the end-to-end "skill row from a real
  completed run" needs the deployed stack plus a live LLM — every hop is now
  either unit-tested or live-verified, but the seam-to-seam run is not, and is
  called out as such rather than claimed.
- **2.2 Memory lifecycle honesty (GDPR).** ✅ **DONE 2026-08-22.** Two of the
  three sub-items turned out to be **already fixed**, and verifying that
  relocated the real defect:

  - *"Letta-side `DeleteMemory` no-op"* — **stale.** It delegates to
    `store.Delete`, and all three backends implement it. The per-memory path is
    sound end-to-end: `memory_grpc::delete_memory` authorizes, deletes its own
    row scoped to `(org_id, user_id)` **in the SQL**, `404`s if that matched
    nothing, and only then calls the semantic tier — so the semantic client's
    id-only delete (upstream takes no namespace filter) can never be reached for
    an id the caller does not already own. That reasoning is documented at the
    call site and it checks out.
  - *"Erase memory on thread delete"* — the Postgres half was already there
    (`agent_memory`, `memory_index` in `delete_thread_rows`).
  - **The actual gap: neither bulk path propagated to the semantic tier.**
    Memory is dual-written — durable row plus a letta-bridge copy keyed by the
    *same* `memory_id`. `delete_thread`/`delete_threads` and
    `gdpr::purge_organization_data` were pure SQL, so they deleted the durable
    rows and left the semantic copies resident, with no id left anywhere in
    Postgres to find them by. A user deleting a thread got a smaller deletion
    than they asked for; an org erasure reported a completeness it had not
    achieved.

  Fixed in a new `session-core/src/memory_erasure.rs`: both bulk deletes now
  `RETURNING id, owner`, and propagate **after commit** (a degraded vector store
  must never roll back a delete Postgres already honoured). `PurgeSummary` gained
  `semantic_memory` plus `erasure_is_complete()`, deliberately **excluded from
  `total()`** so one number does not come to mean two things.

  **Reading the existing code corrected the design.** `delete_space_threads`
  already erased semantic twins — through a durable receipt ledger
  (`space_deletion_semantic_memory_receipts`, with `attempts`/`last_error` and an
  idempotent-retry-safe reconciliation query). It sets a *stricter* confirmation
  bar than the one drafted here: erased only when `deleted == true` **and** no
  degradation, because a `deleted == false` reply cannot distinguish an
  idempotent prior delete from an unknown record. The planned third
  "absent, nothing to do" bucket was an assumption dressed as an answer, and was
  dropped — two confirmation standards for one operation inside one service is
  exactly the drift the ledger exists to prevent. That path keeps its durable
  ledger and opts out of the new helper explicitly.

  Guarded by 9 tests, including source-text caller contracts modelled on
  `learning_events`'s "both terminal paths must emit": the bug was a *missing*
  line at an unwatched call site, which no unit test of the helper and no type
  can catch. They pin that both bulk deletes capture ids, that propagation
  follows the commit in every path, that the durable path's opt-out stays
  explicit, and that `total()` never absorbs the semantic count.
  Mutation-verified — moving one propagation call above its commit fails with
  the diagnostic naming the consequence.

  *Known limitation, stated rather than papered over:* propagation is inline and
  best-effort, so a degraded tier is **reported and counted**
  (`mp_session_semantic_erasure_unconfirmed_total`, plus a warning), not retried.
  Extending the receipt-ledger pattern to org purge is the fuller answer;
  `OrganizationErasure` carries no request id to key receipts by today, so that
  needs a contract change. Tracked as follow-up. What is closed is the silent
  orphaning — incompleteness is now visible.
- **2.3 Per-user scoping** ✅ **DONE 2026-08-22.** **Semantic backend promotion:
  blocked, with the blocker now identified precisely.**

  Per-user scoping on the **durable** path was already correct — session-core's
  query is `org_id = $1 AND (session_id = $2 OR (session_id IS NULL AND scope <>
  'user') OR (session_id IS NULL AND scope = 'user' AND owner = $3))`: your own
  user-scoped memories plus everything unowned.

  The **semantic** tier was not. `letta-bridge`'s `Search` took no user id at
  all — org plus an optional thread was the whole filter. In production nothing
  leaked, because the one caller always passes a `thread_id` and a thread has one
  owner. But **the boundary enforced nothing**: letta-bridge does not require a
  thread id, so any caller omitting it got every user's personal memories in the
  org. Safety by caller discipline, on the path the chat memory prefetch (§7.9,
  item 5.1) now uses on every turn.

  Closed at the boundary: `SearchMemoryRequest.user_id` (field 7), threaded
  through `memory_grpc::search_memory` → `letta_adapter::search_detailed` →
  letta-bridge → all three stores, with pgstore and memstore applying the same
  rule as the durable query (`user_id = $n OR user_id = ''`). The owner is taken
  from the **verified** thread, never from the request: every gateway-side
  `SearchMemoryRequest` leaves `user_id` empty on purpose, because a
  caller-supplied user id would be forgeable scoping.

  Mutation-verified — deleting the filter fails the new test with
  `[mem-alice mem-bob]`, i.e. Bob's personal memory returned to Alice.

  **The promotion blocker (2.3's second half).** `agentmemory` *cannot express
  the rule*. The correct filter is "owned by this user OR owned by nobody"; the
  upstream search filter is equality-only and its response carries no `user_id`,
  so neither the request nor a client-side pass can express the OR. It now sends
  an equality filter, which **narrows** — the user's own memories only, org-level
  ones lost from the semantic tier. That is the correct direction to fail
  (under-recall is a quality regression; over-recall is another user's private
  memory in someone's chat) but it is a real gap, and it means this backend
  **cannot replace pgstore** until either the upstream supports the OR or the
  search response carries `user_id`. Recorded at the call site, not just here.
  `DEGRADED_SEMANTIC_UNVERIFIED` remains accurate.

  Go bindings regenerated with `buf.gen.go-only.yaml`; the 5 changed `*.pb.go`
  files are all additive field additions from this session's proto work, with no
  protoc-version churn. Full Rust workspace: **2172 tests pass**; letta-bridge
  Go build/vet/tests clean.
- **2.4 Beyond-Hermes (the "+ more")** ✅ **DONE 2026-08-24.** Memory
  provenance is now shown to the user, following DeepSeek's `context-provenance`
  model rather than the two-badge sketch this line originally described.

  The premise needed one correction: the roadmap said `stated|inferred`, a
  *boolean*. The proto has **three** values and its own comment says the third
  must never be rendered as stated. A two-state badge would have had to pick a
  side for `MEMORY_PROVENANCE_UNSPECIFIED` — every pre-provenance row — and
  picking `stated` manufactures consent the record does not support.

  - **`memory_provenance.rs`** (new): `MemoryRole` (`recall`/`inject`),
    `MemoryOrigin` (`stated`/`inferred`/**`unrecorded`**), and
    `project_recalled_memory`. Ported from DeepSeek's presentation rule: the
    projection reads the durable record ALONE — no client-side table of known
    topics or producers — so a new topic stays identifiable without a release,
    and an unreadable value degrades to `unrecorded` instead of dropping the row
    or guessing. The label is the entry's own topic, never its id ("an id is not
    a name"). 10 tests, including provenance value `9_999`.
  - **SSE**: `ChatEvent::MemoryRecall` carries the projected views alongside the
    count. `sse.rs`'s prefetch now projects **before** reducing entries to prompt
    text, so the display record and the injected content come from the same
    entries and cannot disagree — previously the prefetch discarded topic and
    provenance in a `.map(|entry| entry.content)`.
  - **SPA**: `RecalledMemory` + `normalizeRecalledMemories` (origin degrades to
    `unrecorded`, never `stated`); `MemoryRecallNotice` became an expandable
    disclosure — collapsed shows the count, expanded shows label, origin, an
    `organisasjon` scope marker for `inject`, and a preview. Persisted through
    transcript/rehydrate/merge with an `isRecalledMemory` guard that re-narrows
    `origin` on reload. CSS gives an affirmative tint to `stated` only: a
    confident colour on a guess is exactly the wrong signal.

  Per the decision recorded above, this is a transparency feature, not a GDPR
  risk: it shows the user what we already hold about them, and makes a stale
  memory visible enough to correct.

## WS-3 · Claude Code benchmark (priority 3 — shapes only, zero code)

- **3.1 Escalating compaction + selective reattachment** ✅ **DONE 2026-08-24.**
  Built need-driven, per the decision above: the model reattaches when it finds
  it is missing something, rather than the gateway pre-attaching a budgeted set
  of "recently touched" state on every compacted turn.

  The reframing that made this small: compaction edits the **prompt**, never the
  durable thread. So "I cannot see that" was only ever true of one request, and
  recovery needs no new storage — just a read.

  - **`context_reattach.rs`** (new): `select_reattachment` picks messages from
    the durable thread by substring query (empty query → oldest history, which
    is exactly what compaction drops), excludes system messages and anything
    already in the prompt, and bounds the result at 20 messages / 6 000 chars.
    Returns 1-based positions in the thread so the model can say *where* in the
    conversation something was said. Substring rather than fuzzy on purpose: a
    recovery that returns loosely-related turns is worse than one that returns
    nothing, because the model cannot tell which it got. 9 tests.
  - **The tool**: `reattach_context` in `builtin_tool_defs()`, dispatched in the
    read-only loop. Thread- and org-scoped from the verified request, never from
    model input. An empty session bearer is a stated **error**, not an empty
    result — session-core answers an unauthenticated `ListConversation` with a
    rejection, and a rejection reduced to "no messages" would have the model tell
    the user their earlier message never existed.
  - **Both compaction notices now offer recovery.** `DROPPED_HISTORY_NOTICE`
    changed from a dead end to an instruction, and `SUMMARY_PREFIX` gained the
    same — the summarised path is the *common* case (dropping is the summariser-
    outage fallback), and a summary losing a detail is the likelier reason the
    model needs the original text back.
  - **The prompt is actually threaded in.** `run_tool_rounds` snapshots the live
    message list per round and passes it down, so a recovery spends its budget on
    what compaction dropped — and a "nothing matched" means the history really
    lacks it, rather than the match sitting in front of the model already.
  - **`context_recovery_contract.rs`** (new, 3 tests) pins the three links that
    each fail silently and that a compile cannot see: the notices naming a tool
    that exists, the arm being handed the real prompt rather than `&[]`, and the
    snapshot following the round rather than the turn's opening prompt. All four
    invariants mutation-verified.
  - **Audit**: `reattach_context` and `recall_memory` were landing on
    `unclassified` in the tool ledger by default. Both read customer content and
    are now `customer_private`, with a test that fails for any private-content
    read left out of the table.
  - **SPA**: presented as a `read` intent rather than `generic`.

  986 model-gateway tests pass; `tsc -b` clean.
- **3.2 Extended thinking as a budgeted mode** ✅ **DONE 2026-08-22.** The
  premise needed correcting first: `reasoning_delta` existed as a *type*, not a
  behaviour. **Nothing anywhere emitted it** — `ChatEvent::ReasoningDelta` was
  constructed only inside its own module's tests, while the SPA had a
  `case 'reasoning_delta':`, `ChatTurn.reasoning` state, normalizers and a
  "Reasoning trace" step all waiting on an event that never arrived, and no
  provider was ever asked to think (`supports_thinking` was an advertised
  capability flag with no request path). A fully-built consumer, a documented
  event, a doc row — and zero producers. Exactly the dead-but-visible feature
  §13.3 rejects.

  Now real end-to-end, backend-only (the consumer was already complete):
  - **proto**: `InferRequest.thinking_budget_tokens` (13),
    `InferChunk.reasoning_delta` (8) — reasoning carried on its own channel so a
    client can render, hide or drop it independently, and so it can never be
    concatenated into the answer by a client that does not know about it.
  - **inference-core**: `resolve_thinking_budget` gates the Anthropic `thinking`
    parameter and **degrades to no-thinking rather than erroring** on every bad
    case (no budget, a model that rejects the parameter, under the 1024 floor, or
    a budget that leaves no room for an answer). Model gating is an **exclusion**
    list, not an allowlist, and deliberately: every Claude family from 3.7 on
    supports thinking, so an allowlist would silently drop it for each new model
    until someone remembered — failing quietly toward "the feature doesn't work".
  - **A real bug fixed in passing**: the stream parser read `delta.text`
    unconditionally, so with thinking enabled every `thinking_delta` would have
    become an **empty answer chunk** — the reasoning silently discarded and a run
    of no-op chunks sent in its place. `split_content_block_delta` now routes
    each delta to exactly one channel, with `signature_delta` (a thinking block's
    signature) and `input_json_delta` (tool arguments) correctly yielding
    neither.
  - **gateway**: emits `ChatEvent::ReasoningDelta`, never appending it to
    `assistant_output` — reasoning is the model's scratchpad, not part of the
    persisted turn. New `thinking.rs` maps the effort dial to a budget
    server-side: a client picks `quick|standard|deep`, never a raw budget, and
    the budget **steps down rather than truncating the answer** when the
    response ceiling is tight. `standard`/unset yields 0, so the ordinary turn is
    byte-identical to before.
  - **SPA**: `effort?: 'quick'|'standard'|'deep'` on `ChatInvokeRequest`, omitted
    from the wire entirely unless non-default.
  - The gRPC Invoke path binds and **explicitly ignores** reasoning with the
    reason at the site — `InvokeChunk` has no reasoning field, and adding one
    with no consumer would recreate the very gap this item closed.

  22 tests (9 inference-core, 4 thinking profiles incl. a headroom property
  test over 8 ceilings × 3 profiles, 3 SPA, plus suite updates). 236
  inference-core / 894 model-gateway / 348 SPA chat+api tests green; `tsc -b`
  clean — which caught a **latent type error in my own 5.1 work**
  (`event.event` is optional, so an unnamed frame would have logged
  `undefined` as a "new event"), now guarded.

  *Not done:* OpenAI reasoning-model mapping (`reasoning_effort`). Its reasoning
  content is generally not streamed as deltas, so there is no equivalent
  producer to wire; adding the request parameter without one would spend tokens
  for nothing visible.
- **3.3 Fork-semantics subagent** ❌ **REJECTED 2026-08-24 — as the reference
  itself rejected it.** Resolved on decision #5 above ("look at what the best
  harness does"), and what it does is refuse this shape.

  DeepSeek's fresh-agent contract is explicit: a child gets "a distinct session
  with no seed while preserving the parent's cwd, so the shared working tree is
  the durable authority and neither parent conversation nor prior child history
  enters the request". Fork was considered and rejected there because "inherited
  completed turns are implicit, growing handoff state and violate the
  fresh-context contract". Building fork here would adopt the thing the reference
  discarded.

  **The half we would have built is already built and already tested.**
  `run_subagent` constructs a fresh `LoopContext` with no parent transcript, and
  `subagent_runs_a_real_nested_loop_in_an_isolated_context_and_returns_its_answer`
  asserts it in both directions: the child's history is "system preamble + the
  delegated goal, nothing inherited", the parent's goal must not appear in it,
  and the child's intermediate tool traffic must not appear in the parent's.

  **The typed handoff report is deferred to 3.5, not skipped.** DeepSeek's
  `{status: continue|complete|blocked, summary, evidence, nextSteps, blocker}`
  exists because its loop is *iterative with a fresh context per round* — the
  report is the only state carried across rounds, and its `status` is what drives
  the loop. Our delegation runs a child once to completion; there is no next
  round for a status to drive, and no code branches on one (verified: the only
  consumers are `ToolExecution` text for the model, plus `settle`/`record`
  bookkeeping). Building the struct here would be a type nothing reads — the
  premature abstraction this repo's own ledger rejects. 3.5's fresh-context
  approval loop is where a status genuinely drives control flow, so the report is
  built there and delegation adopts it.

  *Trigger to revisit:* the first piece of **code** (not prose) that needs to
  branch on a delegation's outcome beyond success/failure.
- **3.4 Worktree-style isolation for concurrent agents** ✅ **DONE 2026-08-24**,
  narrowed on decision #6 above: the reference does not isolate per-agent
  filesystems at all — children deliberately share the parent's cwd, because the
  working tree is the durable authority. What it isolates is **context** (which
  `run_subagent` already does, see 3.3) and what it adds against runaway
  concurrency is a **per-run child ceiling**, kept deliberately separate from the
  round budget. So a literal git-worktree port would have been the wrong change.

  `subagent::MAX_TOTAL_CHILDREN = 8`, claimed with a run-scoped
  `AtomicU32` before the round pool is touched.

  **A wrong premise, corrected by the first test failure.** The runaway I set out
  to bound was fan-out *within* a round — and that turned out to be impossible
  already: the first `spawn` claims the entire remaining pool with `swap(0)`, so
  every sibling in the same round is refused for lack of budget. One delegation
  per round is the real maximum. The actual runaway is *across* rounds: with a
  generous `max_rounds` a run can open dozens of durable child runs, lineage rows
  and managed obligations, one per round. The test now proves that shape, with 64
  rounds of budget so a refusal cannot be the pool talking.

  Two ordering details that carry the correctness:
  - **The slot is claimed BEFORE the round pool.** Refusing after the `swap(0)`
    would report "no budget left" for a delegation actually refused for fan-out
    *and* strand the pool for the rest of the round.
  - **A refused claim is not returned.** Handing the slot back would let an
    unbounded number of refused attempts retry into the same slot.

  The refusal names the ceiling and is asserted NOT to mention the round budget —
  a breadth refusal that blames budget sends the model to re-plan the wrong
  constraint. Mutation-verified by raising the ceiling above the test's fan-out
  and by weakening the guard from `>=` to `==`.
- **3.5** ⚠️ **TWO OF THREE DONE 2026-08-22** (unblocked by 5.3).

  **Skills budget cap ✅.** Skill injection was capped by **count**
  (`MAX_INJECTED_SKILLS`) and not by size — skill bodies are operator-authored
  free text with no length limit, so a few long skills could take more of the
  prompt than the conversation they exist to steer. Nothing fails; the model just
  has less room and answers worse, which is the kind of cost that never gets
  traced back to a constant. `fit_skill_blocks` (8,000 chars ≈ 1% of a 200k
  window) in **both** loops.

  **Degrade before drop, and say so.** Truncating keeps the higher-scoring
  skill's opening guidance, which is where operators put the important part. But
  a silently truncated *instruction* is worse than a dropped one — the model acts
  on half a rule believing it is whole — so every cut block carries an explicit
  marker, anything that cannot keep `MIN_SKILL_CHARS` is dropped rather than
  reduced to a stub, and drops are counted and logged. A char budget rather than
  a token one, deliberately: the model (and so the real window) is resolved
  downstream by inference-core's intent layer, so any token figure here would be
  a guess dressed as a measurement.

  `tests/skill_budget_contract.rs` pins both loops' budgets *and* their
  truncation marker together — drift would mean chat and deployed agents disagree
  about how much prompt skills may take, showing up only as "the agent answers
  worse than chat". Mutation-verified. 10 tests incl. a
  budget-never-exceeded property over four input shapes and multi-byte
  truncation safety.

  **Context inspector ✅.** The data existed all along: `GetContextAssembly`
  returns itemized `ContextSegment { kind, content, estimated_tokens }` plus a
  total, and model-gateway **already called it to build prompts** — it was simply
  never exposed. So the one surface that could answer "why did it answer from
  *that*?" was unreachable. Wired end to end: a model-gateway route, a Verevon
  gateway proxy, an SPA client, and a collapsible panel in the Steps tab
  (deliberately not a fifth chat tab — that is an IA decision, and Steps is
  already the process-visibility surface).
  - Segment **content** is shown, not just sizes: "900 tokens of grounding" does
    not answer the question the inspector exists for.
  - The assembler's own total is reported rather than summed from the segments —
    if the two ever disagree, an inspector should show what the assembler
    believes.
  - Fetched only while the tab is open: assembling a context window is real
    backend work, and doing it every turn in case someone looks is exactly the
    cost that never shows up as a bug.
  - Deliberately does **not** delegate a Data Plane credential (unlike the
    prompt-build path) — otherwise opening a panel could trigger, and bill for,
    live grounding fan-out.
  - A failed read, a loading read, and a genuinely empty window are three
    distinct states, because rendering absence as blank makes only one of them
    look like a problem. A zero budget omits the percentage rather than showing
    `NaN%`.

  **Compound plan-approval control ✅ DONE 2026-08-24**, on decision #5 above:
  the ladder is DeepSeek's, expressed in the vocabulary we already own
  (`MpSandboxPolicy`), so a granted rung and the isolation it implies cannot
  drift apart.

  **The premise was worse than "no control".** `ExitPlanMode` existed, was
  served over gRPC, and had **no production caller** — plan mode could be entered
  from the composer and never left. The grant it makes was neither named nor
  justified anywhere: any caller with write access could flip a run out of plan
  mode and nothing recorded what authority it gained or on what grounds.
  `ApprovalKind::Plan` was likewise only ever mapped in conversion helpers, never
  produced. So the control that decides how much a run may do had no way to be
  exercised at all.

  - **`AutonomyRung`** in `runs.proto` — `read_only | workspace_write |
    danger_full_access`, strictly ordered. `UNSPECIFIED` is deliberately in the
    enum: an older caller that never set the field must not read as a grant.
  - **`mp_contracts::autonomy`** (new) holds the one implementation of the rules,
    in the contracts crate because model-gateway decides a grant and
    execution-core enforces it and neither may depend on the other — two copies
    of an ordering is two chances for a grant to mean something wider on one side
    than the other. `AutonomyEscalation::request` is the only constructor, so a
    malformed ask cannot exist as a value: it refuses a target that widens
    nothing, an unnamed target, a justification under 12 characters ("ok" is
    non-empty and says nothing), and an oversized one — **refused, not
    truncated**, because a truncated reason reads as a complete one. 10 tests.
  - **Per call at execution, never in a schema.** `rung_required_for` derives the
    rung a call needs from the SAME classifiers the posture gates use, so a call
    cannot be "risky" to one gate and "read-only" to the other. It reads the
    *arguments*: `execute_provider_action` is `read_only` for `pages.list` and
    `danger_full_access` for `pages.post`, which is exactly why this cannot live
    in a tool definition — "schemas are registry-global while the effective mode
    is per-call truth".
  - **The state plan mode cannot express, which is the point.** A run granted
    `workspace_write` may write its report and still be refused a send, publish,
    book or pay. Plan mode is all-or-nothing; the posture (`auto`) gates nothing.
  - **Non-regressive by construction.** `check_autonomy_rung` returns `Ok` for an
    unstated rung, so every run that predates the ladder behaves exactly as
    before — while `mp_contracts::autonomy::permits` still treats `UNSPECIFIED`
    as the narrowest, which is the right answer to the different question of what
    a grant covers. Wiring one to the other would either break every existing run
    or turn an unset field into a grant; a test pins both.
  - **Reachable, which is the part that was missing.** `POST
    /v1/runs/:run_id/plan-approval` → BFF proxy → SPA `PlanApprovalControl`: the
    person picks a rung, types a reason, and the grant is validated at three
    layers (HTTP edge for a usable message, coordinator as the rule, session-core
    because a server that trusts its caller to have checked has no rule at all)
    and persisted onto the run via `SetRunMode`'s new fields, so the next
    `RunAgentRequest` carries it. `read_only` is deliberately NOT offered as a
    grant — approving it would change nothing while still leaving plan mode.
  - **The grant and its reason travel together**, onto the
    `RUN_PLAN_MODE_EXITED` event and into `runs.metadata`: a grant with no reason
    is unreviewable afterwards, and a reason with no grant does not say what
    changed.
  - **`autonomy_ladder_contract.rs`** (new, 4 tests) pins the call-shape
    properties a compile cannot see, all mutation-verified: the requirement is
    derived from the arguments and not the name, the gate runs in the loop and
    never in the tool catalogue, an unstated rung stays unconstrained there while
    staying narrowest in the contract, and both grant validators call the shared
    rule.

  *Also carries 3.3's typed handoff report* — see 3.3 for why it belongs here:
  DeepSeek's `{status, summary, evidence, nextSteps, blocker}` exists because its
  loop is iterative with a fresh context per round and the status *drives* the
  loop. Our delegation runs a child once to completion; the report becomes
  load-bearing only in an iterative fresh-context loop, and building the struct
  before then is a type nothing reads.

  Verified 2026-08-24: **2260** Model Plane Rust tests, **428** Verevon gateway
  tests, **1188** frontend tests / 155 files, `tsc -b` clean, build succeeds,
  lint at the pre-existing baseline, Go bindings regenerated with no generator
  churn, and all five Go modules build/vet clean.
- **Anti-patterns stay rejected** (audit §13.3): no LLM on the permission hot
  path, no reminder-injection sprawl, no dead-but-visible features — the
  `save_memory` tombstone comment is the enforcement pattern.

## WS-4 · pi parity (priority 4 — MIT)

- **4.1 Fail tool calls in a length-stopped message.** ✅ **DONE 2026-08-22.**
  Found model-gateway's inline loop already did this — and more precisely than
  pi: pi fails *every* call in a length-stopped message, whereas providers emit
  content blocks in order, so only the FINAL `tool_use` block can be
  half-written. Failing the earlier, complete calls discards valid work.
  execution-core's governed loop ignored `stop_reason` entirely; it now applies
  the same precise rule via `retry::truncated_tool_call_index`, refusing the
  truncated call *before* the purpose-lock/dedup checks (its name and args are
  both unreliable) and telling the model why so it can re-issue. A new
  cross-loop contract assertion pins that both loops read the provider's
  ceiling vocabulary identically (`"max_tokens"`/`"length"`, trimmed +
  case-folded) — drift would mean the same truncated round is refused on one
  surface and dispatched on the other. 3 new tests incl. a behavioural one
  proving the complete sibling call still runs and is audited.
- **4.2 Typed `ProviderError::TooLong`** ✅ **DONE 2026-08-22.** Classification
  moved to the only layer that sees the raw provider body. New
  `inference-core/src/provider/overflow.rs` owns the **authoritative** table
  (12 → 24 markers, 6 → 7 exclusions) and `classify_http_failure`, wired into
  all 5 provider HTTP-failure sites (3 OpenAI/Azure, 2 Anthropic). Additions are
  attributed to real provider wordings — Anthropic's
  ``input length and `max_tokens` exceed context limit`` and `request_too_large`,
  OpenAI's `this model supports at most` and `reduce the length of your prompt` —
  not invented breadth for a count.

  **The Status code deliberately does NOT change.** Overflow still maps to
  `unavailable`, because that is what lets `FallbackChain` move the request to a
  larger-context provider — a prompt too long for an 8k model may well fit a
  200k one. Re-coding it to `invalid_argument` (semantically tidier) would have
  silently deleted a working recovery path. What is added is the *type*: the
  trailer `x-mp-provider-error: too_long`, with the provider's original text left
  verbatim in the message so every existing caller, log grep and test behaves
  exactly as before. Purely additive.

  Gateway now reads type before prose via `is_context_length_status(&Status)`.
  Its own table survives as an explicitly-labelled **rolling-deploy fallback**
  (new gateway + old inference-core sends no trailer, and in that window the
  gateway's table is all that stands between a long prompt and a hard error).

  `mp-contracts` was considered as a shared home for the table and rejected — it
  is a generated-bindings crate and domain logic does not belong in it; a new
  crate for one table is the premature abstraction the ledger warns about. So:
  fallback + contract test, the established local pattern.

  9 tests. `tests/overflow_contract.rs` pins a **superset** relation (fallback ⊇
  authoritative, one-directional on purpose — the gateway may know an extra
  legacy wording) plus identical trailer spelling on both sides.
  Mutation-verified: adding a marker to inference-core alone fails with a message
  naming the deploy window it would break. Unit tests cover the expensive
  direction explicitly — throttling/quota bodies that mention tokens and limits
  must NOT classify as overflow — including exclusion-wins-over-marker
  precedence, and a property test that no exclusion is a substring of any marker
  (the `"refused"`/`"connection refused"` bug class from 4.1's retry table).

  *Not adopted:* pi's silent-overflow heuristics. Detecting a provider that
  truncates input instead of erroring needs prompt-token accounting compared
  against what we sent, which we do not have — inventing a heuristic without it
  would be guesswork on the false-positive-expensive side of this classifier.
- **4.3 Queued-input semantics** ✅ **DONE 2026-08-24.** Built as the third
  semantic the decision above describes, not as pi's two: the message is
  delivered at the tool-round boundary **as a pause**, and the model classifies
  it. pi makes the *caller* choose `steer` vs `followUp`, and the caller cannot
  know — whether a message redirects the work or merely follows it is a property
  of what it says.

  The premise was worse than "unspecified". The SPA's send path began
  `if (!content || state.status === 'streaming') return`: a message typed while
  the agent was working was **silently dropped**. Not queued, not refused, not
  shown as rejected. The user watched their words disappear and retyped them
  after the turn.

  - **`queued_input.rs`** (new): a per-`request_id` registry bound to the
    authenticated tenant/user, deliberately the same shape as `cancel_registry`
    (and carrying the same single-replica note — the SSE connection pins the run
    to one process). Bounded at 3 messages / 2 000 characters, counted in
    *characters* so a Norwegian message is not refused for its diacritics.
    `EnqueueOutcome` has five distinguishable arms because the SPA's correct
    response differs per arm — collapsing them to a bool is what made the
    original drop invisible. 10 tests.
  - **`QUEUED_INPUT_PAUSE`**: names both branches (redirect → adapt now and say
    what changed; follow-up → finish the current task, then address it) and
    forbids the two observed failure modes — dropping the message, and stalling
    to ask which kind it is. Pinned by a test.
  - **Delivery**: `run_tool_rounds` drains at the top of each round, splices the
    pause as its own `system` message with the user's words **unedited** in
    `user` messages (harness prose wrapped around them would put words in the
    user's mouth), and emits `ChatEvent::QueuedInput`. Ungated as a control
    event: the user typed it and is watching for it.
  - **`POST /v1/invoke/:request_id/queue`**: persists to the thread **first**,
    then queues. A reply to a message the transcript does not contain is a
    transcript that lies; at worst this records a message the run never read, and
    the response says so. The thread comes from the stream's own registration —
    a client-supplied thread id is only ever *checked* against it, and a
    mismatch is refused **before** the enqueue so a message that failed its
    authority check can never reach the model.
  - **BFF**: `inject_personal_thread_context` mints a fresh, content-bound
    `thread:append` decision from Control for this exact message, so a
    Space-scoped thread works with no special case and no reused token.
  - **One finisher for ten exits.** The stream task had ten `cancels.finish`
    sites; a leaked queue entry accepts a message no loop will ever drain, so
    all ten now go through `finish_stream_registrations`.
  - **SPA**: the guard hands the message to `deliverMidRun`; a `QueuedInputStrip`
    shows waiting / delivered / refused **in words**, not only by tint; and a
    run that ended before the message landed defers it and re-sends it as an
    ordinary turn from a single reactive effect — the 404 arrives before the SPA
    has processed the terminal event, so sending immediately would just hit the
    guard again. Cleared before sending, or flipping the status re-runs the
    effect over the same text.
  - **`mid-run-input.test.ts`** (new, 5 tests) pins the behaviour a compile
    cannot see: restoring the bare `return` type-checks perfectly and the bug is
    back, invisible. All three of its invariants mutation-verified.

  996 model-gateway tests · 428 gateway · 1 183 SPA tests, `tsc -b` clean, build
  succeeds, lint at the pre-existing baseline.
- **4.4 Compositional system prompt** ✅ **DONE 2026-08-22.** `AGENT_PREAMBLE`
  was one monolithic string that described `knowledge_search`'s JSON status
  protocol, shipment booking and social publishing **unconditionally** — on
  every run, including runs offering none of those tools. Two costs: tokens on
  rules that cannot apply, and the model told it can take actions this run has
  no tool for.

  That is the **inverse of the regression the old comment recorded** and fails
  the same way. The measured 2026-07-08 finding was that a prompt describing the
  toolset as "read-only fact-gathering" suppressed real tool use — models
  answered "I cannot post on your behalf" and drafted copy-paste text with a
  connected account available. The root cause was not the wording's timidity; it
  was that the prompt described a toolset that was not the one in front of the
  model. An unconditional action paragraph has the same defect pointing the
  other way.

  Now `compose_system_prompt(&ctx.allowlist)`: a tool-agnostic `PREAMBLE_CORE`
  plus snippets gated on the tools actually offered — read tools, action tools
  (the measured capability + approval wording, **verbatim**), knowledge_search's
  retrieval protocol, the memory tools, and delegation. Every phrasing preserved;
  what changed is when each appears. Adding a tool now means adding its snippet
  beside the tool instead of editing an unrelated paragraph.

  **Found in passing:** `save_memory`/`recall_memory` shipped with *no prompt
  guidance at all* — two tools and no account of when either is worth calling. A
  tool the model never reaches for is indistinguishable from one that does not
  exist, which is the second way that memory work was inert (the first being the
  missing capability binding, item 1.4). `SNIPPET_MEMORY_TOOLS` now says what
  belongs in memory and what does not, and not to narrate the calls.

  `ACTION_TOOL_NAMES` is deliberately its own list rather than derived from
  `permission::is_risky_call`: that classifier answers "does this *call* need a
  gate", a per-arguments question, while this answers "should the prompt describe
  action capability at all", a question about the offered set. Deriving one from
  the other would make the system prompt vary with tool arguments.

  6 tests: a read-only toolset is never told it can take actions; **every**
  entry in `ACTION_TOOL_NAMES` triggers the measured wording (so adding one
  cannot half-work); snippets track their tools including the bare-`subagent.`
  non-match; composition is deterministic and each snippet appears exactly once;
  and a regression guard that the **real** `offered_tool_defs()` set still
  carries the action, knowledge-search and memory guidance it had as one string —
  which a decomposition that quietly dropped a snippet would otherwise pass.
  453 execution-core tests green.
- **4.5 Session-as-tree / branching** — the one big pi idea deliberately
  parked (§13.5-4): append-only audit already true; branch/fork is a product
  feature awaiting the chat-design decision.
- **NOT adopting**: pi's no-permission/no-sandbox trust model, extensions-as-
  arbitrary-code, and its unimplemented AgentHarness (design reference only).

## WS-5 · DeepSeek observability UI + Verevon wiring (priority 5)

The tracking system the user called out ("see everything the model does —
tokens, thinking, process"). Backend emits nearly all of it already; the work
is frontend + the §0 rule from the parity doc.

- **5.1 SPA stream-client hardening** ✅ **DONE 2026-08-22.** All four parts
  landed in `src/shared/api/chat-client.ts` + `use-chat-controller.ts`:
  (a) `memory_recall` case → new `onMemoryRecall` handler, with a guard that
  drops a malformed `count: 0` rather than rendering "recalled 0 memories";
  (b) `stop_reason` carried through `onDone` (only non-`end_turn` values are
  recorded, so the ordinary case adds no noise);
  (c) a `default` arm → new `onUnknownEvent` hook that logs instead of
  dropping, still ignoring `STREAM_*` upstream envelopes — this is the fix for
  the §0 root cause in the parity doc, where the gateway relays verbatim but
  the SPA silently discarded anything it hadn't learned;
  (d) `stopped` split from `done` into `onStopped`, **falling back to `onDone`
  when a caller predates the handler** so existing callers keep working rather
  than going silent — a server-side stop no longer renders as a completed
  answer. `memory` added to `DEFAULT_FEATURES` so the backend actually emits
  the recall event.
  Rendered end-to-end, not just plumbed: `MemoryRecallNotice` ("Brukte N
  minner fra tidligere samtaler", neutral informational tone) and
  `TruncatedAnswerNotice` (warning tone — the reply *looked* finished and was
  not), both in `ChatMessages.tsx` beside the existing low-confidence notice,
  with matching `global.css` pill styles (no Tailwind in this app).
  6 new tests; 345 chat+api tests green; zero new lint problems.
- **5.2 Recall + truncation indicators** ✅ **DONE 2026-08-22.** Both
  indicators shipped with 5.1 and are rendered, not merely plumbed:
  `MemoryRecallNotice` (Sparkles glyph, Norwegian singular/plural) and
  `TruncatedAnswerNotice` (distinguishing `stream_incomplete` from the
  `max_tokens`/`length` token ceiling), in `ChatMessages.tsx`.

  The part that was **not** already covered was the plan's "honest *not
  reported*" clause, and chasing it found a real hole: of the five ways
  `fetch_chat_memory_context` can answer a turn **without** memory, one — the
  request-not-forwardable path — returned **completely silently**: no log, no
  metric. A turn answered with no memory because the credential could not be
  attached was indistinguishable from a turn with nothing to recall.

  Closed with `mp_gateway_chat_memory_prefetch_total{outcome}` over a closed
  label set (`hit`/`empty`/`timeout`/`error`/`no_credential`) plus a log on the
  silent path. `empty` and `timeout` are deliberately separate labels: "nothing
  to recall" and "could not look" are exactly the two the UI cannot tell apart,
  and conflating them is how a broken prefetch reads as a quiet product.

  **Why a metric rather than a banner** — recorded at the call site so it is not
  re-litigated: a timed-out prefetch is real degradation, but also ordinary
  jitter on a 400ms best-effort budget, and an inline "memory unavailable"
  banner on a slow turn is alarming and unactionable for whoever is reading the
  answer. The operator gets the rate; the reader gets a notice only when memory
  *was* used.

  2 tests, one asserting **every** exit from the prefetch records an outcome
  (structural, so a future sixth return path cannot slip through un-counted) and
  one that the five labels stay distinct. Mutation-verified: deleting a single
  `record_memory_prefetch_outcome` call fails both. 890 model-gateway tests
  green.
- **5.3 `ConversationNodeDefinition` + keyed renderer registry** ✅ **DONE
  2026-08-22.** New `src/shared/chat-nodes/`:
  - `types.ts` — `ConversationNode`, a 14-kind union of **data-only** nodes. No
    node carries a callback or a component; callbacks live in
    `ConversationNodeContext`, supplied once at render. That is what makes the
    derivation a pure function comparable with `toEqual` — the acceptance gate
    is not expressible against JSX.
  - `derive.ts` — `deriveConversationNodes(turn)`. `AssistantMessage` rendered a
    fixed sequence of fourteen nested `<Show>` blocks with inline conditions;
    every new backend capability (memory recall, truncation, reasoning) had
    arrived as one more `<Show>` in the middle of it. The conditions **and the
    order** are behaviour, and they are now one reviewable function.
  - `registry.ts` — `createConversationNodeRegistry`, typed as an **exhaustive
    record** so adding a node kind without a renderer fails the build. The
    runtime warn-once path is a backstop, and it warns rather than dropping: a
    missing renderer must not be indistinguishable from a turn that had no such
    content (same rule as `onUnknownEvent`).
  - `ChatMessages.tsx` now renders `<For each={nodes()}>` through the registry.
    `AnswerRegion` keeps thinking/error/content as ONE component because they
    share the streaming-class wrapper and were chosen by a nested
    `Show`/fallback — a structure that made rendering two at once impossible, and
    `AnswerState` being a discriminated union preserves that guarantee.

  **The acceptance gate paid for itself immediately.** Three-path equivalence
  (full thread load / transcript rehydrate / live append) **failed on first
  run** — and the bug was in my own 5.1–5.2 work: `turnsToTranscript` persisted
  neither `memoryRecallCount` nor `stopReason`, so both notices appeared live and
  **vanished on reload**. Losing `stopReason` is the serious one: an answer
  warned as possibly cut off silently became one that looks complete. A fourth
  route had the same hole — `mergeServerTurnsWithCachedMetadata` (thread
  refresh), where the server's persisted message carries neither field because
  both are derived from SSE events. Both fixed, both now tested.

  That is exactly the bug class the gate exists for: invisible in any single
  path's tests, and reachable only by asserting that reload, scroll-back, and
  watch-it-stream render the same thing. Asserting on the derived node list
  rather than on turn objects is deliberate — turns legitimately differ in
  transport-only fields (`lastFrameId`, `requestId`, `streaming`); what must
  match is what gets rendered.

  14 new tests (11 derivation incl. the gate and the nested-Show precedence that
  a flat condition list gets wrong, 3 registry). Frontend: **1099 tests / 149
  files green**, `tsc -b` clean, `pnpm build` succeeds, lint back to the
  pre-existing baseline. **Unblocks 5.4, 5.6 and 3.5.**
- **5.4 Tool presentation as recomputed intent** ✅ **DONE 2026-08-22.**
  `shared/chat-nodes/tool-presentation.ts` derives a `ToolIntent`
  (`terminal|search|read|generic`) plus the fields that intent makes meaningful
  — result count, truncation, tool-reported outcome — from the call itself, at
  render time.

  **Never persisted, and the reason is load-bearing:** if a card style were
  stored on the turn, changing this mapping would only affect new conversations
  and every existing transcript would keep rendering the old style forever.
  Recomputing means one function decides for all history with no migration.
  Asserted as purity (same call → same presentation, nothing written back).

  **`diff` ✅ DONE 2026-08-22 — with real producers, found rather than assumed.**
  The first pass left it out because no tool here produces a patch, which is
  still true. Two genuine producers exist anyway:

  1. **Tool output that already IS a patch**, detected from the **output, not the
     tool name**. Third-party MCP tools (a git server, a codemod runner) return
     unified diffs under names this client has never seen, so a name-keyed branch
     would have been both dead *and* useless for the tools that actually produce
     patches. Detection requires a real `@@ -a,b +c,d @@` hunk header —
     deliberately strict, because `+`/`-` line prefixes alone match prose,
     markdown lists and log output, and a wrong diff card is more confusing than
     a generic one.
  2. **Artifact revisions.** `ChatArtifact.history` already retains every
     version's full content client-side, so "what changed when I asked for a
     revision" needed **no backend work and no new contract** — the data was
     already there. The artifact viewer gained a `Vis endringer (+n −m)` toggle
     beside its existing version stepper.

  `shared/chat-nodes/diff.ts` computes the patch (line-level LCS, no dependency
  for ~60 lines of well-understood algorithm) and parses tool-supplied ones, so
  both producers render through one `DiffView`. It **degrades honestly above
  2,000 lines** — this runs during render, and an O(n·m) table over a large
  revision would freeze the tab — reporting a stat-only summary and *saying* it
  is truncated, because an empty hunk list otherwise reads as "no changes".
  Colour is never the only signal: every line carries its `+`/`-` sigil.

  27 tests across the producer, the intent detection and both surfaces —
  including trailing-newline-only changes (the most common spurious diff in
  generated documents), file headers not counted as changes, and a
  single-version artifact offering no toggle at all.

  **`truncated/total`, honestly scoped:** `truncated` is real and now surfaced
  (the read path reports it). `total` is **not** — the search envelopes carry
  `count`/`result_count` (the number *returned*), and there is no separate
  total, so inventing one would be a fabricated number next to real evidence.
  Both count spellings are read, because supporting one silently loses the
  number for half the tools.

  **Found in passing: tool calls were rendered nowhere at all.** `ToolCallCard`
  had **no caller**, and the "view steps" pill routed to `StepsPanel`, which
  shows task steps (title/detail/status) — a different type. So the evidence
  every step was built on (what was searched, what came back, what failed) was
  collected on every turn and unreachable in the UI. `StepsPanel` now takes the
  turn's `toolCalls` and renders them, which is what makes the dead component
  live and gives the intent cards a real host.

  9 tests including unparseable/truncated-JSON output (intent still classifies —
  it comes from the name), a search returning 0 rendering explicitly rather than
  being dropped as falsy, and the purity assertion. Frontend: **1111 tests / 151
  files green**, `tsc -b` clean, `pnpm build` succeeds, lint at the pre-existing
  baseline.
- **5.5 Process visibility** ✅ **DONE 2026-08-22** — with **three of five
  sub-items found already shipped**, one built, and one blocked on backend work
  that does not exist yet. Correcting the stale claims was most of the value.

  - *"The 4 unwired approval/pause events"* — **stale.** The run console handles
    **all 14** orchestration events the gateway maps (`plan_transitioned`,
    `todo_transitioned`, `approval_state_changed`, `subagent_attached/stopped`,
    `run_paused_for_approval`, `run_resumed_after_approval`, the five
    `browser_*` ones, `browser_action_decided`, `approval_continuation_verified`)
    plus `step_update`. Nothing was unwired — but nothing was stopping it from
    becoming unwired again either, so this is now **pinned**: a contract test in
    `run-console-client.test.ts` reads the event names straight out of
    `orchestration_event_to_sse` in the Rust source and fails if any lacks a
    `case`. Mutation-verified. An event with no case is parsed and dropped: the
    stream succeeds, the step never appears, and the backend — emitting
    correctly — is the wrong place to look.
  - *Per-run cost line* — **already shipped** (`AgentRunConsole.tsx:1410`).
  - *Run-error surfacing* — **already shipped** (`run.error` surfaced at :287).
  - *Plan view* — **the real gap, built.** `listPlans`/`listTodos` existed in the
    API client with **no caller anywhere in the app**, so the console reported
    `Plan: DRAFT → EXECUTING` in the timeline and could never show what the plan
    *was* or which todos it had — the transitions without the content. New
    `PlanPanel` reads both (independently, so a thread-less run's missing todos
    do not blank the plans) and re-reads on every plan/todo transition. Provider
    enum state names are rendered as-is rather than relabelled, absence is
    *stated* ("Ingen plan registrert for denne kjøringen") rather than left blank,
    and a missing summary says so — an empty panel reads as a loading failure.
    Semantic CSS in `global.css` (no Tailwind in this app). 4 tests incl.
    loading-vs-empty.
  - *Browser screenshots* — ✅ **DONE 2026-08-22 (corrected).** My first pass
    called this blocked "because there is no artifact-fetch route". **That was
    wrong: I looked in the wrong plane.** The artifacts live in **Quarry-v2**
    (Ingestion Plane), which has had the whole chain all along —
    `execution-core/src/quarry_agent.rs:468` maps Quarry's
    `screenshot_artifact_id` straight onto `screenshot_ref`, Quarry stores the
    bytes ZDR-gated and tenant-stamped (`put_artifact_if_allowed`), and
    quarry-edge serves them at `GET /v1/artifacts/{id}` with a tenant check whose
    own doc comment says *"possession of an id is not by itself authority to read
    the bytes"*.

    **The one real blocker was the content type.** `artifact_content_type()` was a
    stub that ignored its argument and returned `application/octet-stream` for
    everything, so alongside `nosniff` **no** artifact could render — a captured
    screenshot came back as an opaque download. Fixed at the source: the object
    key already encodes the kind (`.../screenshot.png`), so
    `content_type_for_key` derives an authoritative type and a new
    `ArtifactStore::get_with_key` surfaces the key every backend already resolved
    and then threw away. Accurate type **plus** `nosniff` is the safe pairing:
    the browser is told exactly what the producer stored and forbidden from
    guessing.

    Then a Verevon gateway relay (`GET /api/v1/chat/browser-artifacts/{id}`) and
    an `<img>` in the run-console timeline. Two security decisions worth naming:
    the relay mints a **Quarry-audience token for the user**, never a service
    credential (a service identity would make every tenant's artifacts readable
    to anyone holding an id — the CR-02 lesson); and it serves only an
    **allowlist** of renderable types, because quarry-edge now reports real types
    including `text/html`, and relaying that from the SPA's own origin would turn
    any captured page into stored XSS with the user's session attached. SVG is
    excluded on purpose and pinned by a test, since it is an image that can carry
    script. Everything else is downgraded to an opaque attachment: retrievable,
    never executable.

    Absent on a ZDR run by construction — Quarry declines to persist the artifact
    at all — so "no screenshot" renders as nothing, and a *failed load* is named
    separately, because only one of those is a problem.

  Frontend: **1088 tests / 148 files green**, `tsc -b` clean, no new lint
  problems (the 1 remaining error in `tests/e2e/space-shell.spec.ts` is
  pre-existing).
- **5.6 Keyed projection channel** ✅ **DONE 2026-08-22.** New
  `shared/projection/keyed-channel.ts`, and it found **two real defects** in the
  bug class it was aimed at.

  Most global writes in the chat controller were already guarded by a
  hand-written `ownsMachine()` check, with the reasoning documented at each site.
  Two were not covered:

  1. **`onTitle` had no guard at all.** The thread title is generated
     server-side *after* the first exchange, so it arrives late — switching
     threads while a first message was still working wrote **this thread's AI
     title onto the next one**, and `titleKind: 'generated'` locks it, so later
     snapshots would not correct it. A likely race, not a theoretical one.
  2. **A key check cannot see a superseded connection.** Neither send path
     aborts a prior `AbortController`, so sending a second message leaves **two
     live streams on the same thread** — and `ownsMachine()` returns true for
     both. The first stream's late `done` then flips the status to idle while the
     second is still answering.

  The channel guards on **key AND generation**, with a monotonic
  `streamGeneration` bumped per send, and counts/logs every rejection — a
  dropped write is normal, but an unexpected *rate* of them means a stream never
  learned it was superseded, and silent dropping is indistinguishable from a
  handler that never fired.

  **API note worth keeping:** this started as `commit(write)`, which reads
  better but makes every guarded site pass a closure that reads store state —
  three new `solid/reactivity` warnings, because the lint cannot know `commit`
  invokes synchronously. Rewritten as a single `accepts()` predicate so the
  guarded writes stay inline exactly as the hand-written checks were, with the
  counting done as part of the guard rather than needing a second call. One way
  to do it, and lint-clean.

  6 tests, including the same-key/older-generation case a thread-id check cannot
  catch, and key-loss taking precedence over generation when both apply.
  Frontend: **1117 tests / 152 files green**, `tsc -b` clean, build succeeds,
  lint at the pre-existing baseline.

  *Not adopted:* the "readiness / gap-repair" two-thirds of the named triad.
  Readiness is already covered — the buffer's `replay.found` plus the resume
  cursor from 5.7 — and gap-repair needs a sequence-hole detector on the client,
  which only becomes meaningful once `Last-Event-ID` resume has run in
  production long enough to show whether holes actually occur. Building a
  repairer for an unobserved failure mode would be speculative.
- **5.7 Gateway** ✅ **DONE 2026-08-22.** Both halves of parity-doc §4.

  **§4.2 no-allowlist contract test.** `apps/gateway/tests/sse_relay_no_allowlist.rs`
  pins the relay as byte-verbatim: upstream chunks yielded through untouched, no
  chat event name mentioned anywhere in the relay function, and the only
  synthesized frame is the transport error (reached from the upstream *error*
  arm, not from inspecting content). It derives the event list from
  model-gateway's own `ChatEvent::name()`, so a newly added backend event is
  covered automatically with no second list to remember. Mutation-verified with a
  planted allowlist. The first draft scanned the whole file and failed on
  `"error"` — which appears there as a JSON *envelope key* and inside the test
  module; **narrowing the scan to the relay function** was the right fix rather
  than allowing `"error"` through everywhere, which would have blinded the test
  to a real allowlist containing it.

  **§4.1 durable rich-event resume — the "one real stream gap", now closed
  across all three layers.**
  - **The buffer stored bare text.** `BufferedDelta { seq, delta }` →
    `BufferedEvent { seq, event, data }`: the SSE *frame*, which makes it general
    over every current and future event instead of a list to keep in step. Legacy
    records already sitting in Redis/Dragonfly under the 10-minute TTL still
    deserialize (`#[serde(default)]` + `is_legacy_text()`), and the resume handler
    rebuilds the exact `chunk` frame they used to produce — without that, a deploy
    would 500 every in-flight resume for a full TTL.
  - **Rich events carried no `id:` at all**, so a `Last-Event-ID` cursor could
    not even be positioned relative to them. One `emit_and_buffer` helper now
    assigns the shared monotonic seq, buffers the frame, and sends it; all **15**
    emission sites in the primary chat stream route through it, and `seq` was
    hoisted to the top of the task so text and rich frames share one sequence.
    Feature gating moved into the helper — and a *suppressed* event is not
    buffered, so a resume never replays an event family the client never opted
    into.
  - **Resume replayed everything as `chunk`.** It now replays each frame under
    its own event name.
  - **The SPA never sent a cursor.** New `onFrameId` handler + `ChatTurn.lastFrameId`,
    tracked on the **live** stream as well as on resume (the reconnect that needs
    the cursor happens after the live stream drops), and passed as
    `Last-Event-ID`.
  - **The interaction that would have silently broken it:** the resume path
    *cleared* the cached partial answer on its first replayed delta, correct only
    while replay always started from the beginning. With a cursor, replay is
    incremental — clearing would have deleted exactly the text the cursor told
    the server not to re-send, turning a lossless resume into a truncated one.
    Now conditional on having resumed *without* a cursor, read from a snapshot
    taken before the stream opens (the tracking handler overwrites the turn's
    value as soon as the first frame lands).

  Not adopted: `RichEventSink` existed but was used at **1 of 56** emission
  sites; rather than adopt an abstraction that had not earned its keep, the
  helper lives beside the stream it serves. Sites outside the primary chat
  stream (image-gen, deep-research) have their own streams and are untouched.

  4 new tests (legacy-record deserialization, any-event-name round trip
  including an invented future name, frame-shape tenancy assertions, SPA cursor
  tracking) plus an updated e2e that now reconstructs the answer the way a
  resuming client actually does. 896 model-gateway + 348 SPA tests green,
  `tsc -b` clean, no new clippy warnings.

## Sequencing

*The original forward plan below is kept as the record of how this was
sequenced; everything down to and including 3.1/3.2 has landed.*

**Now → next session:** 1.3 (CI selftest, hours) · 4.1 (length-stop tool
fail) · 5.1 (SPA stream hardening) · 2.1 (G7 publisher).
**Then:** 1.1 (durable subagents) · 2.2/2.3 (memory lifecycle) · 5.2/5.5.
**Then:** 5.3/5.4 (renderer registry — gate for 3.5) · 1.2 · 1.4 · 4.2/4.4 ·
3.1/3.2.
**Awaiting explicit product decisions (flagged, not stalled):** 3.4 workspace
design · 4.3 queued-input semantics · 4.5 branching · 2.4 promotion-gate UX ·
§7.5 reattachment scope. — **all answered 2026-08-22/24**; see *Product
decisions* above.

### Addendum — 2026-08-24, closing the two rows that still carried caveats

The parity matrix rows for post-compaction reattachment (187) and the durable
subagent lifecycle (189) were marked ✅ with caveats that made the mark
overstated. Both are now fully functional, and closing them found two real
defects rather than just missing surface.

**187 — reattachment covered conversation but not skills, and only one loop.**

- **Skills were the literal half of the row's own title** ("file/skill state")
  and had no recovery at all. The shared skill budget *degrades before dropping*
  and marks what it cut, which is honest and was a dead end: the model read
  `[… skill truncated …]` with no way to obtain the rest. Worst possible thing to
  leave half-read is an instruction — the model acts on half a rule believing it
  is whole. `mp_contracts::skill_recovery` + `reattach_skill` in **both** loops
  now closes it, with `disabled` reported as its own state (a rule the operator
  switched off must not be presented as current) and `not_found` suggesting only
  skills that could actually be read.
- **The governed agent loop had no compaction of any kind** — and it is the loop
  that accumulates the most payload, since every round's tool results are
  re-sent on every later round. An overflowing deployed-agent run simply ended
  on the graceful-failure sentence while chat degraded and recovered.
  `execution-core::compaction_budget` brings tier 1 across at an identical
  threshold, spared tail and notice text, and
  `tests/compaction_parity_contract.rs` (4 tests, all mutation-verified) pins
  them together — including that each loop's *detector* still matches what its
  own *formatter* writes, because a detector that has drifted compacts nothing
  and reports nothing wrong.
- `RunAgentResponse.compaction_triggered` now reports it. `StepOutcome`'s copy
  stays `false` and that is correct, not dead: one step accumulates no history.

**189 — a replayed delegation re-ran, and forked the ledger from the answer.**

`start_key` is identifiers-only so a re-driven parent step reuses its child run.
That is the right identity behaviour and it made re-running the nested loop
actively wrong: the child already carries an **immutable** terminal receipt, so a
second `RecordTerminalOutcome` returns the FIRST outcome. A retry that succeeded
handed the parent a good answer while the lineage permanently recorded the child
as failed — and this was reachable in-turn, since `subagent.*` is not risky and a
transient failure was retried.

- A replay now **resumes from the recorded answer and charges zero rounds**,
  with distinct refusals for "finished with nothing recorded" and "still in
  flight from an earlier attempt".
- A delegation is excluded from in-turn retry outright, in the retry classifier
  where "not safely replayable" belongs — the same place a write is excluded, for
  the same reason.
- *Not adopted:* DeepSeek's FIFO inbox. Our delegation is synchronous within a
  turn; an inbox is a different execution model, not a missing piece of this one.

Matrix rows 186, 187 and 189 rewritten to match. Verified: **2 280** Model Plane
Rust tests, Go bindings regenerated with no generator churn, five Go modules
build/vet clean, no new clippy warnings on touched files.

## Sequence — closed 2026-08-24

Nothing remains. The two open threads that outlive this plan are both recorded at
their items rather than here:

- **3.3's typed handoff report** — build it when a fresh-context *iterative* loop
  exists for its `status` to drive, or when the first piece of code (not prose)
  needs to branch on a delegation's outcome beyond success/failure.
- **4.5 branching** — decided as *wait*, pending a real need.

Two things want a staging pass before production trust, both flagged at their
items: registering managed child runs (1.1's lineage half, which changes run
counts, `RUN_COMPLETED` and GDPR purge counts) and the new `RecordRunOutput`
write.

Every item lands with the standing evidence bar: source + tests green
(`cargo test` workspace / `pnpm test` / `make selftest`), clippy-clean on
touched files, matrix row updated in `claude-hermes-deepseek.md` the same day.
