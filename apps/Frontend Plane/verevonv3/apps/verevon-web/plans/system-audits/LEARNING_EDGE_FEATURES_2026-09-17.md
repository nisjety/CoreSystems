# What gives Verevon an edge — external landscape research and build list

> **Execution update — 2026-09-19:** [Q05–Q07](../product-recordings/evidence/PRODUCT_RECORDING_Q05_Q07_2026-09-19.md) supersedes the older delivery/parse/retry and memory edit/forget findings below. Source isolation, deletion across a background cycle, schema-bearing skill reviews and one delayed retry recovery were verified locally. Historical exhausted events are not all recovered; grounding remains disabled and audit outbox delivery is still unhealthy. [Q08–Q10](../product-recordings/evidence/PRODUCT_RECORDING_Q08_Q10_2026-09-19.md) tracks the current product recording gate. The research and earlier observations remain historical evidence.

> **Execution update — 2026-09-24:** §7 root-causes the §5.5 parse failure and records the parser fix, corrects the §5.4 default-model claim, and adds the memory lineage and deletion fixes and the `memory-dedup` tool. `MEMORY_GROUNDING_ENABLED` is still off. None of it is deployed yet.

Research and design pass, cross-referencing `CACHE_COMPACTION_AUDIT_2026-09-17.md`,
`LEARNING_SYSTEM_REDESIGN_2026-09-17.md`, the chat design/UX/implementation docs, and
`CHAT_PARITY_AUDIT_2026-09-15.md` against 4 additional Hermes Agent docs pages and 7 external
memory/context projects the user named: Nous Research's Honcho/context-files/memory-providers
pages, OpenViking, mem0, hindsight, holographic-memory, RetainDB, byterover-cli, supermemory.
Every external claim was independently fact-checked (adversarial verification pass); refuted
claims are excluded below. Produced by a research workflow — 54 agents, 0 errors.

## §0.0 — Original research status

Item #1 below (the recall-ranking fix) is **built, tested, deployed, and verified live** (§3,
§4). Items #2–#5 are designed but not started. §4 records a **root-cause correction to #3**:
the skill pipeline's blocker is an event-delivery gap, not (only) the ZDR gate.

---

## 1. Per-source verdict

**Hermes (Honcho / memory-providers / context-files).** One technique worth adopting: Honcho's
"dialectic reasoning" — a standing, continuously re-derived user-model object, distinct from raw
memory storage. Everything else in this cluster is a pass: Honcho itself is a third-party paid
service Verevon shouldn't depend on; the memory-providers "swap one plugin" model doesn't fit
Verevon's deliberate two-tier design; context-files is a convention-loading mechanism (like
CLAUDE.md), not a memory technique.

**mem0.** Not worth adopting mem0's *current* architecture — its April 2026 rewrite dropped
reconciliation for single-pass ADD-only extraction, and mem0's own open bugs (#4956, #5867) show
this produces exactly Verevon's duplicate-fact failure mode. mem0's *original* design
(retrieve-then-LLM-picks-ADD/UPDATE/DELETE/NOOP) is the right shape, but mem0's own team abandoned
it because overwrite/delete silently destroyed information. Adapt the "retrieve-then-reconcile"
core; reject "overwrite-then-delete."

**supermemory.** Two precise techniques worth adopting: (1) typed-edge consolidation — an
`Updates`/`Extends`/`Derives` edge plus an `isLatest` flag, implementable on the existing Postgres
`agent_memory` table as a `supersedes_id` + `is_latest` column pair, no graph DB needed; (2)
Reciprocal Rank Fusion merging lexical (BM25) and vector search — the specific formula this
document's item #1 (§3) already adopted. No documented recency-decay formula was found — a real
gap Verevon still has to design.

**byterover-cli.** Worth adopting: the review-gated curation loop (`curate` →
`review pending/approve/reject`) maps directly onto capability-core's existing skill-review
pattern, extended to gate memory writes the same way. Also worth borrowing as a pattern (not a
specific algorithm): tiered retrieval — resolve cheap deterministic lookups before any LLM/
embedding call. Not worth adopting: the markdown-file/no-vector-DB storage model — Verevon's
Postgres+Redis stack is a deliberate choice byterover bets against.

**OpenViking.** Marginal value. One claimed structural detail (three parallel top-level
directories) was independently refuted — the real structure is nested, not sibling. The one
transferable idea is L0/L1/L2 progressive-disclosure loading (abstract → overview → full content
loaded only when necessary) — a token-budgeting idea relevant to *compaction*, not memory ranking.
Its headline benchmark numbers are self-reported at low confidence; treat as "a real shipped
pattern," not proof of anything quantitative.

**hindsight.** The single most directly applicable external precedent. Two named techniques: (1)
"mental models" — a standing, background-refreshed answer to a fixed question about an entity,
read with zero retrieval/LLM cost — a more concretely specified version of Honcho's dialectic
reasoning, and it maps almost exactly onto Verevon chat's already-shipped "utledet" (derived)
provenance labeling; (2) "refined rather than overwritten" observations — independent
confirmation of the non-destructive-supersession pattern also seen in supermemory and RetainDB.
hindsight also runs RRF across four parallel strategies (semantic/keyword/graph/temporal) — a
third independent source for the RRF ranking fix. Its LongMemEval numbers were independently
reproduced by outside parties (not merely self-reported), carrying real evidentiary weight.

**holographic-memory.** Not worth it. A genuine technique (Vector Symbolic Architecture / Binary
Spatter Code — XOR binding, majority-rule bundling, Hopfield cleanup), but it solves compositional/
analogical binding at the representation level — a different problem than any of Verevon's named
gaps. Adopting it would mean re-architecting the embedding layer for a capability nothing on
Verevon's gap list calls for.

