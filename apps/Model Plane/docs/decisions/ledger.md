# Design decision ledger

A running record of design tradeoffs this repo has actually settled —
proposed, then implemented, rejected, deferred, or archived — so the next
contributor can find a decision here instead of re-deriving it from grep'd
commit messages or, worse, re-proposing something already tried and
withdrawn.

This is the *lightweight* half of the postmortem practice
(`claude-hermes-deepseek.md` §7.6, adapting DeepSeek Harness's Agent Notes
ledger). Reserve `docs/postmortem/NNNN-*.md` for the deeper root-cause
write-ups this ledger's entries can point to; most decisions only need a row
here.

**States:** `proposed` (on the table, not yet acted on) · `implemented` ·
`deferred` (a real "not yet" with a stated trigger to revisit) · `rejected`
(measured and declined) · `archived` (superseded by a later decision).

**Adding an entry:** append, do not rewrite history — if a decision changes,
add a new entry that supersedes the old one and mark the old one `archived`
with a pointer forward. Newest first.

---

## 2026-08-22 — Sandbox-backend interface: defer a `SandboxBackend` trait

**State:** `rejected` (for now — same shape as the letta-bridge entry below;
revisit if a second real sandbox backend is ever planned)

A harness-adoption pass (`claude-hermes-deepseek.md` §7.1) proposed adopting
the pattern all three externally-audited systems converge on: a
`SandboxBackend { init_session, execute, cleanup }` trait + `SandboxHandle`
interface, so a future remote/SDK-only backend could satisfy the same seam a
real `bwrap`-wrapped process does today.

**Why declined:** `execution-core` has exactly ONE local backend
(bwrap-or-passthrough) today. `MpSandboxPolicy::External` already
anticipates a remote/Daytona-style backend, but hands it to a DIFFERENT
service (`sandbox-manager`, Go) — so even that doesn't call for a Rust-side
trait. Building the trait now, with nothing real to polymorph over, is the
identical mistake this repo already declined to make for memory adapters
(see the entry below) — a registry/interface for one implementation.

**What shipped instead:** investigating the "honest full/partial
enforcement" half of the same item (not the trait) surfaced a REAL,
previously unenforced gap — `MpNetworkPolicy::AllowDomains` could report
`sandboxed: true` while granting fully unrestricted network egress, because
nothing checked whether an egress proxy was actually configured. That was
fixed (`executor.rs`'s `require_requested_isolation`), which is the concrete
value this item's investigation actually produced.

**Revisit when:** a second real sandbox backend (remote/SDK-only) is
actually being built, per the same trigger as the memory-adapter entry.

---

## 2026-05-30 — Memory-adapter registry: defer until a 2nd backend exists

**State:** `deferred`
**Source:** `docs/capability-ownership-matrix.md` §4.4

A multi-adapter registry + `MemoryAdapter` interface (hermes `MemoryProvider`
shape) is the right shape once there are ≥2 real memory backends to swap
between. Today `letta-bridge` is a single in-memory *stub* with no real
Letta upstream — building a registry + interface to hold one stub is a
premature abstraction (over-engineered, not small).

**Ruling:** `session-core`'s `agent_memory` stays the canonical memory
index; `letta-bridge` stays as-is. Introduce the `MemoryAdapter` registry
when the second adapter lands, or when a real Letta upstream is wired —
whichever comes first.

**Revisit when:** a second real memory backend is actually being built.

---

## 2026-08-14 — HARN-1/2: unify the two tool-dispatch loops

**State:** `rejected`, on measurement
**Full write-up:** `docs/postmortem/0001-harn-1-2-tool-dispatch-unification.md`

Proposed collapsing `execution-core::execute_step_inner` and
`model-gateway::dispatch_tool` into one module, and unifying `run_rounds`/
`run_tool_rounds` behind one `run_turn` trait. Measured against the actual
code once the tree went quiet enough to attempt it: the two dispatchers
share zero tools (one is a read-only refusal-first router, the other is the
gated execution pipeline — the refusal boundary IS the authority boundary),
and the two loop signatures fit only their own caller (fresh history for
subagent isolation vs. a continued thread). What survived: the ADR
documenting why, plus `cross_service_loop_contract.rs` fixing the REAL
underlying issue (three cross-service invariants enforced only by comment,
one of which had already drifted in production).

---

## 2026-08-22 — Semantic-memory erasure: one confirmation standard, not two

**State:** `implemented`

Memory is dual-written (durable `agent_memory` row + a letta-bridge semantic
copy keyed by the same `memory_id`). The per-memory `DeleteMemory` erased both;
the two BULK paths — `grpc::delete_thread_rows` and
`gdpr::purge_organization_data` — were pure SQL and left the semantic copies
resident, with no id left in Postgres to find them by.

Fixed in `session-core/src/memory_erasure.rs`. The design question was what
counts as erased. The draft had three buckets — `confirmed`, `absent`
(the tier replied "no such record"), `unconfirmed` — with only the last
breaking completeness.

**Ruling: two buckets, matching `delete_space_threads`.** That path already
erased semantic twins durably and had already answered this: a `deleted = false`
reply cannot distinguish an idempotent prior delete from an unknown or
mismatched record, so `absent` was an assumption dressed as an answer. Erased
means `deleted == true` **and** no degradation; everything else, including an
unconfigured tier, is `unconfirmed`. Two confirmation standards for one
operation inside one service is exactly the drift this ledger exists to prevent.

**Also settled:** propagation runs strictly *after* commit — a degraded vector
store must never roll back a delete Postgres has already honoured — and
`PurgeSummary::semantic_memory` is deliberately excluded from `total()`, which
counts Postgres rows. `erasure_is_complete()` is the question a DSAR response
should ask instead.

**Deferred (not rejected):** a durable retry queue for the bulk paths.
`delete_space_threads` has one because it has a `deletion_request_id` to key
receipts by; `OrganizationErasure` carries only `org_id` + `requested_by`, so
extending the ledger there needs a contract change. Until then incompleteness is
counted and logged (`mp_session_semantic_erasure_unconfirmed_total`) rather than
silent — which was the actual defect.

**Revisit when:** the erasure event contract gains a request id, or the semantic
tier is promoted out of `DEGRADED_SEMANTIC_UNVERIFIED` (plan item 2.3).

---

## 2026-08-22 — Skill guidance gets a size budget, not just a count cap

**State:** `implemented`

Skill injection was capped at 3 skills and unbounded in bytes. Skill bodies are
operator-authored free text, so a few long skills could take more of the prompt
than the conversation they exist to steer — with no failure, just less room and
worse answers.

**Ruling:** an 8,000-character budget (~1% of a 200k window) in **both** dispatch
loops, spent in relevance order, **degrading before dropping**. A truncated block
carries an explicit marker and anything that cannot keep 240 characters is
dropped instead of reduced to a stub. Drops are counted and logged.

**Why a character budget, not a token one:** the model — and therefore the real
window — is resolved downstream by inference-core's intent layer. Any token
figure computed at the injection site would be a guess presented as a
measurement. Characters are what can actually be counted there.

**Why the marker is non-negotiable:** a silently truncated *instruction* is worse
than a dropped one. The model acts on half a rule believing it is whole, and
neither the operator nor the reader can tell.

Duplicated across the two loops (they are separate crates by design) and pinned
by `model-gateway/tests/skill_budget_contract.rs`, which asserts both the budget
and the marker text — drift would mean chat and deployed agents disagree about
how much prompt skills may take, surfacing only as "the agent answers worse than
chat".

---

## 2026-08-22 — Context inspector is a read-only view, with no delegated Data Plane credential

**State:** `implemented`

`GetContextAssembly` returns the itemized context window and model-gateway
already called it to build prompts; it was never exposed, so nothing could answer
"why did it answer from *that*?".

**Ruling:** exposed as `GET /v1/threads/:id/context`, and deliberately **without**
delegating the caller's Data Plane bearer — unlike the prompt-build path, which
must. An inspector request is a diagnostic read; forwarding a retrieval
credential would let opening a panel trigger live grounding fan-out and bill for
it. Grounding already assembled into the thread is still reported.

**Also settled:** the client fetches only while the panel's tab is open, and the
assembler's own `estimated_tokens` total is reported rather than summed from the
segments — if the two disagree, an inspector must show what the assembler
believes, not a number the route computed.

**Placement:** a collapsible section in the existing Steps tab rather than a
fifth chat tab. Adding a top-level tab is an information-architecture decision;
Steps is already the process-visibility surface.

---

## 2026-08-22 — Browser screenshots come from Quarry-v2, and artifacts get an authoritative content type

**State:** `implemented`

`BrowserObservationReceived.screenshot_ref` was thought unresolvable. It is not:
`execution-core/src/quarry_agent.rs` maps Quarry-v2's `screenshot_artifact_id`
straight onto it, Quarry stores the bytes (ZDR-gated, tenant-stamped) and
quarry-edge already served them at `GET /v1/artifacts/{id}`. The gap was looked
for in the Model Plane; the artifacts live in the Ingestion Plane.

**The actual blocker was the content type.** quarry-edge's
`artifact_content_type()` ignored its argument and returned
`application/octet-stream` for every artifact, which alongside `nosniff` meant
nothing could be rendered — a captured screenshot was an opaque download.

**Ruling:** derive the type from the object key, which already encodes the kind
(`quarry_core::artifact::object_key` → `.../screenshot.png`). A new
`ArtifactStore::get_with_key` surfaces the key each backend already resolved on
the way to the bytes. An accurate type **with** `nosniff` is the safe pairing —
the producer chose the kind server-side, so the declaration is a fact, and the
browser is still forbidden from guessing. Unknown keys keep the opaque default.

**Two boundary rulings on the Verevon relay:**

1. **User credential, never a service token.** The relay mints a Quarry-audience
   token for the end user; Quarry authorizes from the token's own org claim. A
   service identity would make every tenant's artifacts readable to anyone
   holding an id — the same trap as the rejected "service token for MCP" fix.
2. **Renderable types are allowlisted, not passed through.** Now that
   quarry-edge reports real types, relaying `text/html` from the SPA's origin
   would turn any captured page into stored XSS with the user's session attached.
   Only `image/png|jpeg|webp` are served inline; **`image/svg+xml` is excluded on
   purpose** (an image that can carry script) and pinned by a test. Everything
   else becomes an opaque attachment — retrievable, never executable.

---

## 2026-08-22 — The `diff` tool intent is detected from output, not from a tool name

**State:** `implemented`

The reference intent set includes `diff`, and it was first rejected because no
tool this platform ships produces a patch — no file-editing tool, and no tool
result carrying before/after state. That remains true.

**Ruling: detect it from the OUTPUT.** Third-party MCP tools (a git server, a
codemod runner) return unified diffs under names this client has never
registered, so a name-keyed branch would have been dead code *and* useless for
the tools that actually produce patches. Detection requires a real
`@@ -a,b +c,d @@` hunk header — `+`/`-` prefixes alone match prose, markdown
lists and log output, and a wrong diff card is worse than a generic one.

**Second producer, already in hand:** `ChatArtifact.history` retains every
revision's full content client-side, so an artifact revision diff needed no
backend work and no contract change. Both producers render through one
`DiffView`; two renderers for one concept would drift.

**Also settled:** the diff degrades above 2,000 lines to a stat-only summary and
*declares* that it did. This computes during render, an O(n·m) LCS over a large
revision would freeze the tab, and an empty hunk list would otherwise read as
"no changes".

---

## 2026-08-22 — The browser egress proof belongs in quarry-runtime, and it was hiding ~1000 tests

**State:** `implemented`

`quarry-browser`'s `blocks_loopback_subresources_before_they_reach_the_server`
failed on **every** machine with `"chromium browser navigation requires a pinned
egress proxy"`. It could never have passed there:

- `require_configured_egress` needs a real `BrowserEgressProxyProvider`; the only
  real one is `quarry_runtime::PinnedBrowserEgressProxy`, and quarry-runtime
  depends on quarry-browser — importing it back would be circular.
- It also lacked the `CHROMIUMOXIDE_TEST=1` guard its sibling browser tests
  carry, so it ran unconditionally and failed with no browser installed too.

**It was worse than one red test.** `cargo test` stops at a failing test binary
without `--no-fail-fast`, so this one failure aborted the run before the rest of
the workspace executed: the suite reported **81 passing**, and with the test fixed
it reports **1,104 across 31 binaries**. A permanently-failing test in an
early-ordered crate silently un-ran most of the workspace.

**Ruling:** the live proof lives in
`quarry-runtime/tests/browser_egress_boundary.rs`, the one crate where both
halves of the boundary exist, and it uses only the public `BrowserDriver` trait —
nothing was widened for a test. It is now two tests, because the boundary has two
independent layers and the old one only ever touched the outer:

1. **Live Chromium refusing a loopback navigation** behind the real pinned proxy
   (gated on `CHROMIUMOXIDE_TEST=1`). Its positive control is reading the
   `about:blank` document back over CDP plus asserting
   `capabilities().isolated_egress`, so a browser that never started cannot be
   mistaken for a refusal.
2. **A real TCP request through the pinned proxy** to a live loopback listener,
   speaking absolute-form HTTP as a proxied browser does. This is the transport
   half — the one CDP interception is only defence in depth for, since Chromium
   would otherwise re-resolve an approved hostname when it opens a socket. No
   browser needed, so it runs everywhere.

**Both carry a positive control, deliberately.** The assertion is "the request log
is empty", which is also what a broken witness produces. One test contacts the
listener directly and requires that it *does* record, so emptiness means
"refused" rather than "not watching".

The page-initiated subresource case could not survive the move — it needed the
private `current_page`/`set_content` — but no coverage was lost: the decision it
exercised is unit-tested next to where it lives (`navigation::tests` for
`guard_page_request_target` against loopback and RFC1918). A marker test in
quarry-browser fails if the live proof file disappears, since moved coverage is
easy to lose track of.

## 2026-08-24 — Mid-run user input arrives as a pause, not an interruption

**Decision.** A message the user sends while a run is streaming is delivered at
the run's next **tool-round boundary**, accompanied by an instruction to classify
it: if it changes what the agent should be doing, adapt now and say what changed;
if it is a follow-up to the current task, finish that task and address it before
ending the turn.

**Why not the two modes pi offers.** pi makes the caller choose `steer` (inject
after the current tool round) or `followUp` (wait for a natural stop). The caller
cannot know which applies — whether a message redirects the work or merely
follows it is a property of what it says, readable only by the model that reads
it. Making the *model* classify is a third semantic, and the one that matches how
people actually interject.

**Why the boundary.** It is the only point in a run where the loop is between
actions rather than mid-call, so delivery never cuts a tool off.

**What was actually broken.** The SPA discarded mid-run input silently
(`if (!content || state.status === 'streaming') return`) — not queued, not
refused, not shown as rejected. Every refusal path now states its own reason,
because the SPA's correct response differs per reason and a single boolean is
what made the original loss invisible.

**Persist before deliver.** The message is written to the thread when it is
accepted, before it is queued. A reply to a message the transcript does not
contain is a transcript that lies. At worst this records a message the run never
read, and the response reports exactly that.

**Authority.** The append lands on the thread the *stream* registered, never on a
thread the client names; a client-supplied thread id is only checked against it,
and a mismatch is refused before the enqueue. Space-scoped threads get a fresh,
content-bound `thread:append` decision from Control for the exact message, the
same rule the ordinary send path follows.

## 2026-08-24 — Compaction is recoverable, and recovery is need-driven

**Decision.** Compaction edits the **prompt**, never the durable thread. So the
model may read compacted-away messages back with `reattach_context` when it finds
it is missing something — rather than the gateway pre-attaching a budgeted set of
"recently touched" state on every compacted turn.

**Both compaction outcomes say so.** The dropped-history notice and the summary
prefix each tell the model recovery is possible. The summarised path is the
common case; dropping is the summariser-outage fallback, and a summary losing a
detail is the likelier reason the original text is needed.

**An unauthenticated read is an error, never an empty result.** session-core
rejects an unauthenticated `ListConversation`, and a rejection reduced to "no
messages" is indistinguishable from a genuinely empty history — the model would
tell the user their earlier message never existed.

**Substring, not fuzzy.** A recovery that returns loosely-related turns is worse
than one that returns nothing, because the model cannot tell which it got.

## 2026-08-24 — Memory provenance has three states, and the third must show

**Decision.** Recalled memories are shown to the user with their provenance:
`stated`, `inferred`, or `unrecorded`. The roadmap called for a `stated|inferred`
badge — a boolean — but the proto has three values and its own comment says the
third must never be rendered as stated. A two-state badge would have had to pick
a side for every pre-provenance row, and picking `stated` manufactures consent
the record does not support.

**Projected from the durable record alone.** No client-side table of known topics
or producers, so a new topic stays identifiable without a client release, and an
unreadable value degrades to `unrecorded` rather than being dropped or guessed.
The label is the entry's own topic, never its id — an id is not a name.

**Shown, not hidden.** Displaying what we already hold about a user is
transparency, not exposure; it is also the only way a stale remembered fact
becomes visible enough to correct. Only `stated` gets an affirmative tint: a
confident colour on a guess is exactly the wrong signal.

## 2026-08-24 — A subagent's conclusion lives on the child run, and the parent needs consent to read it

**Decision.** A delegation's answer is persisted on the **child run's own
record** (`runs.final_output`, via the new `RecordRunOutput`), never on the
parent's transcript. A resumed parent learns *that* its child finished; it learns
*what* it concluded only through an approval-gated read.

**Why not the parent's transcript.** It is the obvious place and it collapses the
boundary: the conclusion would be in context on every later turn with nobody
asked, and a zero-retention parent would end up carrying content from a run whose
retention scope is its own.

**Why a new RPC and not a field on the terminal receipt.**
`RecordTerminalOutcome` is metadata-only *by design* — that is exactly what makes
it safe to call on every run, including a ZDR one. Adding content to it would
trade a safe universal receipt for a conditional one. The content write is its
own call, refused outright for a zero-retention caller, and separately auditable.

**Two tools, because the split is the boundary.** `list_subagent_results` is
content-free and ungated, so a resumed run can orient itself without interrupting
anyone. `read_subagent_result` returns the answer and is gated on **every**
posture.

**Why `requires_consent_to_disclose` and not `is_risky_tool`.** The risk-based
gate is a no-op precisely where it matters: ordinary chat runs use the `auto`
posture, where nothing is gated on risk. Reading genuinely is not risky — the
approval is about **disclosure**. A test asserts the tool must not be classed
risky, so the predicate cannot quietly become redundant.

**Scope is verified server-side.** A run id in a tool argument is untrusted; the
read is confined to direct children of the calling run via
`RunDetail.parent_run_id`, with one refusal text for both "not yours" and "does
not exist" so the tool is not an existence oracle.

**Two labels corrected on the way.** Every approval reported itself as
`APPROVAL_KIND_DESTRUCTIVE` with the reason "tool 'X' requires approval".
Describing a disclosure as destructive is how an approval prompt stops carrying
information, so a consent pause is now `APPROVAL_KIND_PERMISSION` with a reason
naming what would be disclosed and where it would go. Separately, the
capability-seed contract test scanned every `*.sql`, so an id appearing only in a
`*.down.sql` DELETE read as seeded; it now scans `*.up.sql` only.

**What was already there.** `runs.final_output` has existed since
`0001_init.sql` and `GetRun` always returned it. Nothing ever wrote it. The
column was not the gap — the write was.

## 2026-08-24 — The autonomy ladder: a graded grant, per call, with a stated reason

**Decision.** A run's authority is a strictly ordered ladder —
`read_only | workspace_write | danger_full_access` — and approving a plan is what
grants a rung. The grant must name the rung and carry a substantive
justification; it is enforced **per call at execution**, with the call's actual
arguments.

**Why not in a tool schema.** A schema is registry-global while the effective
rung is per-call truth: `execute_provider_action` needs `read_only` for
`pages.list` and `danger_full_access` for `pages.post`, and only the arguments
say which. A contract test pins the gate out of the tool catalogue for this
reason.

**Why the rung requirement reuses the existing risk classifiers.** A second table
would let a call be "risky" to the posture gate and "read-only" to the ladder,
and that disagreement is a bypass rather than an inconsistency.

**Why `UNSPECIFIED` means two different things, deliberately.** In
`mp_contracts::autonomy::permits` it ranks with the narrowest rung, so an unset
field is never read as a grant. In the loop's gate it means *no graded constraint
stated*, so a caller that predates the ladder behaves exactly as before. Wiring
one to the other would either turn an unset field into a grant or refuse every
write on every existing run. Both are pinned by tests.

**Why the justification has a minimum length.** A required field that accepts
"ok" is required in name only, and a person reads it before deciding. An
oversized one is **refused, not truncated** — a truncated reason reads as a
complete one.

**Where it is validated: three layers, on purpose.** The HTTP edge (to give a
person a usable message), the coordinator (the rule), and session-core (because a
server that trusts its caller to have checked has no rule at all).

**What was already there.** `ExitPlanMode` was served over gRPC with **no
production caller**, and `ApprovalKind::Plan` was only ever mapped in conversion
helpers, never produced. Plan mode could be entered and never left. The control
was not missing a decision — it was missing a way to be used.

## 2026-08-24 — Fork semantics for subagents: REJECTED, as the reference rejected it

**Decision.** Do not build a fork-semantics subagent that inherits the parent's
history. `deepseek-harness` (MIT) considered and discarded it: "inherited
completed turns are implicit, growing handoff state and violate the fresh-context
contract". Its fresh-agent contract is a distinct session with no seed, sharing
only the working tree, so the tree is the durable authority.

**What we already have.** `run_subagent` builds a fresh `LoopContext` with no
parent transcript, and the isolation is asserted in both directions by an
existing test — the child's history is "system preamble + the delegated goal,
nothing inherited", and the child's intermediate tool traffic stays out of the
parent's context.

**The typed handoff report is deferred, not skipped.** DeepSeek's
`{status, summary, evidence, nextSteps, blocker}` exists because its loop is
iterative with a fresh context per round: the report is the only state carried
across rounds and its `status` drives the loop. Our delegation runs a child once
to completion, nothing branches on a finer status than success/failure, and
building the struct now is a type nothing reads. **Revisit when** an iterative
fresh-context loop exists for the status to drive, or when the first piece of
code — not prose — needs to branch on a delegation's outcome.

## 2026-08-24 — Per-run child ceiling instead of worktree isolation

**Decision.** Bound a run's delegation *breadth* with an independent per-run
ceiling (`subagent::MAX_TOTAL_CHILDREN = 8`), kept deliberately separate from the
round budget. Do not port git-worktree-style per-agent filesystem isolation.

**Why not worktrees.** The reference does not isolate per-agent filesystems for
its fresh-agent loop at all — children share the parent's cwd deliberately,
because the working tree is the durable authority. What it isolates is context
(which `run_subagent` already does) and what it adds against runaway concurrency
is exactly this ceiling.

**A premise the first test corrected.** The runaway was assumed to be fan-out
*within* a round. It is not possible: the first `spawn` claims the whole
remaining pool with `swap(0)`, so every sibling that round is refused for lack of
budget. The real runaway is *across* rounds — one delegation per round, for as
many rounds as the budget allows, which with a generous `max_rounds` is dozens of
durable child runs, lineage rows and managed obligations.

**Two ordering details carry the correctness.** The child slot is claimed
*before* the round pool, or a fan-out refusal would report "no budget left" and
strand the pool; and a refused claim is not returned, or unbounded refused
attempts could retry into the same slot. The refusal names the ceiling and is
asserted not to mention the round budget — a breadth refusal that blames budget
sends the model to re-plan the wrong constraint.

## 2026-08-24 — A replayed delegation resumes; it never re-runs

**Decision.** When `StartManagedRun` reports `already_started` for a delegation,
the recorded outcome is the answer. Resume from `runs.final_output`, or refuse
with the reason — never re-enter the nested loop.

**Why re-running is wrong, not merely wasteful.** `start_key` is
`<parent_run_id>:<parent_step_id>` — identifiers only, so a re-driven parent step
reuses its child run instead of forking the lineage. That means the child already
carries an **immutable** terminal receipt, and a second `RecordTerminalOutcome`
returns the FIRST outcome. A replay that re-ran and succeeded would hand the
parent a good answer while the ledger permanently recorded the child as failed.
Two truths, and the ledger is the one people audit.

**This was reachable in-turn.** `subagent.*` is not risky, so a delegation whose
failure looked transient was retried by `runtime_loop::retry` — producing exactly
that disagreement. Delegations are now excluded from retry in the classifier,
where "not safely replayable" already lives for writes.

**Refusals are distinguished.** "Finished with nothing recorded" and "still in
flight from an earlier attempt" call for different next moves from the model, so
they are different messages.

**Not adopted:** DeepSeek's FIFO inbox for subagents. Our delegation is
synchronous within a turn; an inbox is a different execution model, not a missing
piece of this one.

## 2026-08-24 — Compaction and its recoveries belong to both loops

**Decision.** Tier-1 compaction (clear stale tool-result payloads) runs in the
governed agent loop as well as the chat loop, at an identical threshold, spared
tail and notice; and both loops offer both recoveries — `reattach_context` for
conversation and `reattach_skill` for a truncated instruction.

**What was actually broken.** The agent loop had **no compaction at all**, and it
is the loop that accumulates the most payload: every round's tool results are
re-sent on every later round. An overflowing deployed-agent run ended on the
graceful-failure sentence while chat degraded and recovered. Separately, the
shared skill budget truncated instructions and marked them — honestly, and with
no way to read the rest. A notice that says "this is incomplete" without a route
to completeness is the same defect the dropped-history notice had before it
learned to name `reattach_context`.

**Two implementations, pinned rather than shared.** The services deploy
separately and neither may depend on the other — the same situation as
`tool_retry_contract.rs` and `skill_budget_contract.rs`, and the same answer:
implement in both, keep the constants and notice identical, and read the other
service's source in a contract test. `compaction_parity_contract.rs` also asserts
each loop's *detector* still matches what its own *formatter* writes, because a
detector that has drifted from its formatter compacts nothing and reports nothing
wrong.

**Tiers 2–3 stay chat-only by design.** They compact a durable thread
transcript; an agentic turn does not have one. Extending them there would be
inventing a transcript to summarise.

**`disabled` is not `not_found`.** A skill the operator switched off must be
reported as switched off — presenting it as missing invites the model to proceed
as if no rule existed, and presenting it as current lets a retired rule keep
steering answers.

## 2026-08-25 — Needle 2 / Cactus: ideas yes, engine no (and one real gap it exposed)

**Decision.** Record `cactus-compute/needle` in the harvest doc as an **ideas-only**
source, and do not adopt the model or its engine into any plane. Port the two
ideas that are genuinely better than ours through the provider we already use.

**CORRECTED same day — the conclusion changed.** The first version of this entry
said needle was a thin client with no engine source and put it in daytona's
ideas-only tier. That came from reading the README's documented API rather than
the repo, and it was wrong.

The repo has **two** inference paths. The documented product API
(`needle.Needle(...).run()`) does load the paid `cactus` engine over `ctypes` — so
that path is unshippable, as originally stated. But `needle/model/` also holds a
**complete, self-contained Apache-2.0 JAX path**: 630 LoC architecture, 405 LoC
KV-cached decode, 226 LoC generate, with no `ctypes` or engine reference. And the
weights (`Cactus-Compute/needle2` on Hugging Face) are **apache-2.0 and ungated**.

So the paid artefact is the *optimized ARM/mobile runtime* — a 14MB binary in 28MB
of RAM on a phone. That is Cactus's moat and it solves a problem we do not have.
**needle is adoptable; `cactus` is not, and is not needed.** One feature does stay
behind the moat: the byte-level grammar decoder is not in the Python at all (zero
hits across 4 418 LoC), so the technique is portable but their implementation is
not. `cactus-hybrid` (MIT) remains portable outright.

**Where they are right, and it costs us nothing to fix.** Needle compiles a
byte-level grammar from the declared schemas and constrains every decoded token.
We send Azure OpenAI function objects with **no `strict: true`**, so nothing
constrains tool-argument decoding on any path; and `argument_repair` is
`mcp_call`-scoped, fail-open, missing-required-and-enum only. The principle ports
through Azure OpenAI's strict function schemas — same guarantee, no new engine.
Also found: an unparseable `parameters_json` silently degrades to an empty
schema, so the model gets no constraint and nothing reports it.

**The gap it exposed that we should have found ourselves.** We capped skills at
8 000 chars on the argument that a few long skills could take more of the prompt
than the conversation they steer — and left the tool catalogue unbounded at
~15 500 chars (chat) / ~16 300 (agent), before client and MCP tools. Twice the
budget, same argument, never applied.

**But their mechanism does not port.** Needle renders only the top five tools per
turn. Our `offered_tool_defs()` IS the purpose-lock allowlist, so omitting a tool
forbids it rather than hiding it. The safe adaptation is description/schema
budgeting with degrade-before-drop while the allowlist stays complete — not
catalogue selection. Selection would require decoupling the allowlist from the
rendered set first.

**Confidence: right direction, right order.** Theirs is a learned calibrated
head. Ours is an explainable heuristic (hedging-phrase substrings, evidence
volume) that gates exactly one behaviour — follow-up chips. That is not an
oversight: escalating on an uncalibrated heuristic gives users unpredictable
behaviour. Calibrate on provider logprobs first, then consider gating.

**Not applicable:** their KV-sink sliding window is a property of owning the
decode loop, which we do not.

**Asked two ways, answered once.** "Clone `cactus` and change its architecture"
and "build a parity system from their research" were both proposed.

- **Cloning/modifying `cactus`: no.** Its grant of "use, copy, modify, merge" is
  *conditioned* on falling inside its §2 categories; outside them there is no
  grant at all, so there is nothing to modify under. A modified copy is still a
  copy, and a rewrite done while reading the source is a derivative work.
  Modifying is the same licence problem, not an escape from it — legal sign-off
  standard, same as AGPL, and the honest advice is not to start. It is also ARM
  kernel work for a constraint (phone RAM) that is not ours.
- **A parity system: unnecessary.** The architecture, decode path, quantizer,
  LoRA pipeline and weights are already Apache-2.0. Nothing needs re-deriving from
  the paper. What would be built is a *serving path for a model we already hold* —
  smaller and better defined than a reimplementation.

**Sequencing, so this cannot become a runtime project by drift:** Phase 0 is an
eval-lab spike running the Apache-2.0 JAX path against our real tool catalogue and
a held-out set from our own traces, measured against the frontier model we route
to today. Phase 1 — a Rust forward pass in inference-core plus an OSS grammar
layer, exposed as one more provider behind the existing routing policy — happens
**only if Phase 0 holds**. A self-hosted model is not a Foundry bypass in the
sense the 2026-08-19 audit polices (that was content reaching unapproved
providers; self-hosting is strictly stronger on residency), but inference-core
must remain the canonical owner. Their data-synthesis pipeline calls OpenRouter,
which is a residency decision rather than a config flag.

## 2026-08-25 — Needle Phase 0 ran; Phase 1 cancelled

**Decision.** Do not build a Needle serving path. Phase 0 measured it against our
real `offered_tool_defs()` catalogue and it fails on more than one axis.

**The hard blocker is architectural, not tunable.** `max_seq_len` is 2 048 tokens.
Our catalogue serialises to **4 698** tokens for the agent loop (2.3× over) and
3 964 for chat (1.9× over); only 11 of 25 tools fit. That also explains their
tool-retrieval head from the other direction — at 2 048 tokens retrieval is
*structurally required*. For us it collides with the purpose-lock, where an
un-offered tool is forbidden rather than hidden.

**Accuracy on the 8-tool subset that does fit: 47.1 %** (EN 52.9 / NO 41.2),
schema-valid-when-called 76 %, abstention 33 %. Eight tools hand-picked to be
maximally distinguishable, so that is a best case.

**Four failure modes weigh more than the number:**

1. `knowledge_search` **0/4** — it chose `web_search` every time for "search *our
   uploaded documents*". That is the grounding boundary the product rests on.
2. It **hallucinated `lookup_org_chart`**, a tool not in the catalogue. Their
   byte-level grammar would prevent this; the grammar is the one part that lives
   only in the paid engine. The path we may legally run is structurally worse than
   the one we may not.
3. **Abstention degraded as the budget grew** (83 % → 33 %): given room it invents
   calls for arithmetic and poetry.
4. Norwegian costs twice — 1.3–2.3× more tokens *and* ~12 pp less accuracy.

**A correction to an earlier entry.** The prior ledger entry called their
calibrated confidence head "better in kind" than our heuristic. Phase 0 weakens
that: mean logprob separated right from wrong by only **+0.107**. Logprobs are
still the right direction for `confidence.rs`, but Needle is not evidence the gate
will be strong.

**Method honesty.** The 34 queries are authored, not sampled from production
traces (session-core has them; this ran offline). And the first run reported 35.3 %
purely because a 64-token budget truncated calls mid-JSON — the model emits a
`<think>` block first, which is part of its trained format. Corrected to 224
tokens with an explicit truncation flag. No frontier baseline was run: it cannot
change a conclusion already settled by the context limit, and would spend real
credit to confirm it.

**What survives, and both were readable from the source without any of this:** the
tool-catalogue budget gap is ours and real (~16 k unbounded chars against an 8 k
skill cap justified by the same argument), and grammar-constrained decoding
matters — `lookup_org_chart` is exactly what an unconstrained decoder produces,
which strengthens the `strict: true` item rather than replacing it.

## 2026-08-25 — `strict: true` on function tools: deferred, it disables parallel tool calls

**Decision.** Do **not** enable Azure OpenAI structured outputs (`strict: true`)
on function tools. Defer to a product decision, because it is a trade rather than
a fix.

**The blocking constraint, from Microsoft's docs (three language variants agree):**
"Structured outputs are not supported with parallel function calls. When using
structured outputs set `parallel_tool_calls` to `false`."

**Measured cost, not assumed:**

- **Parallel tool dispatch in both loops** — `tool_loop.rs:3578` and
  `agent.rs:1157` both fan out with `futures::future::join_all`. Shipped
  2026-08-22, claimed ✅ in the parity matrix. Strict mode means one call per
  round: more rounds, more latency, faster round-budget burn, and it moots both
  the shared subagent budget pool and the only-the-last-call-can-be-truncated
  logic.
- We never send `parallel_tool_calls`, so we currently get Azure's default of
  `true` for free. Enabling strict means explicitly turning it off.
- `openai_chat_models` defaults to `["gpt-4o-mini", "gpt-5-mini"]`, and
  **`gpt-5-mini` is not on the strict-supported model list**.
- Strict forbids `minimum`/`maximum` on numbers; three of our schemas use them.
- Strict requires every property in `required`, with optionality as a
  `["string","null"]` union — a wire-format change each executor's serde types
  would need checked against.

**A correction to the harvest doc, twice on the same item.** §10 originally
called this a free win needing nothing from Cactus. It is not. Checking the API
contract before writing code is what caught it; the gap §10 describes is real, the
remedy was wrong.

**And one failure mode was already covered.** Phase 0's hallucinated
`lookup_org_chart` is refused today by `dispatch_tool`'s `other => "unknown tool"`
arm and by execution-core's purpose-lock. The genuinely unprotected thing is
**argument validity on the builtin paths** — `argument_repair` is scoped to
`mcp_call` alone, fail-open, missing-required-and-enum only. Validating arguments
pre-dispatch closes that without surrendering parallel calls, and is the
recommended replacement for this item.

## 2026-08-25 — A malformed tool schema degrades loudly, in both providers

**Decision.** `inference-core::provider::tool_parameters` replaces the bare
`unwrap_or_else` that silently turned an unparseable `parameters_json` into
`{"type":"object","properties":{}}`.

**Why the silence was the bug, not the fallback.** An open schema tells the
provider "this function accepts anything". The model then invents argument names,
the executor rejects them, and the only visible symptom is a tool that
mysteriously never works — while the schema was malformed all along and nothing
said so.

**Why the fallback stays.** Rejecting the request would fail an entire turn
because one of possibly twenty tools has a bad schema, turning a caller's
authoring mistake into an outage. Degrading that one tool and naming it — with the
tool name and the offending JSON type in the log — is proportionate.

**An absent schema is not a malformation.** Empty means "no arguments" and is not
reported, or the warning becomes noise operators learn to ignore.

**The same bug existed in two providers.** `openai.rs` and `anthropic.rs` each had
their own copy. The identical bug in two places is what a shared concern looks
like before it is shared, so the helper lives in `provider/mod.rs` and both call
it. Five tests, mutation-verified; 240 inference-core tests pass.

## 2026-08-25 — Tool arguments are validated before dispatch, in both loops

**Decision.** Check a tool call's arguments against the tool's own declared schema
before anything is dispatched, in model-gateway's `dispatch_tool` and in
execution-core's pre-pass. On failure the model gets the exact field problems plus
the schema, so a repair takes one round instead of a guess.

**Chosen instead of `strict: true`** (deferred the same day — it disables parallel
tool calls). This is a cure we own rather than a prevention we rent, and it
composes with parallel dispatch.

**The validator had no callers at all.** `model-gateway/argument_repair.rs` was
393 lines with 12 passing tests and a module doc stating it was wired to
`tool_loop`'s `mcp_call` arm. That arm does not exist. Moved to
`mp_contracts::tool_arguments` so both loops share one implementation, with the
dead-callers history recorded in its own docs.

**Fail-open is asserted, not trusted.** A pre-dispatch validator that refuses a
call the executor would have accepted breaks a working tool — strictly worse than
not validating. So a test requires every advertised tool in both catalogues,
given exactly its required fields, to pass.

**No audit step for a pre-pass rejection**, matching every sibling gate
(purpose-lock, leaf blocklist, plan mode, autonomy rung): nothing was attempted,
so nothing is recorded. The refusal is fed back as the next round's tool context.

**Three findings in our own code, which was the real value:**

1. `every_advertised_builtin_tool_has_a_dispatch_arm` dispatched every tool with
   `{}`. With validation in front, `{}` stopped reaching the arm for any tool with
   a required field — the test kept passing while proving nothing. It now
   synthesises minimally valid arguments and asserts validation did not fire.
2. The shared `failing_tool_call` fixture passed `{}` to `yr_weather` as a
   shortcut to a dispatch failure. Three tests broke, which is how it surfaced; it
   now passes valid coordinates so it fails for the reason its users test.
3. I asserted the validator stays silent on unparseable *arguments*. Wrong, and it
   should not: an unreadable *schema* silences it, while unparseable *arguments*
   are reported as a parse failure — naming the real cause rather than reporting
   it as a missing field.

Mutation-verified three ways: removing the chat gate, moving it after the arm
match, and removing the agentic gate. 2 291 Model Plane Rust tests pass.

## Our tool-dispatch path measured against the Needle harness (2026-08-25) — status: **measured, one new defect found**

Phase 0 produced a number for Needle (47.1 %) and none for us, so "are we better"
was unanswerable. Ran our real wire shape (`provider/openai.rs` serialization,
`tool_choice:"auto"`, no `strict`, temp 0.7 as both loops send) against the same
34 cases: **91.2 %** under conditions matched to Needle, **97.1 %** on our real
path, **91.2 %** on the full 25-tool catalogue Needle cannot fit at all.

Two things worth keeping:

1. **Norwegian is not our weak split** — the opposite of the prediction on
   record. NO matched or beat EN in all three arms. Needle's 11-point EN→NO drop
   is a 45 M-model tokenizer property, not a task property. Do not generalize
   small-model language deficits to our path without measuring.

2. **Required tool arguments are fabricated 55–70 % of the time**, and every
   fabrication is schema-valid. `get_shipping_quotes` requires postal codes and
   dimensions and its description says "never guess them"; the model invents
   `postal_code=5000`, `length_cm=30`, `name="Sender"` instead of asking.
   `called-without-inventing` was **0** across all conditions — the only two
   behaviours are ask or invent.

   A 2×2 ablation (preamble × temperature, 80 calls) shows this is **not** a
   prompt bug. An earlier single-sample reading suggested our "prefer calling a
   tool" preamble caused it (1/4 → 3/4); with repeats the effect vanishes into
   noise (n=20/cell, SE ≈ 11 pp). Recorded because the wrong version is the
   tempting one: it points at a one-line prompt fix that would not work.

   The §1c pre-dispatch validator does not and cannot catch this — it checks that
   required fields are present, not that they are grounded in the request. This
   promotes §5 (argument-to-source-span derivation) from "cheapest idea" to the
   best-evidenced item in `docs/external-ideas-harvest.md`: a span requirement on
   required arguments refuses exactly these calls, enforcing what the tool
   description already asks for and currently cannot.

Not fixed in this pass — recorded as the next actionable item, ranked #3.

## Tool calling checked against published research (2026-08-25) — status: **two defects found, none fixed**

Asked whether our tool calling is optimized against current research. It was
never checked against anything; this is the first comparison. Full write-up in
`docs/external-ideas-harvest.md` §11.

**Verdict: selection is strong, arguments are a generation behind.** BFCL went
from AST grading (v1) to state-based agentic grading (v4) explicitly because
syntactic correctness is considered solved and the frontier is semantic
correctness — and v4 carries a `missing-parameter` category that is exactly the
failure we measured. SAP's DiaFORGE telemetry: ~71 % of live enterprise APIs
declare required params and **~76–81 % of calls arrive missing at least one**, so
our fabrication finding is the normal case, not an edge case.

Two defects surfaced by the comparison, neither fixed:

1. **No elicitation mechanism anywhere.** 19/25 tools declare required params
   (76 %, matching the paper's ~71 %); exactly **one** tells the model to ask
   rather than guess, as unenforced prose in a description. The preamble never
   mentions missing arguments — while explicitly instructing "do not ask ...
   call the tool and let its result decide". Stated precisely: the ablation found
   **no causal effect** of the preamble on fabrication rate, so the claim is
   asymmetry, not causation — defences against under-calling, none against
   fabrication. Highest exposure is `book_shipment` (10 required fields, books
   for real; approval gating does not help because a fabricated postal code looks
   legitimate in the dialog).

2. **Four near-duplicate web tools in one catalogue.** `agent.rs:2592-2607`
   offers `web_search`, `web_fetch`, `web.search`, `web.read` together and the
   namespaced pair concedes the overlap in its own descriptions. This is
   DiaFORGE's distractor condition (35–38 % of production queries) and violates
   Anthropic's namespace-and-differentiate guidance.

Also recorded: **our 91–97 % is a selection number, not a tool-calling number**,
measured on 34 self-authored single-turn cases. Do not quote it as the latter.
Any future claim should come from BFCL v4's `missing-parameter` and `relevance`
categories instead of our own harness.

## Elicitation snippet for user-only tool arguments (2026-08-25) — status: **implemented, measured both directions**

Fix for the fabrication defect recorded above. Full write-up in
`docs/external-ideas-harvest.md` §12. Implementation:
`SNIPPET_USER_SUPPLIED_ARGS` + `USER_SUPPLIED_ARG_TOOLS` in
`runtime_loop/agent.rs`, gated like `SNIPPET_ACTION_TOOLS`.

**Result: fabrication 68.3 % → 86.7 % correct (+18.3 pp, p=0.016); on
`get_shipping_quotes` alone 1/20 → 12/20 (Fisher p=0.0004). Under-calling
unchanged at 60/60.**

Three design calls the baseline measurement forced, all against the plan as
originally ranked:

1. **Two tools, not eighteen.** 19/25 declare required params, but most requireds
   restate the request (`web_search.query`) or are public fact
   (`yr_weather.lat/lon`). Telling the model to ask for those manufactures the
   under-calling regression `PREAMBLE_CORE` exists to prevent. Rejected.

2. **Discoverable ≠ un-inventable.** Where a missing value can be found by
   another offered tool, the model already does it unprompted — it called
   `list_subagent_results` for a `child_run_id` and `list_social_accounts` for a
   `connection_id`, 10/10 each, with no prompt help. `read_subagent_result` and
   `execute_provider_action` deliberately excluded from the list; a test pins
   that exclusion with the reason.

3. **Per-tool prose is a measured non-fix.** `get_shipping_quotes` already said
   "never guess them" in its own description and invented anyway 19/20. Adding
   that sentence to more tools was the original plan and would have been
   cargo-culting. `PREAMBLE_CORE` left byte-identical, per its own doc comment's
   instruction to add a snippet rather than edit measured wording.

Honest limits: 8/20 still fabricate — a prompt cannot make this a guarantee, and
the residual is what §5 (argument-to-source-span derivation) is for. The clean
60/60 on the under-call direction rules out a regression below ~95 % (p<0.05) but
does not rule out 1–2 pp; zero regression is not claimed. Also note the *raw*
arm-C score falls 91.2 % → 85.3 % under the discredited pre-§10 rubric, which
counts a correct decline as a miss; under the fabrication-aware rubric the same
run is 97.1 %.

## `skill_budget_contract.rs` source-text parser fooled by rustfmt (2026-08-25) — status: **fixed**

Surfaced by the workspace run for the elicitation change, but unrelated to it:
`both_loops_mark_a_truncated_skill_identically` was failing with
"TRUNCATION_MARKER not found in model-gateway/src/skills.rs". The constant is
there; its parser searched for `TRUNCATION_MARKER: &str = "` on **one line**, and
rustfmt puts a long literal on the line after the `=`.

**This is the second time a source-text contract parser in this repo has been
fooled by rustfmt** — `compaction_parity_contract.rs::const_after_from` had the
same class of bug (two false readings) and the fix there was not generalized.
Replaced the inline closure with `str_const`, which finds the declaration by
name across visibility forms, scans to the first *unescaped* closing quote, and
decodes `\`-continuations and escapes so it compares **logical** strings. The two
crates need not wrap at the same column — the constant names differ in length —
so comparing raw source slices was guaranteed to break eventually.

Mutation-tested both ways: altering one marker fails the assertion, restoring it
passes. Workspace green at 2 296 tests.

Standing lesson for this class: a contract test that reads source text must
decode what it reads, or it asserts formatting rather than the invariant.

## Argument grounding (§5) implemented in both loops (2026-08-25) — status: **implemented**

`mp_contracts::tool_arguments::{ungrounded_arguments, grounding_message}`, called
right after `validate_arguments` in both `dispatch_tool` and the agent pre-pass.
Full write-up in `docs/external-ideas-harvest.md` §13.1.

Closes what the §12 prompt change could not: persuasion took
`get_shipping_quotes` from 1/20 to 12/20 and stalled; 8/20 kept inventing. This is
a check, so the residual is **zero by construction** — a value with no source in
the conversation never reaches an executor.

Decisions worth not re-litigating:

- **Grounding excludes the system prompt.** An observed fabrication was
  `from.name: "Verevon"`, which appears only in the preamble. Grounding against it
  would ground the invention. Pinned by contract test on both loops.
- **Digit runs, not substrings.** `30` matches "30x20x15" and must not match
  "130". The lenient direction is deliberate everywhere else (minor-unit
  conversion counts as grounded, string match is case-insensitive substring)
  because a false refusal is worse than a missed fabrication here.
- **`grounding_message`, not `repair_message`.** "Fix this field" invites a second
  guess. Measured: 9/9 refusals produced a question, 0/9 produced another guess.
- **The table is two tools, and must stay small.** Derivable values
  (`web_search.query`) and discoverable ones (`execute_provider_action.connection_id`,
  which `list_provider_actions` already handles at 10/10) must never be added —
  they would refuse calls that were about to succeed.

## `web.search`/`web.read` were classified OrgInternal — injection-defense hole (2026-08-25) — status: **fixed**

Found while examining §11's near-duplicate-web-tool finding; unrelated to the
duplication. Both tools are dispatched (`runtime_loop/mod.rs`) and fetch public
web content, and neither appeared in `TrustClass::classify` on either surface, so
both fell through to `OrgInternal`. That makes `is_external()` false, so
`framing()` returns `None`: open-internet content reached the model with no
"UNTRUSTED — data, not instructions" framing, and a skipped scan was not flagged
for audit either.

Both copies carried a "keep in sync by hand" comment and had drifted three ways
(gateway knew `fetch_url`, execution-core knew `web_fetch`, neither knew the
dotted pair). Both now list the same names, pinned by a mutation-tested contract
test — a hand-synced security rule with no test is how this happened.

**The aliases were deliberately NOT merged.** The constants' doc comment states
the underscore names are compatibility aliases and both routes share one Quarry
client, so the four tools are two. But dropping a name from the offered set drops
it from the purpose-lock allowlist, breaking any stored plan that names it, and
that inventory is unverified. The alias descriptions now say they are aliases and
name the preferred tool. Collapsing the catalogue stays open, pending a check of
stored agent definitions. No measurement shows the duplication costing accuracy —
`web_search` selection was 10/10 — so this is hygiene plus token cost, not a
demonstrated defect.

## The grounding gate was inert in the chat loop (2026-08-25) — status: **fixed, chat measured at 100 % fabrication**

Immediately after §13 claimed grounding in "both loops". It was in both loops and
covered only one: **the gateway offers neither ground-checked tool.** Its shipping
tool is `shipping_get_quotes` (agent loop: `get_shipping_quotes`), with dimensions
nested under `package`. The table used the agent's spelling, so chat invoked the
checker on every call and it never fired.

Worse, chat was the more exposed surface — no "never guess" line in its tool
description, and the §12 elicitation snippet lives in execution-core's
`compose_system_prompt`, which chat does not use. Measured on the chat catalogue:
**20/20 (100 %) fabricated both postal codes and all three dimensions.** The agent
loop's 55-70 % was the defended number.

Fixed: added `shipping_get_quotes` with `package.*` paths, and `book_shipment`'s
`price_amount_cents` / `carrier_code` / `service_name` (real order, real money;
legitimate values arrive via a quote in the conversation).

**Standing lesson — the one worth carrying:** a contract test asserting both loops
*call* a checker proves nothing about coverage. Assert **reachability**: no table
entry may name a tool no loop offers, and no loop may offer a tool of the checked
shape without an entry. The new test does both and is mutation-tested. This is the
same built-but-unreachable failure this repo has produced repeatedly, and this
time it was produced by the fix for the previous instance of it.

Two rejected approaches: keying the check on tool *names* (missed the chat
spelling, then flagged `track_shipment`, whose tracking number is always stated —
the working criterion is schema shape, a required `postal_code` or `*_cm`); and
adding "never guess them" to chat's description (already measured ignored 19/20 on
the agent side — a measured non-fix that would read as coverage).

Left open: chat has no elicitation snippet, so the gate is its only defense and it
always pays one round-trip where the agent loop avoids it 11/20. Fixing that means
touching session-core's prompt assembly; not done blind.

## Adoption scorecard verified claim-by-claim (2026-08-25) — status: **8 of 22 claims are built but unreachable**

A 22-claim adversarial verification of the four-harness adoption scorecard
(Claude Code / Hermes / DeepSeek / pi), each claim traced from advertisement to
production caller. 12 CONFIRMED-and-reachable, 8 with a real reachability
defect, 2 stale.

**The defects, each with its missing link — these are work items, not notes:**

1. **Context inspector returns nothing, ever.** `session-core/src/grpc.rs`
   calls `authorize_run_owner(..., OwnerIntent::Mutate)` *before*
   `get_context_assembly_inner`, and that function has no empty-string guard —
   it runs `SELECT ... FROM runs WHERE id = $1` and rejects. Every production
   request dies before the assembler runs. The whole SPA panel is unreachable.
2. **Extended-thinking dial has zero production writers.** `InvokeRequest.effort`
   is read once (`sse.rs`) and turned into `thinking_budget_tokens`, but nothing
   in the product ever sets it, and the `deep` tier has no reachable config.
3. **The per-call autonomy gate can never refuse anything.** Only one non-test
   constructor of `RunAgentRequest` sets the rung; every in-loop construction
   leaves it unset, which reads as the narrowest rung but is never checked
   against a graded grant.
4. **`ProviderError::TooLong` is never propagated.** Constructed at exactly one
   site (`overflow.rs` `classify_http_failure`); every consumer chain swallows
   it, so there is no reachable route to `too_long_status` at all.
5. **The mid-run queued-input fix is client-side dead.** The server half is real,
   but `ChatPage.tsx` passes `submitting={isStreaming()}` and the composer
   returns silently — so the silent-drop bug the module exists to fix is *still
   live in the product*.
6. **`save_memory`/`recall_memory` are advertised AND dispatched yet still
   unreachable** — the usual dead-arm test passes, so this one needs the full
   chain re-walked.
7. **Memory provenance renders only vacuously**: nothing writes a non-zero
   origin on the path that feeds it, so two of its three states cannot occur.
8. **`run_subagent` has no first-party dispatch path.** Dispatch is prefix-based
   on `subagent.`, and no catalogue advertises any `subagent.*` name.

**Two stale claims**, both in the optimistic-then-pessimistic direction that
this repo keeps producing: fork-semantics was recorded as "open, next in
sequence" when it is a dated **reject** (DeepSeek rejected it first and we
concurred); and the plan-approval ladder was recorded as "needs a product
decision" when the decision is recorded and the code shipped.

**Standing lesson, now third time this session:** a ✅ in
`claude-hermes-deepseek.md` means "the mechanism was built", and repeatedly does
not mean "a user can reach it." Verification must trace advertisement → caller →
writer → reader. The TL;DR now says this explicitly.

## P0 critical-path list verified item-by-item (2026-08-26) — status: **4 of 10 confirmed as stated; 6 wrong in the pessimistic direction**

A 10-item production-readiness P0 list, each claim verified against source, local
config, and where possible a live command or the running stack. Verdicts:

**CONFIRMED, and one is worse than claimed**
- **P0-5 tenant delegation** — real P0. 15 of 16 live workload principals carry
  `allowAnyOrg`; the guard exists in source and the running Auth Core bypasses it
  (`NODE_ENV=development` + `PLANE_SERVICE_PRINCIPALS_JSON`). But the fix is not a
  missing mechanism: `orgIds` already exists, is enforced, and 1 of 16 principals
  uses it. This is a migration, not a build.
- **P0-6 managed-run terminalization** — confirmed, and WORSE. Crash: no coverage
  at all. Cancel-race: none. Response-loss: start side only. The whole lease-backed
  reconciler (`recover_due_terminalizations` et al.) has **zero callers outside its
  own file on every branch**. Do not mistake
  `expired_worker_lease_reclaims_without_duplicate_start_receipt` for coverage — it
  is a real-Postgres test of *approval continuation*, not terminalization.
- **P0-9 Space computer** — all four assertions correct. Both stores are
  `map + RWMutex`, no DB/cache/object-store import, no migrations dir. Scope accepts
  only `thread`|`agent`; `space_id` is first-class in sessions/runs/execution protos
  and was never added to `sandboxes.proto`. `SnapshotSandbox` synthesizes a MinIO
  `object_key` and uploads nothing.
- **P0-1 artifacts (half)** — no signed artifact instance exists anywhere; no
  cosign/SLSA/SBOM in the plane; no CI workflow invokes the script.

**WRONG IN THE PESSIMISTIC DIRECTION — the documented failure mode, six more times**
- **P0-2 approval dispatch**: ~3/4 already shipped. Five continuation RPCs exist and
  are implemented; the encrypted exact-effect descriptor exists (migration 0022 +
  `continuation_crypto.rs`, AES-256-GCM, row-bound AAD, ZDR rejected pre-creation,
  fail-closed on missing key); the durable lease/receipt/outcome state machine has a
  no-double-execute guard. Only **KMS lifecycle** is genuinely missing (one base64
  env key, hardcoded `v1:`, no envelope encryption or rotation). "HITL is decorative"
  is not supportable.
- **P0-3 capability health**: "zero configured reporters" is false — three exist and
  are wired (execution-core `health_attest.rs`, conversation-core, shipping-core),
  plus a dedicated `capability-health-proof.sh` harness. "27 capabilities" matches
  nothing: 24 in `registry.go`, 30 rows across 6 migrations.
- **P0-4 retention/ZDR**: "fails closed with no verified ZDR provider" is exactly
  right (0 ZDR vars in `deploy/.env`). "Default all-ZDR" is wrong — `DEFAULT_POSTURE`
  is `zdr:false`; ZDR is an opt-in plan-gated add-on. And **"no live chat answers
  exist" is refuted on the running stack**: 8 assistant messages, 8 completed runs,
  8 `cost_entries` billed against gpt-4o-mini x7 and claude-sonnet-4-6 x1.
- **P0-7 memory**: `DEGRADED_SEMANTIC_UNVERIFIED` does **not** stick. It is the
  pre-first-call state and clears on the first semantic call; model-gateway
  prefetches every turn, so it clears on turn one. The states that persist are
  `EMPTY_INDEX` and `LEXICAL_FALLBACK`. Five statuses exist; the docs say four.
- **P0-8 MCP containment**: the negative matrix exists — loopback, link-local, `::1`,
  private ranges, redirects and cloud-metadata cases across several test files, plus
  `mcp_dns_revalidator_test.go`. And an **OAuth remote-MCP client exists**: full
  OAuth 2.1 + DCR, authorization-code exchange, refresh with writeback, encrypted
  refresh tokens, `mcp_oauth_tokens`. `.env.example:79` names Visma Net as its use
  case, so Visma is a credentials state, not a missing client.
- **P0-1 artifacts (other half)**: the *distinct rollback artifact* is fully built
  and tested — external-only locator, independent verify key,
  `ROLLBACK_ARTIFACT_MANIFEST_SHA256` re-derived from a signature-verified view, and
  an explicit "candidate cannot be its own rollback" guard. Only 11 of 21 containers
  are local builds (10 are upstream images). "Artifact v3 refuses the tree" is stale:
  both worktrees report 0 dirty paths; what blocks a build now is the release-mode
  evidence gates.

**Numbers the list got wrong, corrected**
- buf lint: **1,502** errors (1,428 COMMENTS-rule, 74 other) across all 20 protos —
  not unquantified "debt". Identical from all three invocation forms.
- "No buf breaking baseline" is wrong in mechanism and worse in effect: the baseline
  IS configured (`buf.yaml breaking: use: [FILE]`), CI-wired
  (`buf.yml` runs `buf breaking --against .../baseline.binpb`), and committed — but
  it is a **0-byte placeholder**, so the command hard-errors "image contains no
  files" instead of comparing. A gate that looks like coverage and cannot fail.
- Coverage figures (approval delivery 35 %, SSE 56 %, runtime loop 65 %) are stale
  quotes; not reproduced, and should not be cited until re-run.
- 21 containers is right for the release/production topology (22 in dev, 24 union).

**Standing lesson, again:** six of ten claims understated what exists. Every one was
found by searching for the *behaviour* rather than an expected type name — the same
method failure §3 of `claude-hermes-deepseek.md` documents. A P0 list is a build
plan; each false "missing" is budgeted work that is already done.

### Correction to the entry above, same day — P0-10's coverage figures ARE reproducible, and both are wrong

The entry above said the coverage figures were "stale quotes; not reproduced, and
should not be cited until re-run". Two of the three have now been re-run
(`cargo llvm-cov --lib -p execution-core`), and the claim is wrong on both:

| Claim | Measured (lines) |
|---|---|
| runtime loop 65 % | **82.5 %** — *above* the 80 % target |
| approval delivery 35 % | **50.8 %** (53.5 % regions) — below target, but not 35 % |
| SSE 56 % | still unverified — `sse.rs` is in model-gateway, a different crate |

The single "runtime loop" figure also hides the distribution, which is where the
real work is: `retry.rs` 100 %, `skill_budget.rs` 99.0 %, `agent.rs` 91.4 %,
`mod.rs` 62.2 %, **`subagent_results.rs` 36.5 %**. Quote the file, not the
directory. execution-core lib overall is 76.0 % lines.

Two further corrections to the entry above:

1. **A second proto breaking-change detector does exist** — `mp-orchestration/
   tests/proto_wire_parity.rs`, cross-language canonical wire-byte goldens with
   Go and Python siblings, whose own docstring states "Updating a golden
   constitutes a wire-level breaking change." It is narrow: **one message**
   (`OrchestrationEvent`), not the 20-file proto surface. Worth naming precisely
   because it would NOT have caught the `RunAgentRequest` field-11 collision
   resolved in `a06eae7b` — the message it guards is not the one that collided.

2. **The empty-baseline finding survives an adversarial challenge.** A verifying
   agent reported the gate "works — I proved it detects a real break". Re-checked
   directly: the committed blob is `e69de29bb2d1d6434b8b29ae775ad8c2e48c5391`,
   git's canonical EMPTY blob, 0 bytes both on disk and at HEAD, and the exact CI
   command still fails with "image contains no files". The agent almost certainly
   built a fresh baseline and tested against that, which proves the *tooling*
   works, not the *committed* baseline. Recorded because the same mistake is easy
   to repeat: regenerating an artifact and then testing the regenerated copy
   measures nothing about what is checked in.

## Chat elicitation + the third shipping spelling (2026-08-26) — status: **implemented, measured; snippet is weak on chat and the gate carries it**

Closes the asymmetry recorded in `external-ideas-harvest.md` §14 (chat had zero
tool guidance), and fixes a grounding bypass found on the way in.

**The bypass first, because it is the sharper find:** the chat dispatch arm
accepts `"shipping_get_quotes" | "shipping.get_quotes"` — the dotted id is what
the Console uses for explicit tool selection, arriving as a CLIENT-declared tool.
`inline_tool_allowed` is a denylist, so it admits the name; it is not a builtin,
so `builtin_argument_problem`'s def-lookup `?` bailed before schema OR grounding
ran; and the arm then executed it against the real shipping aggregator. Third
spelling of one capability, and the second time an alias walked around a gate
keyed on exact names. Fixed three ways: a `GROUNDED_ARGUMENT_PATHS` entry for the
dotted spelling, the gateway gate restructured so grounding runs for ANY
dispatchable name (schema validation still only where we hold the schema), and
the reachability contract extended to count a dispatch arm as a reachable route —
mutation-tested.

**The snippet:** `SNIPPET_USER_SUPPLIED_ARGS` now exists in model-gateway too,
byte-identical to execution-core's (pinned by a decoded-string contract test —
raw-source comparison would assert formatting, the `skill_budget_contract`
lesson applied pre-emptively), injected in `sse.rs` before the first non-system
turn, gated on the FINAL offered set.

**Measured, chat catalogue, temp 0.7, 5 samples/case:**

| Direction | before | after |
|---|---|---|
| FAB — must not invent (4 shipping queries) | 0/20 | **4/20** |
| CALL — must call now (valid cases) | 20/20 | **20/20** |

Two honest readings that must not be lost:

1. **The snippet alone is weak on chat.** +4/20, nowhere near the agent loop's
   +18.3 pp — there the snippet rode with a full preamble; here it is the only
   tool guidance in the stack. On chat the grounding gate carries correctness
   (all 16 remaining fabrications are refused pre-dispatch); the snippet's value
   is saving the refusal round-trip, and it saves 4/20, not 11/20. Do not quote
   the agent loop's number for chat.
2. **Zero under-calling regression**, including the case built to catch it: a
   fully-specified quote request (every value stated) calls 10/10 with the
   snippet present.

Also corrected in place: `MODEL_PLANE_DEEP_DIVE.md` and `feature.md` both
asserted `grep -rni visma` returns 0 matches — it returns 22 across 7 files
(`mcp_oauth.rs` is a full OAuth 2.1 + DCR client naming Visma Net as its use
case). Those two paragraphs seeded the false "no OAuth remote-MCP client exists"
P0 item; the correction is stamped STALE-as-of-2026-08-26 above the original
text rather than deleting it, so the provenance of the wrong P0 item stays
visible.

Measurement harness bug worth recording: 4 of 8 must-CALL cases initially
expected `track_shipment`/`company_lookup`, which chat's catalogue does not
offer — `web_search` was the model's CORRECT answer there. A must-call case is
only valid against the catalogue actually offered; scores before exclusion
(20/40) would have read as a selection collapse that never happened.

Workspace green at 2,327.

## The 8 reachability defects fixed (2026-08-26) — status: **all 8 closed; three were built-and-never-wired writers**

The parity verification found 8 mechanisms that existed, were tested, and could
not activate in production. All fixed, each at its named missing link:

1. **Context inspector (C4)** — session-core ran `authorize_run_owner` on an
   ALWAYS-empty `run_id` before the assembler, 404ing every production request.
   Run authz is now guarded on a non-empty id; thread authz (the line above it)
   still covers the thread-scoped case the inner function explicitly supports.
2. **Thinking dial (C2)** — the writer existed all along: the composer's
   response-mode selector (Auto / Raskt svar / Dyp research) rode the submit
   payload as `responseMode` with ZERO downstream readers — its "deep" arm even
   pushed a `"reason"` tool tag nothing read. Now mapped to the wire's `effort`
   (quick/deep; Auto omits the key), through SendOptions into `streamChat`. The
   read path (gateway → thinking budget → provider → reasoning_delta → Innsikt
   popover) was already complete.
3. **Autonomy gate (C7)** — two faults: the only producer set a rung exactly
   when the check was skipped (`!req.plan_mode` gating), and the approved grant
   was written to the PLAN run's metadata under a comment claiming "the next
   RunAgentRequest reads it back" — no read existed. The gate now checks
   unconditionally, and `PlanModeStore` carries the grant THREAD-keyed from
   `handle_exit_plan_mode` to the next dispatch. In-memory: a gateway restart
   drops the grant to UNSPECIFIED (today's behaviour for every run) — accepted
   because the durable posture gates are unaffected; the durable thread-scoped
   carrier belongs in session-core and remains open.
4. **`ProviderError::TooLong` (P2)** — produced at five provider sites and
   discarded by every chain's generic `Err(e) => warn!` arm. `ThrottleState`
   now records the first TooLong during the walk (the walk still continues —
   a larger-window provider is the recovery path) and exhaustion surfaces it
   typed, RateLimited outranking it since a throttled provider might still
   serve the prompt. The embedding walk gained the same state; it previously
   reported overflow as generic exhaustion, which callers retried.
5. **Queued input (P3)** — the composer swallowed Enter for the entire stream
   (`submitting={isStreaming()}` guard), so the whole server arc had no
   reachable client. New opt-in `allowMidRunSubmit` (chat page only — the
   dashboard's own window is a real double-send guard): Enter now routes into
   `sendContent` → `deliverMidRun`; the Stop button is unchanged.
6. **`save_memory`/`recall_memory` (H1)** — bound to `cap.memory.{index,search}`,
   which existed ONLY in registry.go's static seed while production resolves
   from Postgres: every call died at the fail-closed gate. Migration 0014 seeds
   both rows (unavailable, like 0008/0013 — source presence is not health), and
   execution-core's health reporter now attests them per heartbeat from a live
   session-core connect probe, so a session-core outage stops renewing them and
   its recovery brings them back without a restart.
7. **Memory provenance (D7)** — `search_agent_memory` hardcoded
   `MemoryProvenance::Unknown` with a comment scoped to the management surface —
   but SearchMemory is what feeds the CHAT recall notice, so `stated`/`inferred`
   were unreachable on the one surface built to display them. The search SQL now
   selects `source_links` and derives through `MemoryProvenance::classify`, the
   single existing authority.
8. **`run_subagent` (C5)** — dispatch is prefix-matched on `subagent.`, the
   capability binding was seeded in 0008, the prompt snippet gates on the name —
   and no catalogue advertised any `subagent.*` tool. `subagent.task` is now in
   `offered_tool_defs` with the `{goal, max_rounds}` contract `parse_task`
   already enforces.

Pattern note for the file: items 2, 3 and 8 are not "missing features" — they
are writers/definitions that existed in half-built form (a UI selector with no
reader, a stored grant with no reader, a dispatcher with no advertiser). The
16-day-old lesson stands: verify advertisement → dispatch → writer → reader as
a chain, never any link alone.

Verification: Rust workspace 2,330 passed / 0 failed; SPA 1,217 passed across
159 files; `tsc -b` clean; capability-core `go build` clean. New tests: TooLong
exhaustion priority (2), memory attestation honesty (1), mid-run composer
regression (3), plus the C4 guard exercised via existing context-assembly
paths.

### Correction, same day — two of the 8 "fixes" were themselves inert, and the test that should have caught it also false-passed

Asked directly whether all 8 were working, and checked instead of asserting.
Two were not:

**C7 (autonomy grant) was a no-op.** `PlanModeStore::record_grant` is keyed by
thread, and the production caller — `http_routes::plan_approval` — sent
`session_id: String::new()`. The guard correctly refused every empty key, so no
grant was ever recorded. Identical in shape to the C4 bug fixed hours earlier
(authz on an always-empty `run_id`): the fix reads a field the caller leaves
blank. Now `plan_approval` resolves the run's `thread_id` via `GetRun` before
granting — server-side from the already-ownership-checked run, so a caller
cannot attach a grant to a thread it does not own. Pinned by a **round-trip**
test (write with the approval's thread, read with the next run's thread) plus
org/thread isolation cases; mutation-verified by reverting the record call.

**C5 (subagent.task) created a newly-advertised-but-denied tool.**
`cap.agent.spawn` is seeded `unavailable` by 0008 doctrine and NOTHING attests
it — so advertising the tool meant the model would now try it and hit the
fail-closed gate, having burned a round. Attested from the same session-core
probe as the memory pair, which is the truthful dependency:
`register_delegated_child_run` is a session-core StartRun, and the nested loop
otherwise runs in-process on the parent's own inference path. Health attestation
is not permission — the row's `medium` risk still decides allow/ask/deny.

**And the new contract test guarding this false-passed.**
`every_offered_tool_capability_has_an_attestor` first checked
`health_attest.rs.contains(&capability)`, which matched the
`pub const X: &str = "id";` DECLARATION — so deleting the attestation while
leaving the constant still passed, verified by mutation. It now resolves the
constant→id map and searches only inside the bodies of `attestable` and
`memory_attestations`. Both mutations (drop the spawn attestation, drop a memory
attestation) now fail it.

**The test also surfaced 12 more instances of the same defect, unverified.**
Twelve advertised tools are bound to capabilities execution-core does not
attest: `cap.tool.information.read` (yr_weather, traffic, news,
company_lookup), `cap.tool.shipping.track`, `cap.tool.social.{read,publish}`,
`cap.retrieval.query`, `cap.tool.provider.{read,execute}`, `cap.browser.open`,
`cap.tool.http`, `cap.skill.summarize`, `cap.agent.lineage.read`. Each
PLAUSIBLY belongs to another service's reporter — but the three reporters
confirmed to exist attest only `cap.command.{shell,sandbox}` (execution-core),
`cap.tool.ticket.create` (conversation-core) and
`cap.tool.shipping.{read,book}` (shipping-core). **`cap.tool.shipping.track` is
not among them.** Held in a named `UNATTESTED_BASELINE` quarantine — explicitly
a record of an open question, not an approval — so the list cannot grow while
the question stays visible. Verified-elsewhere entries are kept in a separate
list that names the owning reporter.

Honest status: 8 of 8 have their missing link closed in source with mutation-
tested guards on the ones that carry them. What is NOT established is live
end-to-end proof for any of them — no deployed stack was exercised, the 0014
migration has never been applied, and the grant store is in-memory so it fails
open to UNSPECIFIED on gateway restart.

## Live proof against the running stack (2026-08-26) — **3 of 8 proven live; a 403 security boundary and a fragile test found doing it**

Migration 0014 applied to the live `session_core` DB (capability rows live there,
not a `capability_core` DB): 29 → 31 rows. Four services rebuilt from
`469c6a74` and recreated in the `model-plane` project (`rev=469c6a74` on all
four); capability-core rebuilt after the finding below.

**PROVEN LIVE**

| | Evidence |
|---|---|
| 0014 applied | rows exist, `unavailable/health_not_attested` per doctrine |
| **H1** memory tools | `cap.memory.search` + `cap.memory.index` → **`available/session_memory_probed`** with a real `health_checked_at`; execution-core logs `INFO runtime capability health attested` for both |
| **C5** delegation | `cap.agent.spawn` → **`available/session_run_registration_probed`**, same log line |
| **D7** provenance (data path) | the exact new SELECT runs against the live schema, and all 4 `agent_memory` rows carry `extractor:llm` → `classify()` = **INFERRED** where the old code hardcoded `Unknown` |

Pre-fix state captured live first, so these are before/after and not just
after: `cap.memory.*` genuinely absent from the table, `cap.agent.spawn`
`unavailable/health_not_attested`, and **0 runs with an empty id** — which is
why C4's `authorize_run_owner('')` could never match.

**THE 403 — a deliberate boundary my fix ran into.** The first restart produced
`capability-core refused the cap.memory.index attestation (status 403
Forbidden)` for all three. Cause: `genericGlobalHealthCapabilityIDs` in
`capability-core/internal/api/availability.go` is an explicit allowlist —
previously only `cap.command.{sandbox,shell}` — existing so execution-core's
health credential cannot become "a universal make-available authority". Widened
to the five ids execution-core genuinely owns the runtime for, with the
reasoning in the file. **No Rust test could have caught this**: the allowlist is
Go. Added `every_attested_capability_is_allowlisted_by_capability_core`, which
reads across the language boundary like `cross_service_loop_contract.rs`;
mutation-verified by removing one id from the Go map.

**A test that silently required the stack to be DOWN.**
`invoke_stream_agentic_reuses_the_prepared_session_run` asserted an
`agent_dispatch_unreachable` outcome while inheriting `make_state`'s default
`http://localhost:9093` — which is execution-core's published port. With the
stack up the gateway reached the real service and got
`agent_dispatch_rejected`. Proven by stopping the container: fail → pass, no
code change. Now points at a port the OS just confirmed free
(`unreachable_execution_client`), so it is hermetic; 31/31 pass with the stack
running.

**NOT PROVEN LIVE — and why.** C4, C2, C7 and P2 all need an authenticated
request (`GET /v1/threads/:id/context` returns 401 unauthenticated), and P3
needs a browser session. Getting there means either a real logged-in session or
minting a service token to impersonate a service — the latter is fabricating a
credential to satisfy my own verification, so it was not done. Their status
stays: source-correct, unit- and contract-tested, mutation-verified where a
guard exists, **not observed end to end**. The honest scoreboard is 3 of 8
proven live (4 counting D7's data path), 8 of 8 fixed in source.

Also: `deploy/.env` is gitignored and absent from a fresh worktree, so
`scripts/compose.sh` cannot build from a clean checkout — the same finding the
P0 verification flagged. It was copied in to build and deleted afterward.

### The remaining 4 need a credential decision, not more code (2026-08-26)

C4, C2, C7 and P2 are each observable only through an authenticated request.
Two paths exist and both are the operator's call, not mine:

1. **A real session** — log into the SPA, take the bearer from any `/v1/...`
   request. This proves the production path, including the BFF hop.
2. **The gateway's dev bypass** — `MODEL_GATEWAY_AUTH_DEV_BYPASS` +
   `ALLOW_INSECURE_DEV_DEFAULTS`. It "accepts an unverified bearer", i.e. it
   disables authentication for the whole gateway. It is double-gated precisely
   so it cannot be enabled casually, and this stack holds real org data and live
   provider keys. Flipping it is a security-posture change on a running system,
   so it was NOT done unilaterally.

What was ruled out explicitly: minting a service token to impersonate a service.
That is fabricating a credential to satisfy one's own verification, and a proof
that requires forging its own premise is not a proof.

Shipped instead: `scripts/tests/reachability-live-proof.sh`. Given `MP_TOKEN` it
decides all four from observed behaviour — HTTP status plus service logs plus the
durable row — and every check is written to FAIL on the documented pre-fix
symptom, so a pass is informative rather than vacuous. Verified fail-safe: run
with an invalid token it reports **4 SKIP / 0 FAIL / 0 PASS**, never a false
pass, and a request that does not complete is a SKIP rather than a verdict.

P3 (mid-run submit) is the one that genuinely needs a browser: the assertion is
that Enter during a live stream produces a queued-input strip instead of being
swallowed. It has three component tests; the live version is a UI interaction.

Standing status: **8 of 8 fixed in source, 4 of 8 proven live** (0014, H1, C5,
plus D7's data path). The other four are one valid bearer away, and the
instrument to decide them is committed.

### The dev auth bypass does not work (2026-08-26) — a NEW defect, and it blocks the last four proofs

Enabled at the operator's explicit request, on model-gateway only, to prove C4,
C2, C7 and P2 live. It does not function, so those four remain unproven.

**What was verified, in order:** both gates reach the container
(`docker exec … echo $MODEL_GATEWAY_AUTH_DEV_BYPASS` → `1`, and both are in the
compose `environment:` block, so the shell export is not silently dropped —
which is the usual trap here). The gateway then LOGS acceptance on every
request: `WARN MODEL_GATEWAY_AUTH_DEV_BYPASS enabled — accepting bearer without
verification`. And every `/v1/*` request still returns **bare 401,
content-length 0** — `/v1/models`, `/v1/threads`, `/v1/threads/:id/context`,
with and without `x-user-id`/`x-org-id`. `/healthz` returns 200, so the public
router is fine.

**Localized:** session-core receives NOTHING during a context request, so the
rejection happens inside model-gateway before its own handler runs. Of the
layers after `require_auth`, `authorize_principal_route` is the only one that
returns 401 silently (two `?` sites: absent `Claims`, and
`Claims::principal_kind()`). `rate_limit_middleware` only ever returns 429.

**Why this is surprising and worth a real investigation:** on source,
`dev_bypass_claims` builds `principal_type="user"`, `sub == user_id`,
`service_id: None`, which satisfies `principal_kind`'s User arm exactly; and all
eight `verify_delegated_*_bearer` calls return `Ok(None)` when their header is
absent, before touching JWKS. So the bypass branch should reach `next.run(req)`
with valid claims. It logs that it did, and the request is still refused. The
gap between "the code says this works" and "it does not" is the whole subject of
this session, now in the verification tool itself.

**Auth was restored immediately and confirmed:** `BYPASS=0`, `INSECURE` empty,
**zero** bypass log lines, and an unauthenticated `/v1/models` returns 401. The
copied `deploy/.env` was deleted again.

Standing status unchanged: **8 of 8 fixed in source, 4 of 8 proven live** (0014,
H1, C5, D7's data path). The remaining four need a real SPA session — the
bypass is not a usable substitute until this defect is fixed, and
`scripts/tests/reachability-live-proof.sh` will decide all four the moment a
valid bearer exists.

### CORRECTION — the dev auth bypass is NOT broken; the entry above was wrong (2026-08-26)

Asked to fix it. There is nothing to fix: the behaviour is deliberate, and a
test has pinned it since before this session —
`auth::tests::dev_bypass_cannot_supply_a_data_plane_bearer`, whose name states
the rule.

**Why it is right.** Every delegated bearer is a *user credential for another
plane*. If the gateway's dev bypass could MINT one, then setting a local flag on
model-gateway would become unverified access to Data Plane documents, Session
Core threads, and Capability Core policy. The bypass is scoped to the gateway's
own authentication on purpose; it does not propagate trust across planes. The
visible consequence — any route whose handler EXTRACTS a delegated bearer
returns 401 under the bypass — is the boundary holding, not failing.

**What I got wrong, and how.** I traced the 401 correctly to
`VerifiedInferenceBearer`'s extractor rejecting a missing extension, then wrote
a "fix" synthesising all eight delegated bearers from the presented token. The
existing test failed immediately and stopped it. Diagnosis right, conclusion
wrong: I read a deliberate limitation as a defect because the acceptance log
("accepting bearer without verification") reads like success. `CLAUDE.md` says
to check `docs/decisions/ledger.md` before proposing to change something that
already exists in two forms — the equivalent check here was the test suite, and
running it is what caught me.

**Kept, since the gap was real even if the diagnosis was not:**
`dev_bypass_survives_the_real_layer_stack` (the prior bypass tests layered
`require_auth` alone over a handler that extracts nothing, so nothing covered
the deployed `require_auth` → `authorize_principal_route` stack) and
`a_supplied_delegated_bearer_is_still_verified_under_the_bypass` (a junk
`x-inference-authorization` must be refused, never shadowed). Plus a comment at
the pass-through block stating the boundary and its intentional consequence, so
the next person tracing that 401 finds the reason instead of re-deriving it.

**Consequence for the four unproven fixes.** The bypass was never a valid route
to them — not because it is broken, but because it is correctly scoped. C4, C2,
C7 and P2 need a real session, full stop.
`scripts/tests/reachability-live-proof.sh` still decides all four the moment a
valid bearer exists. The previous entry's claim of "a NEW defect" is withdrawn.