**RetainDB.** Not worth adopting as a product — its core recommendation (Postgres+pgvector for
server mode) directly conflicts with this session's own architecture decision (§2 of the learning
redesign doc: don't add pgvector to `agent_memory`, the Redis semantic layer already covers it).
Worth citing as the most explicit reference schema: a typed `RelationType`
(`updates|extends|contradicts|supports|derives`) plus `validFrom`/`validUntil` temporal-validity
fields — the most fleshed-out version of the non-destructive-supersession pattern found across all
8 sources. Its retrieval recipe (BM25 + vector + graph via RRF + reranking) is a fourth
independent confirmation of RRF.

---

## 2. Top 5 ranked build items

**1. Replace the broken recall-ranking query with RRF fusion over the already-live semantic
layer — DONE, see §3.** Closes two named gaps at once: the literally-broken `LIKE`-based ranking,
and the fact that the real semantic layer (letta-bridge → agent-memory-server → Redis) was
"shadowed" as a shortfall-only backfill instead of a primary signal. Corroborated by three
independent sources (supermemory, hindsight, RetainDB) — unusually strong convergent signal for a
single technique.

**2. Ground the extractor against existing memory slots, with non-destructive supersession.**
Before writing a new fact: cheap key-match lookup, then a semantic-layer lookup against same-scope
memories; an LLM classifies the candidate as ADD / EXTEND / SUPERSEDE / NOOP against the top
matches; a SUPERSEDE writes a *new* row with `supersedes_id` + `is_latest` rather than overwriting.
Directly closes the "same fact stored 6 times under different keys" bug. Four independent sources
point at this (mem0's original design, supermemory's typed edges, hindsight's refinement,
RetainDB's relation types) — build to the non-destructive lineage (supermemory/hindsight/
RetainDB), explicitly avoiding both ends of mem0's history (its abandoned destructive
overwrite/delete, and its current zero-reconciliation ADD-only). Effort: medium-high (extractor
rewrite + 2-column migration); risk: medium (touches the write path — ship behind a flag with the
old path as fallback).

**3. Stamp the ZDR attestation so the auto-skill-generation pipeline can actually fire.**
capability-core's LLM-review-of-completed-transcripts → propose-skills pipeline is fully wired and
has simply never executed, because nothing stamps the required ZDR attestation on `RUN_COMPLETED`
envelopes. Highest ROI on this list by construction — not new engineering, unlocking work that
already exists. Caveat: if "ZDR" here means genuine compliance verification rather than a code
flag, this needs scoping with whoever owns that attestation before being treated as a quick win.

**4. A "mental model" standing derived-profile writer, surfaced through the existing "utledet"
provenance UI.** A small fixed set of named questions per user/session scope, refreshed by a
background job, exposed through Verevon chat's already-shipped provenance labeling. Closes three
things at once: gives the duplicate-fact problem a synthesis-layer escape valve (a standing answer
doesn't need six raw entries to stay correct); makes "utledet" show something richer than
backfilled facts; gives the still-open "no per-entry memory correct/forget UI control" gap
(CHAT_PARITY_AUDIT §3.8) a well-scoped first target. Effort: medium; risk: low-medium (additive,
doesn't touch raw memory read/write paths).

**5. Make Anthropic prompt-cache breakpoints aware of the hand-rolled conversation compaction.**
Already scoped in detail in `CACHE_COMPACTION_AUDIT_2026-09-17.md` §4.2 — the cache_control
implementation has zero awareness of when compaction runs, so a compacting turn almost certainly
invalidates cache prefixes on a schedule uncorrelated with breakpoint placement. Pure latency/cost
waste, no correctness gap. Effort: low-medium; risk: low.

*Two cheap riders worth doing alongside #1:* deploying message pinning (already built and tested
per `VEREVON_CHAT_DESIGN.md` §9.7, just not yet in the containers — closing that now, see §3) and
a message-search-over-raw-history RPC (the FTS index built for #1's fusion is most of what that
RPC needs once built).

---

## 3. What shipped from this list, today

**Item #1 — RRF-based recall ranking.** Implemented in
`apps/Model Plane/rust/services/session-core/src/dreaming.rs` and `src/memory_grpc.rs`:

- `search_agent_memory`'s SQL candidate pool widened from `== requested limit` (5–10 rows) to a
  flat `MAX_CANDIDATE_POOL = 200`. Live data proved the narrower pool insufficient even with a
  4× multiplier: in the running org, 21 of 24 memories share identical confidence, and a real,
  relevant memory ("User retrieves freight prices from shipping carriers") sits at rank 23 —
  outside any caller-scaled window, never reaching the app layer to be scored at all.
- `search_memory` now queries pgstore and the semantic backend (letta-bridge/agent-memory-server/
  Redis) **in parallel** via `tokio::join!`, unconditionally on any non-trivial query — not only
  as a shortfall backfill when pgstore under-returns, which in practice almost never happened
  (pgstore's `WHERE` has no relevance filter and always fills the limit once a user has that many
  memories).
- The two results are merged via genuine **Reciprocal Rank Fusion** (`score = Σ 1/(60+rank)` per
  list, ranks 1-indexed), fusing only the two genuine relevance signals — exact-match pgstore rows
  and the semantic backend's cosine-ranked results — with everything else in pgstore's
  confidence-ordered pool relegated to a last-resort fallback tier, never competing numerically
  against real relevance signals.
- RRF was chosen over this session's own earlier hard-bucket design specifically because 3
  independent external sources converged on it, and because it resolves the "confidence vs. cosine
  similarity are different units" problem more principled than any hand-tuned bucket order:
  it compares rank positions, never raw scores, and a fact confirmed by *both* signals earns a
  real, deterministic boost over one confirmed by only one.
- Verified: 6 new unit tests on the merge logic (deterministic under equal-score ties — no
  `HashMap`-iteration-order flakiness), plus the full `session-core` suite: **337 tests, 0
  failures**. Live-data spot check (read-only) confirmed the exact bug this closes.

**§4.4 of the cache audit — the two inert env vars.** Removed `SEMANTIC_CACHE_URL`,
`SEMANTIC_CACHE_TTL_SECS`, `SEMANTIC_CACHE_DATAPLANE_ENABLED` from both `docker-compose.yml` and
`apps/Model Plane/deploy/docker-compose.yml`, including the line that was interpolating a live
Dragonfly credential into a variable nothing read. Both compose files re-validated clean.

**Message pinning deployment.** Per `VEREVON_CHAT_DESIGN.md` §9.7, the feature was fully built and
tested across every layer (proto, session-core, model-gateway, frontend gateway, SPA) but the
running containers still carried pre-change binaries — "the one thing between this and done in
production." Rebuilding from the Model Plane onward (the same deploy carrying items above) closes
this.

**Not started this round:** items #2–#5. Each is scoped precisely enough above to pick up
directly; #2 and #3 are the natural next steps (#2 fixes the write-path root cause the ranking fix
above works around; #3 is pure unlock of already-built value).

---

## 4. Verification — 2026-09-17 (no code changed in this pass)

Every claim in §3 was re-checked against the live stack after the deploy landed, not against
build logs alone.

### 4.1 Item #1 is deployed and its dependencies are genuinely healthy

- `model-plane-session-core-1` runs an image built 08:02 UTC today (fresh `COPY rust/` +
  uncached `cargo build`, not a layer-cache hit); `model-gateway` 08:02, `verevon-gateway-rs`
  08:10, frontend 08:10. All healthy.
- The RRF fix now calls the semantic backend on **every** non-trivial query instead of only on
  shortfall, so it is only "optimally working" if that backend is actually up: `letta-bridge`,
  `agent-memory-server` and its Redis are all healthy, letta-bridge logs show zero errors, and
  session-core has logged **zero** memory-search degradation since the deploy. It is not silently
  falling back to pgstore-only.
- Live Postgres: `agent_memory` = 24 rows, `agent_skills` = **0**, `runs` = 184.
- **Honest limit:** no live chat turn has exercised the new recall path since the deploy (it is
  under an hour old). The fix is proven by 337 passing tests — including the six merge tests
  grounded in the org's real data shape — not yet by production traffic. `MODEL_GATEWAY_AUTH_DEV_
  BYPASS=0` in this environment, correctly, so no unauthenticated smoke request was fabricated.

### 4.2 Root-cause CORRECTION to item #3 (and to `LEARNING_SYSTEM_REDESIGN` §1b)

Both documents attribute `agent_skills = 0` "most plausibly" to no `RUN_COMPLETED` envelope ever
carrying an attested `zdr: false`. The live evidence points at a blocker **in front of** that gate:

- session-core's durable event log holds **147 `RUN_COMPLETED`** events (plus 184 `RUN_STARTED`,
  36 `RUN_FAILED`, 1 `RUN_CANCELLED`). Runs do complete, and are recorded.
- capability-core subscribes to `mp.v1.run.*.event` (`sessionreview/consumer.go:19`) and logs a
  ZDR skip at **INFO** level (`consumer.go:68`, "learning review skipped: run retention posture
  forbids derived persistence") — so if any event had arrived and been gated, the log would show
  it.
- capability-core's logs for the last 24h contain **zero** run-completed, retention, skip or
  review lines — only its startup line.

So the 147 completed runs never reached the reviewer at all. The ZDR gate is a second blocker
that has not yet had the chance to fire. What this pass could not distinguish: whether session-
core writes `RUN_COMPLETED` only to its Postgres event log and never publishes it to NATS, or
publishes it but capability-core is not receiving (different NATS server, subject mismatch,
consumer not actually subscribed). **Item #3's effort estimate changes accordingly** — from
"stamp a flag" to "trace the event path from session-core's event log to capability-core's
subscription first, then address the gate."

### 4.3 What is still missing, measured against the competitors these docs cite

| Capability the bar-setters have | Source | Verevon today |
|---|---|---|
| Write-time reconciliation before storing a memory (ADD/EXTEND/SUPERSEDE/NOOP) | mem0 (original), supermemory, hindsight, RetainDB | **Missing** — item #2, not started. Extractor still writes blind; the RRF fix ranks around the duplicates rather than preventing them |
| Per-memory correct / forget from where you see it | Claude, ChatGPT | **Missing** — chat shows "utledet" entries but offers only "Skjul" (`CHAT_PARITY_AUDIT` §3.8) |
| Approval-gated memory writes | Hermes `memory.write_approval`, byterover `curate → review` | **Missing** — no equivalent; capability-core's skill-review gate is the closest primitive |
| Standing derived profile, zero-cost to read | hindsight "mental models", Honcho | **Missing** — item #4, not started |
| Auto-generated skills from solved problems | Hermes `skill_manage`, byterover | **Built, never fired** — see 4.2 |
| Recency-aware ranking | (no reviewed source had a decay formula) | **Gap, unsolved everywhere** — confined to pgstore's fallback-tier tiebreak by design |

### 4.4 Next steps, in order

1. **Item #2** — non-destructive extractor grounding. It is the root cause; #1 is the workaround.
2. **Trace #3's event path** (4.2) before touching the ZDR gate — the cheapest possible check is
   whether session-core publishes `RUN_COMPLETED` to NATS at all.
3. **Per-memory forget control in chat** — smallest visible parity gap vs Claude/ChatGPT, and the
   natural first UI for item #4.
4. Item #4, then #5.

## 5. Item #3 event path traced end-to-end and fixed — 2026-09-17, later the same day

4.2 asked one question: does session-core ever publish `RUN_COMPLETED` to NATS, and if so, why
does capability-core never see it? Tracing that path live (not by reading the code in isolation)
found **five independent, compounding bugs** stacked in the path, each hiding the next. All five
are now fixed and deployed; a sixth, non-code blocker was reached and is unresolved. This is not
"item #3 done" — it is "item #3's actual scope," which turned out to be much larger than "stamp
a flag."

### 5.1 The five bugs, in the order they were found

1. **`sessionreview.RunConsumer` used a plain `nc.Subscribe`**, not a JetStream durable. A plain
   subscribe is fire-and-forget and only receives messages published while the process is
   connected — it cannot durably survive a restart or replay history. Fixed: rewritten onto
   `js.QueueSubscribe(..., nats.Bind(stream, durable), nats.ManualAck())`
   (`go/services/capability-core/internal/sessionreview/consumer.go`).
2. **No retry on the initial NATS connect.** `cmd/main.go`'s reconcile-events path called
   `nats.Connect` once; if NATS was not yet accepting connections at that instant (a real race on
   every cold start of this multi-service stack), the whole learning-review goroutine exited
   silently and never ran again for the life of the process. Fixed: `connectNATSWithRetry` (60
   attempts, 1s apart, mirroring nats-provisioner's own pattern).
3. **Two consumers were missing `DeliverSubject`/`DeliverGroup`** — `capability-core-skill-review`
   (new) and, discovered as a side effect, the already-shipped **AUTO-2 run-watch-notify**
   consumer (`runwatch.Notifier`), which had the identical latent bug and had never been caught
   because nothing had ever tried to bind to it either. A `ConsumerConfig` with no
   `DeliverSubject` is a server-side **pull** consumer; nats.go's push-style `Subscribe`/
   `QueueSubscribe` refuses to bind to one at all. Fixed in `nats-provisioner/main.go`, plus a
   self-heal path in `ensureConsumer` for consumers provisioned before this requirement existed.
4. **NATS ACL gaps for the JetStream calls `nats.Bind` and `msg.Ack`/`msg.Nak` make internally.**
   `nats.Bind` calls `$JS.API.CONSUMER.INFO` before it can subscribe; acking/naking publishes to
   `$JS.ACK.*`. `capability-core-runtime`'s grant covered the plain subject but neither of these —
   the bind hung until "context deadline exceeded" and the server logged a Permissions Violation
   that surfaces only on the connection's async error handler, never on the RPC caller. Fixed in
   `deploy/nats.conf` (see the comment block on `capability-core-runtime`).
5. **The `nats-provisioner` self-heal was too narrow.** Bug 3's fix had already been deployed once
   under an earlier `DeliverSubject` naming (`deliver.<durable>`) before it was renamed to the
   `_VEREVON.MODEL.DELIVER.<service>.<purpose>` convention used elsewhere in this file. The live
   consumers still carried the old name, and the original self-heal condition only matched an
   **empty** `DeliverSubject` (the pull-consumer case), so it left this drift to fail loudly by
   design. Broadened the self-heal invariant from "was pull-mode" to the more general and still-
   safe "zero prior deliveries" (`info.Delivered.Consumer == 0`) — proven both ways: it now heals
   a renamed subject with no history, and a new test (`TestEnsureConsumerRefusesToHealAfterReal
   Deliveries`) proves it still refuses once a consumer has actually delivered a message.

Each bug was reproduced in isolation against a disposable `nats:2-alpine` container pinned to the
exact production `nats.go v1.37.0` before touching production code, then proven fixed against the
real `deploy/nats.conf` with the real three-account credential separation
(`TestSkillReviewJetStreamACLGrant_ProductionConfig`). Deployed via `build-verevon-services.sh
--from model` plus a targeted `model-plane-nats-1` restart (nats.conf is a live-mounted `ro`
volume, not baked into the image) and a `model-plane-capability-core-1` restart. Live proof: the
"Permissions Violation" and "consumer stopped" log lines are gone, and `docker logs` shows
`learning-review consumer bound` followed by real message processing.

### 5.2 A sixth bug, found only once #1–#5 were fixed: auth-core registration drift

With NATS delivery finally working, the next failure was new: `capability-core could not obtain
a session-core credential`, `auth-core refused a session-core token ... Service principal is not
authorized`. `cmd/main.go`'s `sessionCoreScopes` requests `["session:read", "session:skills:write",
"session:schedule-prepare"]` as one bundled credential — but `config/plane-service-principals.json`
only granted capability-core the first two against the `session-core` audience.
`session:schedule-prepare` is real and exclusively capability-core's
(`rust/services/session-core/src/grpc.rs:3960`, `prepare_scheduled_run_thread`, gated to
`service:capability-core` only) — it was simply never added to the registry when this scope set
was written, because nothing had ever successfully exercised this credential before (bugs #1–#5
blocked it every time). Fixed by adding the scope to both `scopes` and `scopesByAudience.session-
core` in `plane-service-principals.json`, then `run-control-plane.sh up -d --no-build auth-core` —
which reassembles `PLANE_SERVICE_PRINCIPALS_JSON` from that file and recreates only `auth-service`
(the loader re-reads it per-request, so no code changed, only the policy). Verified: the
credential-mint failures are gone from capability-core's logs.

### 5.3 A seventh bug: the ZDR gate, and a genuine design inconsistency it exposed

With #1–#6 fixed, the review call to inference-core failed with:

> `learning: skill review failed: llmreviewer: infer call: rpc error: code = FailedPrecondition
> desc = no matching provider deployment has verified ZDR support`

`plane-service-principals.json` set capability-core's retention posture for the `inference-core`
audience to `"zdr"` (its other three audiences are `"persistent"`), and separately,
`llmreviewer/reviewer.go` hardcoded `Zdr: true` on every review call regardless of that posture.
`inference-core`'s ZDR gate (`rust/services/inference-core/src/provider/zdr.rs`) is an
evidence-bound attestation, not a boolean — a real Azure resource ID, retention-exception approval
reference, dates, reviewer, and a SHA-256 digest binding all five. No environment in this checkout
has one configured, correctly — this is not something to fabricate.

Presented as a decision, not fixed unilaterally: **`sessionreview.HandleRunCompleted`'s own
retention gate (§ "Zero Data Retention gate" in `consumer.go`) already refuses to reach the review
call at all unless the run's envelope declared an explicit `zdr: false`** — i.e. the org/user
already consented to Verevon deriving and persisting artifacts from this exact conversation.
Requesting a *second*, independent ZDR guarantee for the LLM call itself, on top of that
already-checked consent, protects nothing the upstream gate hasn't already cleared — it just adds
a requirement no environment without a real Azure retention exception can ever satisfy. Confirmed
with the user and changed both: `retentionByAudience.inference-core` → `"persistent"`, and
`reviewer.go`'s `Zdr: true` → `false` (with `reviewer_test.go`'s assertion flipped to match, and a
comment on each explaining the safety argument). Redeployed and reset the backlogged consumer:
the ZDR error is gone.

### 5.4 An eighth bug, found only once #7 was fixed: `DefaultModel` isn't a real Azure deployment

Past the ZDR gate, the call failed differently:

> `rpc error: code = Unavailable desc = all providers exhausted after 3 attempts`
> (inference-core logs: `azure-anthropic` → `404 DeploymentNotFound: claude-sonnet-4-20250514`)

`llmreviewer.DefaultModel` is `"claude-sonnet-4-20250514"` — which is also
`inference-core::provider::anthropic::DEFAULT_ANTHROPIC_MODEL`, the system's own "Verevon Auto"
fallback for the `anthropic`/`azure-anthropic` providers. This environment's real deployment
catalog (`AZURE_ANTHROPIC_DEPLOYMENTS` in `deploy/.env`) is `claude-haiku-4-5, claude-sonnet-4-5,
claude-sonnet-4-6, claude-opus-4-8` — **`claude-sonnet-4-20250514` is not in it and 404s every
time.** This is bigger than skill-review: it means any "Verevon Auto" request that resolves to
`azure-anthropic` with no explicit model would hit the identical 404 — worth a separate, dedicated
look, out of scope for this pass, and not touched here (it is a shared constant with call sites
across the whole Model Plane, not something to change on a guess).

For skill-review specifically, `LEARNING_REVIEW_MODEL` already existed as a purpose-built,
narrowly-scoped override — just never wired into Compose. Added it to `deploy/docker-compose.yml`
for the `capability-core` service, defaulting to `claude-sonnet-4-5` (a real, cataloged
deployment). Redeployed and reset the consumer: the 404 is gone, and inference-core now returns a
real model response for the first time in this pipeline's history.

### 5.5 Where it stands: a ninth issue, not yet root-caused

The model reply itself now fails to parse:

> `learning: skill review failed: review response contained no JSON object`

This is **not** the stale-test-data hypothesis it first looked like — one of the stuck runs'
transcripts is 5,127 characters of real conversation content, not an empty/synthetic fixture.
`ParseReviewResponse` (`internal/learning/reviewer.go`) requires a `{...}` span somewhere in the
reply and errors otherwise; every one of the 8 backlogged messages hits this deterministically
(not a transient/rate-limit issue — `num_redelivered` climbs to `MaxDeliver: 5` and stops on every
retry). The raw model reply is not currently logged anywhere, so the next step is to log it
(temporarily or behind a debug flag) and inspect one directly — is the model replying in prose
despite `ReviewPrompt`'s instructions, is `reviewMaxTokens: 2048` truncating a longer JSON reply
before it closes, or something else. Not root-caused in this pass.

**Cumulative status: `agent_skills` is still 0 rows**, but the pipeline now has verifiable, live
proof of every link working up to and including a real inference-core call returning a real model
response — NATS delivery, ACL, auth-core credential, transcript fetch, ZDR posture, and model
routing are all confirmed correct. What's left is entirely inside `internal/learning`'s response
handling, the smallest and most self-contained piece of this whole chain.

A separate, smaller finding along the way: the review handler's `msg.Nak()` on failure carries no
backoff, so under any *sustained* downstream failure the 10 backlogged messages each exhaust
`MaxDeliver: 5` in well under a second and stop being retried until a fresh event arrives or the
consumer is recreated. That is a real reliability gap worth a `NakWithDelay` or backoff policy,
independent of everything above — not fixed in this pass, noted for the next one.

## 6. Item #2 — non-destructive extractor grounding, implemented and verified — 2026-09-17

Ground truth first, precisely: `dream_once` (`session-core/src/dreaming.rs`) runs two extractors on
every pending message — a phrase-matcher (`extract_memory_candidates`) and an LLM extractor
(`DreamExtractor::extract`, `claude-haiku-4-5`) — both funneling into one `upsert_agent_memory` with
a real `ON CONFLICT (org_id, scope, owner, key)` unique constraint. The constraint is real, but
`key` is minted from free-form/model-chosen text (an LLM-picked `slot` name, or a literal slugify of
a captured phrase) with no cross-call normalization, so semantically identical facts legitimately
mint different keys and sail past the constraint as distinct rows — the exact "same fact stored 6
times" bug, now root-caused at the code level rather than inferred.

**Implemented**: `ground_candidates`/`ground_one_candidate` in `dreaming.rs` run an exact-key tier,
then a normalized-key-fragment tier, then a Letta semantic-search tier, then an LLM classifier
(`memory_grounding.rs`, new, 777 lines) that decides ADD / EXTEND / SUPERSEDE / NOOP. A SUPERSEDE
or EXTEND never overwrites: `write_lineage_memory` flips the old row's new `is_latest` column to
`false` and inserts a new row with `supersedes_id` pointing at it, both in one transaction — proven
by direct SQL read, no `DELETE` anywhere in the path. The three read queries (`search_agent_memory`,
`load_agent_memory_context_rows`, `list_user_memory`, including the RRF fix from §3 above) now
filter `is_latest = true`. Migration `0037_agent_memory_lineage.sql` adds the two columns,
forward-only per this service's own convention (zero down-migrations across 37 prior migrations).

The whole feature is gated behind `MEMORY_GROUNDING_ENABLED` (default **off**, following the exact
`env_flag` convention `DREAMING_LLM_EXTRACTION` already established in this file) — the flag-off
path was proven **byte-for-byte identical** to pre-change behavior by diffing `persist_candidates`'s
function body against `git show HEAD`, not by trusting a comment, and a test in the suite itself
(`persist_candidates_itself_has_no_grounding_dependency`) `include_str!`s the file and asserts the
function contains no grounding reference at all. The flag had been implemented in Rust but never
actually wired to anything operable — `deploy/docker-compose.yml` never passed
`MEMORY_GROUNDING_ENABLED`/`MEMORY_GROUNDING_MODEL` into the session-core container, so it could
never be turned on in the deployed stack; both are now wired (still defaulting off) alongside the
existing Dreaming-extractor env vars.

**One caveat worth operator awareness, not a defect**: the `is_latest = true` predicate was added
to the three read queries *unconditionally*, not gated by the flag. This is safe and proven
semantically inert while the flag is off (every row defaults `is_latest = true` and nothing ever
flips it except the grounding-only write path) — but it is a real, live code-path change on the read
side regardless of the flag, and becomes load-bearing the moment the flag is turned on anywhere.

**Verified independently** (a second, adversarial pass, not the implementer's own report):
`cargo check -p session-core` clean; full suite **364 passed, 0 failed, 21 ignored** (the 21 are
pre-existing `DATABASE_URL`-gated integration tests, unrelated) — up from 337 pre-grounding, with
27 net new tests counted directly by diffing test functions per file (`memory_grounding.rs` +20,
`dreaming.rs` +7); the migration is real and idempotent (`IF NOT EXISTS` guards); the SUPERSEDE/
EXTEND transaction genuinely preserves the old row; and the RRF fix's own 6 merge tests are
structurally immune to the read-side filter change (they construct rows in-process and never touch
SQL, confirmed by reading each one). A second, unrelated, already-bundled fix in `dream_extractor.rs`
(stripping pasted-document blocks out of the extraction window, closing a real 2026-09-14 incident
where a demo document got recorded as a fact about the user) rode along in the same uncommitted
diff — noted here so it isn't mistaken for part of the grounding feature itself.

**Not committed** — all of this remains in the working tree, per this repo's standing "commit only
when explicitly asked" rule.

**Next step for item #2**: turn `MEMORY_GROUNDING_ENABLED=1` on in a real environment and observe
whether the "same fact under six keys" pattern actually stops recurring — nothing in this pass
exercised the feature live against a real conversation, only unit-level proof that its logic and
safety properties are correct.

## 7. Skill-review parse fix, memory lineage fixes and the dedup tool — 2026-09-24

Two read-only investigations ran on 2026-09-23, one on skill review and one on memory duplicates.
The fixes followed on 2026-09-24, then a cross-cutting review. **Nothing in this section is
deployed.** capability-core, session-core, letta-bridge and the Frontend Plane changes ship with
the launcher rebuild (`build-verevon-services.sh --from model`), and then need live checks. No
live skill review with the new code, and no dedup run against the live database, has happened.

### 7.1 §5.5 root-caused: why the review reply had no JSON

- The review prompt of 2026-09-17 never asked for JSON. It ended with "Output only the skill
  candidates you are confident improve future performance." There were no tools, and the JSON
  shape existed only in a Go comment the model never sees. Claude answered in prose, and
  `ParseReviewResponse` found no `{`.
- It looked deterministic because §5.3 set `Zdr: false`, which turned on inference-core's 300 s
  response cache for the review call. The instant `Nak` redeliveries were served the same cached
  reply.
- That was already fixed on 2026-09-19 in the running image. The prompt requires a JSON object,
  the model must answer through the `submit_skill_review` tool with a schema, `MaxTokens` is 4096,
  and truncation and refusal are detected. §5.5's `reviewMaxTokens: 2048` question is answered.
- **A second parse bug was still live.** `extractJSONObject` (`internal/learning/reviewer.go`)
  cut the reply at the first triple backtick anywhere, including inside a skill's own text, and it
  also ran on tool-call arguments. A skill containing a fenced `bash` block or one inline triple
  backtick failed with "review response contained no JSON object". A fenced `json` example failed
  as "schema invalid". This was reproduced against the repo code with `go test -overlay`. The
  first useful procedural skill with a code block would have been retried and then dropped.
- §5.5's backoff gap is closed. `NakWithDelay` retries after 30 s, 2 min, 10 min and 30 min, and
  configuration and permission errors are terminated instead of retried.

### 7.2 Live state on 2026-09-23 (read-only)

- capability-core logged 319 "learning review completed" since 2026-09-19 14:15Z, all with
  `skills_persisted: 0`. "no JSON object" appeared 0 times.
- 309 of those 319 runs were in conversation-only threads. Those reviews return before calling the
  model, but were logged as "completed". Only 10 reviews reached the model, all on 2026-09-19, all
  2-message chats. None errored and none persisted a skill.
- Every thread created since 2026-09-20 is conversation-only (240 threads). The recording and e2e
  journeys choose "Kun samtalen", while the UI default is workspace (`DashboardComposer.tsx`). So
  no recorded run is reviewed for skills.
- `agent_skills` = 0 rows. The JetStream consumer was fully caught up: 0 pending, 0 redelivered.

### 7.3 Fixes F1–F6 (capability-core)

- **F1, parser** (`internal/learning/reviewer.go`). A new strict `ParseReviewJSON` handles tool-call
  arguments whenever a tool call is present, and reply text next to a tool call is ignored. The
  text path tries a JSON decode at each `{` and returns the first complete object with a `skills`
  key. It no longer slices on backticks or braces, and its total decoding work is bounded: a
  hostile 64 KiB reply of unclosed nesting went from 4.1 s to about 1 ms. Fenced `bash`, inline
  triple backticks, fenced `json` and brace text now parse. Truncated objects, `{}`, bad
  candidates and oversized replies are still rejected.
- **F2, diagnostics.** Parse failures return a typed `ParseError` with a reason: `no_json`,
  `syntax`, `missing_skills`, `candidate_invalid` or `size`. `llmreviewer.ResponseError` logs the
  source (tool or text), the raw length and the reason, never reply content. The skillsink upsert
  error no longer quotes the model-written skill name.
- **F3, cache.** The review call sends gRPC metadata `cache-control: no-store`. inference-core
  honours it and neither reads nor writes its response cache for that call.
- **F4, visibility.** A skipped run logs "learning review skipped" with `reason` =
  `conversation_scope`, `empty_transcript`, `retention_posture` or `provider_refusal`. "learning
  review completed" is logged only when the model was called, now with `candidates` and
  `selected` next to `skills_persisted`. The retention-skip line quoted in §4.2 no longer exists in
  that form.
- **F5, timeout.** Each message first gets `msg.InProgress()`, then a review deadline of AckWait
  minus 20 s (100 s of the provisioned 120 s). A review that hits the deadline is retried with a
  delay, never terminated. A slow review can no longer be redelivered mid-flight and reviewed
  twice.
- **F6, limit.** `RunConsumer` reads AckWait and MaxDeliver from the consumer info at bind time.
  The hard-coded 5 is gone. If the read fails, it falls back to 120 s and 5 with a warning.
- Side fix: `GET /api/v1/tasks` caps `limit` (0 or less, or over 200, becomes 50).
- The full capability-core suite passes (31 packages with tests). The new parser tests failed
  against the old parser (3 tests, 7 subtests). The live-NATS tests were not run.
- Live check after the rebuild: a multi-turn chat in workspace scope (not "Kun samtalen") that
  teaches a procedure with a code block. Expect no "cache hit" for the review in inference-core,
  `agent_skills` above 0 with origin `background_review`, and 0 redeliveries.

### 7.4 Correction to §5.4: the default model

§5.4 says `DEFAULT_ANTHROPIC_MODEL` (`claude-sonnet-4-20250514`) is the "Verevon Auto" fallback for
both `anthropic` and `azure-anthropic`, so any unpinned azure-anthropic request would 404. That is
wrong.

- `DEFAULT_ANTHROPIC_MODEL` applies only to the direct `anthropic` provider
  (`inference-core/src/provider/fallback.rs`, `default_model_for`).
- `azure-anthropic` has its own default, `ProviderDefaults.azure_anthropic`. It is
  `claude-haiku-4-5` (`DEFAULT_AZURE_ANTHROPIC_MODEL`) when that name is in
  `AZURE_ANTHROPIC_DEPLOYMENTS`, and otherwise the first configured deployment
  (`azure_default_deployment`). An unpinned azure-anthropic request therefore resolves to a real
  deployment.
- The 404 in §5.4 came from capability-core itself: `llmreviewer.DefaultModel` sends
  `claude-sonnet-4-20250514` as an explicit model. The Compose default
  `LEARNING_REVIEW_MODEL=claude-sonnet-4-5` still covers that.
- The separate look §5.4 asked for is not needed for azure-anthropic.

### 7.5 Memory duplicates: live state on 2026-09-23 (read-only)

- Migration 0037 is applied (2026-09-17 21:35:34 UTC) and its checksum matches the file. No row
  uses the lineage yet: 51 of 51 rows are `is_latest = true` and none has `supersedes_id`.
- 8 near-duplicate clusters hold 20 rows. 12 of them could be retired, taking 51 current rows to
  39. The clusters were found with a lexical score and checked by reading. No classifier calls
  were made.
- The biggest clusters restate the product-recording journey prompts: drafts only, source notes in
  a separate internal note, the Fjordform pilot. The extractor never saw the existing slot names,
  so every new thread minted new keys.
- The set is frozen for now. Every new thread since 2026-09-19 15:00 is conversation-scope, and
  Dreaming skips those (782 `skipped_conversation_scope` runs).
- Grounding is off in `model-plane-session-core-1` (`MEMORY_GROUNDING_ENABLED=0`).

### 7.6 `MEMORY_GROUNDING_ENABLED` is still off, and enabling it would not clean these rows

The flag stays at its Compose default of `0` (`apps/Model Plane/deploy/docker-compose.yml`).
Turning it on would not remove the existing duplicates:

1. It runs only on new background candidates. Nothing re-reads stored rows, so all 12 redundant
   rows would stay current.
2. Its key tier matches none of the eight clusters. Run over the live keys, it matches one pair,
   `employer`/`employer_sector`, which is not a duplicate.
3. Its semantic tier for user scope asks for the 5 most recently updated entries, not the 5 most
   similar (`list_detailed(org, user, 5)`). With the flag off, user-scope rows were never mirrored
   to the semantic store, so today's duplicates are probably not there to find. (The semantic
   store was not queried.)
4. Other write paths stay ungrounded: the AppendMessage phrase matcher, manual
   `save_memory`/IndexMemory, and "Rediger".
5. Candidates from the same extraction are not compared with each other.

The path for existing rows is the dedup tool (7.8).

### 7.7 Lineage and deletion fixes (session-core, letta-bridge, BFF, SPA)

1. **Retired rows no longer come back through the semantic store.**
   `memory_control::filter_forgotten` also drops semantic entries whose durable row is retired
   (`is_latest = false`). That covers SearchMemory, ListMemory, grounding's semantic tier and chat
   context assembly. A lineage write now also removes the retired row's mirror.
2. **DeleteMemory deletes what the caller owns.** `delete_user_memory` returns Deleted, Absent or
   NotOwned, with the ownership check inside the SQL: user-scope rows the caller owns, and
   thread-scope rows in the caller's own threads. Before, a "deleted" thread-scope row survived in
   Postgres and kept being injected, while the RPC said `deleted: true`. Retired ancestors go with
   the row (a recursive CTE with a CYCLE guard), and each is recorded as forgotten. NotOwned
   returns NOT_FOUND without touching the semantic tier.
3. **letta-bridge Delete is scoped** to the caller's org and user, checked with a GET first, and
   `deleted` comes from the server's reply. Before, it could erase another user's memory by id and
   reported erasures that never happened.
4. **Writes on a retired key go to the lineage head**, or revive an orphaned row, so a correction
   cannot disappear into a hidden row. They are counted in
   `mp_session_memory_retired_key_writes_total`.
5. **The extractor sees the existing slot names**: the user's and the thread's current `llm` slot
   names (names only, newest 40 per scope), with one instruction to reuse them.
6. **"Rediger" and "Glem" report what happened.** The BFF `correct_memory` response gains
   `original: removed | not_found | not_removed`, and the SPA keeps the original row when it was
   not removed. The cross-review also stopped recalled-memory text from being saved in browser
   storage, so a forgotten memory cannot come back on reload.

session-core passes 370 unit tests plus 12 DB tests run on a throwaway Postgres 16.14 container.
letta-bridge passes 61. None of it ran against the live database.

### 7.8 The `session-core memory-dedup` tool

A subcommand of the session-core binary. It opens only a database pool: no gRPC, HTTP, NATS,
workers or migrations.

- **`plan`** (the default mode) runs in a `REPEATABLE READ READ ONLY` transaction and refuses to
  run without migration 0037. It compares current, accepted, unexpired, non-forgotten rows inside
  their unique-index partitions and keeps the top 5 lexical neighbours per row. It writes
  `plan.json` to `/tmp`: ids, keys, content SHA-256, `updated_at`, before-state and evidence, but no
  content. `--classifier off` is deterministic and puts every cluster in the review tier. The
  default reuses the grounding classifier (`claude-haiku-4-5` through inference-core) without
  turning grounding on. A cluster is auto tier only if every retired row is a NOOP against the
  survivor.
- **Survivor rule:** a user-stated row beats an unknown one, which beats an inferred one; then the
  newest `updated_at`, higher confidence, longer content, then id. It ignores `kind`, so a cluster
  whose survivor should be a preference-kind row needs a manual swap in the plan.
- **`apply --plan <file>`** runs one transaction per auto-tier cluster. It locks the rows and skips
  the cluster as stale if anything changed since the plan. It flips `is_latest`, chains
  `supersedes_id` only where it is empty, and never touches `updated_at` or content. It checks
  that exactly one current row remains and there is no cycle, then writes a `dream_runs` ledger
  row (`trigger = memory_dedup_backfill`, ids and booleans only). A re-run skips clusters already
  applied. `--dry-run` reports and rolls back.
- **`rollback --plan-id <id> [--cluster <id>]`** works from the ledger and reverts only rows still
  in their after-state.

**Procedure, dry run first.** Only after the launcher rebuild; not yet run live.

1. `docker exec -e RUST_LOG=warn model-plane-session-core-1 session-core memory-dedup plan
   --classifier off`. The `-e RUST_LOG=warn` matters: the container logs at debug to stdout.
   Add `--show-content` for previews. Copy the plan out with `docker cp`.
2. A person reviews `plan.json`, removes clusters or swaps survivors, and sets the chosen clusters'
   tier to `auto`.
3. `session-core memory-dedup apply --plan <file> --dry-run`, and read the report.
4. `session-core memory-dedup apply --plan <file>`.
5. If needed, `session-core memory-dedup rollback --plan-id <id>`, optionally with `--cluster`.

Caveat: a DeleteMemory after an apply also removes retired ancestors, and a later rollback cannot
bring them back. It reports them as missing, by design.

### 7.9 Next

1. After the rebuild: the skill-review check in 7.3, and a dry-run `plan` against the live
   database.
2. Open product question: whether any recording journey should use workspace scope. Today no
   recorded run is reviewed for skills, and Dreaming skips every conversation-scope thread.
