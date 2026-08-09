# Agent Quality, Learning & Efficiency Plan — 2026-07-29

Every defect below was **verified in code**, not inferred from docs. The recurring pattern in this
codebase is subsystems that are fully built on both ends with a disconnected middle — so each phase
states the *specific missing link*, and no phase is "done" until it is observed working live.

**Rule for this plan:** a phase is complete only when (a) tests pass, (b) it is deployed, and (c) the
behaviour is observed in a real chat turn or a real log line. "Compiles and looks right" is not done.

---

## Phase 0 — Chat has NO Data Plane grounding in the deployed config (added after verification; do this first)

A follow-up sweep found the chat context path is far emptier than assumed. Three independent faults
stack, and the first is **three lines of compose**.

### 0.1 session-core never receives the Data Plane addresses — retrieval, knowledge AND graph tiers are all silently empty

**Verified.** session-core reads `DATAPLANE_RETRIEVAL_ADDR` / `DATAPLANE_GRAPH_ADDR` /
`DATAPLANE_KNOWLEDGE_ADDR` (`grpc.rs:2786,2802,2820`). The `session-core:` block in
`deploy/docker-compose.yml` contains **zero** `DATAPLANE_*` variables (confirmed by count). The
`DATAPLANE_RETRIEVAL_URL` / `_GRAPH_URL` / `_WIKI_URL` vars that DO exist in compose belong to
**model-gateway** (`:242,247,248`) and **execution-core** (`:607`). model-gateway accepts both
spellings (`state.rs:443-464` tries `_URL` then `_ADDR`); session-core only ever accepted `_ADDR`.

So session-core resolves to `localhost:50052/50053` inside its own container. `connect_lazy()` means
the clients are `Some(...)` and the code path *looks* wired, but every RPC fails at request time and is
swallowed by deliberate degrade-to-empty branches (`grpc.rs:2370`, `:2424`, `:2538`). **Nothing errors;
answers just arrive ungrounded.**

Worse, it cannot self-heal: the `RETRIEVAL` fallback rows in `memory_index` are written only when
`evidence.source == RetrievalSource::DataPlane` (`grpc.rs:1489-1492`) — i.e. only after a *successful*
Data Plane call. The local fallback can never bootstrap.

**Fix:** add the three `DATAPLANE_*_ADDR` vars to the session-core block pointing at the
`inter-plane-bus` aliases (`dpv2-retrieval-engine:50052`, `dpv2-graph-index:50053`), **and** give
session-core the same `_URL`→`_ADDR` fallback model-gateway has, so the two services can never drift
apart again. Verify by observing non-empty `retrieval`/`knowledge` segments in a real turn — not by
reading config.

### 0.1b session-core sends NO credential to the Data Plane — found only by deploying 0.1

**Live-observed after deploying 0.1.** With the addresses fixed, session-core finally *calls* the
Data Plane — and every call is rejected:

```
WARN retrieval service call failed, using local fallback
     status: Unauthenticated, message: "invalid or missing credential"
WARN graph contradictions call failed
     status: Unauthenticated
```

`grep` confirms session-core attaches no `authorization` metadata to any of the three clients. The
Data Plane interceptor (`retrieval-engine-rs/src/grpc/interceptor.rs:50-79`) requires an **RS256 JWT**
with `exp, nbf, aud, iss, sub, zdr` + non-empty `org_id` — not a shared internal key. So this grounding
path has never worked; 0.1 only moved the failure from *silent* to *visible*.

**This is the value of the deploy-and-watch rule.** Source review said 0.1 was the fix. It was
necessary but not sufficient, and nothing short of a real turn would have shown that.

**Constraint on the fix:** forward the **verified caller bearer**, mirroring
`model-gateway/src/retrieval.rs:610-614` and its test
`data_plane_grpc_authorization_uses_only_verified_bearer`. Do NOT mint a broad service token — Data
Plane retrieval enforces per-user, private-until-shared authorization keyed on the JWT's `sub`/`org_id`,
so a service identity would collapse every user's view into one and reintroduce the cross-tenant leak
class. (The `SESSION_CORE_SERVICE_*` mint in `letta_adapter.rs` is right for letta-bridge, which is not
user-scoped, and wrong here.)

### 0.2 The properly-wired grounding path (with GraphRAG + citations) is gated OFF

**Verified.** `model-gateway/src/retrieval.rs` contains a *second*, correctly-configured grounding
implementation that DOES include GraphRAG — `retrieve()` runs `Retrieve` and
`POST /v1/retrieve/graph` (entities + community summaries) concurrently and produces real citations.
It runs only when `explicit_grounding_requested` (features contain `rag`/`knowledge`) **or**
`!used_context_assembly` (`sse.rs:744-767`).

The SPA sends neither flag (`DEFAULT_FEATURES` = usage/citations/reasoning/steps/artifacts + tools;
`citations` explicitly does not match). And `used_context_assembly` is effectively always true, because
the `user:<id>` fallback segment alone makes `emitted > 0`. **The good path is skipped because the
broken path "succeeded".** Consequence: `grounded` is always false (`sse.rs:1462`, `:2637`), so
knowledge-grounding never reaches the confidence score either.

**Fix:** make the gate depend on whether assembly produced *real grounding content*, not merely any
segment. A fallback id segment must not count as grounding.

### 0.3 The graph tier is dead even after 0.1

**Verified.** The only writer of `graph_claims` (`graph-index-rs/src/store.rs:158-162`) **omits
`contradicted_by_claim_ids` from its INSERT column list**, no `UPDATE` ever sets it, and the extractor
prompt has no contradiction step. `GetContradictions`' read predicate
(`jsonb_array_length(COALESCE(contradicted_by_claim_ids,'[]')) > 0`) is unsatisfiable outside test
fixtures. Also note it queries **Postgres, not Neo4j** — Neo4j serves only the HTTP
`/v1/graph/traverse` route, and `graph.proto` has no `Traverse` rpc.

### 0.4 The prompt says "wiki" but no wiki content is ever supplied

The consumer preamble (`sse.rs:1700`) instructs the model to *"Treat retrieved, wiki, graph, and memory
content as evidence"*, but **no wiki segment is ever produced** — `WikiService` (`:50054`) is absent
from the chat path entirely. Either supply wiki content or stop claiming it in the prompt; telling a
model to rely on evidence that never arrives is a hallucination invitation.

### 0.5 No cache helps a streaming chat turn — correcting an earlier statement

- **`PromptCache` never serves streaming chat.** `get`/`put` are called only from `infer()`
  (`fallback.rs:701,741`); `infer_stream()` (`:924`) never touches it. Chat streams, so it only ever
  applies to the non-streaming fallback, `direct_infer`, and the tool loop.
- **A real semantic cache exists, is Dragonfly-backed, enabled by default in compose
  (`SEMANTIC_CACHE_URL`, TTL 3600s) — and has ZERO callers in `sse.rs`.** Every caller is on the gRPC
  `ModelGateway::invoke` path. Its vector tier is hard-disabled in code: `DataPlaneCache::from_env`
  returns `None` unconditionally regardless of config.
- DP2 has its own Qdrant semantic cache (`cache/semantic.rs`, min_score 0.95) with zero callers in the
  orchestrator, and a Dragonfly query-embedding cache that only lives *inside* `Retrieve` — which chat
  cannot currently reach.

**This substantially changes Phase 4:** the infrastructure for CAG already exists and is wired to
Dragonfly; the work is mostly *reaching* it from the SSE path with correct tenant scoping, not building
it. Re-scope Phase 4 accordingly.

### 0.6 Latent: one Cohere outage flips every answer to low-confidence

`low_confidence = reranked.first().rerank_score < 0.35` (`orchestrator.rs:960`), but `rerank_score` is
initialised to `0.0` by every retrieval arm and RRF only writes `final_score`. Only `RerankClient` sets
it, and rerank failure is deliberately non-fatal. So an unset `COHERE_API_KEY` or any Cohere outage
silently marks **every** turn low-confidence. Currently masked because the live `.env` has the key.

Also dead-but-harmless (assigned, never read): `reranker_model`, `query_expansion`, and
`context_format: "toon"` — there is no TOON serializer in the retrieval path at all.

---

## Phase 1 — Turn on what is already built (highest ROI, lowest risk)

### 1.1 Publish `RUN_COMPLETED` to NATS → activates the whole skill-learning loop

**Defect.** capability-core `sessionreview/consumer.go:19` subscribes `mp.v1.run.*.event` and filters
`RUN_COMPLETED`; the LLM reviewer + `skillsink` → `UpsertAgentSkill` write path and model-gateway's
`sse.rs:817 fetch_skill_context` read path (top-3 injection per turn) are both live. **Nothing
publishes the trigger.** session-core `grpc.rs:684 record_run_terminal` only INSERTs into the Postgres
`events` table; `terminalization.rs`'s outbox is Postgres-only; model-gateway publishes to that subject
but only `TRAJECTORY_RECORDED`. The sole NATS publisher is orchestrator-core
`activities.go:318 CompleteRunActivity` — a Temporal activity — and nothing in production starts any
workflow (`ExecuteWorkflow|StartWorkflow` outside tests = zero).

**Fix.** Publish a `RUN_COMPLETED` envelope from session-core's terminal path, on
`mp.v1.run.<run_id>.event`, carrying `event_type`, `org_id`, `run_id`, and `thread_id` in the payload
(the consumer reads `thread_id` at `trigger.go:74` and fetches the transcript via `ListConversation`).
Reuse the existing `audit_publisher.rs` NATS connection pattern; publish **after** the DB commit and
treat failure as non-fatal (the run is already durable — a lost review must never fail a user's turn).

**⚠ Decision required (cost).** This fires an LLM transcript review on *every* completed chat turn.
At current volume that is an extra inference call per turn. Ship it **gated**:
`LEARNING_REVIEW_ENABLED` (default on) plus a sampling/eligibility rule — recommend reviewing only
turns that (a) used at least one tool, or (b) exceed N messages. Rationale: a one-line "hei" turn has
no skill to extract, and reviewing it is pure cost.

**Verify.** Deploy; send one tool-using chat turn; confirm in capability-core logs
`learning review persisted skills` with a non-zero count, then confirm the skill appears in
Settings → Ferdigheter, then confirm a *subsequent* turn injects it (model-gateway debug log for
skill context).

### 1.2 Repair the feedback contract end to end

**Defect (four independent breaks).**
| Hop | Sends / Expects | Break |
|---|---|---|
| verevonv3 `chat-client.ts:753` | `{requestId, rating:'positive'\|'negative', note}` | — |
| model-gateway `http_routes.rs:4983` | `{run_id, skill_id, rating:'good'\|'acceptable'\|'poor'}` | **422**: `requestId`≠`run_id`, no rename/default; `skill_id` absent |
| `ChatPage.tsx:133` | `.catch(() => undefined)` | failure silently swallowed — UI shows the thumb, nothing persisted |
| orchestrator-core `feedback.go:64` | counts only `rating=="good"` | `'positive'` raises `total` but never `good` → a thumbs-**up** *lowers* the score |
| orchestrator-core `feedback.go:52` | `if skillID=="" { return }` | dropped |
| `FeedbackStore` `feedback.go:16` | `sync.Mutex` + `map` | **in-memory**, wiped on restart |
| `FeedbackPromotionWorkflow` | registered `cmd/main.go:95` | **never started** |

**⚠ Decision required (semantics).** A chat turn has no `skill_id`. What does a rating attach to?
Recommendation: **both** — `run_id` for answer quality, and the ids of the skills that were *injected*
that turn (available from `fetch_skill_context`), so a thumbs-down demotes the skills that steered a
bad answer. That is what closes the loop to promotion/demotion; rating the run alone only produces a
dashboard number.

**Fix.** Align the contract (map `requestId`→`run_id`, make `skill_id` optional, normalise the rating
vocabulary in ONE place with a test pinning the mapping), stop swallowing the error in the UI (surface
a quiet toast on failure), make `FeedbackStore` Postgres-backed, and either start
`FeedbackPromotionWorkflow` on a schedule or replace it with a simple durable aggregate the read path
can query. Note `PromoteSkill` (`capability-core/internal/server/server.go:532`) is *deliberately*
hard-failed whenever a durable store is attached, with a test locking that in — decide whether to
un-quarantine it or aggregate demotion-only for now.

**Verify.** Click thumbs-down in chat → row lands in Postgres with the run id and injected skill ids →
no 422 in model-gateway logs.

---

## Phase 2 — Cost & latency (large, mechanical wins)

### 2.1 Anthropic prompt caching — the single biggest cost lever
**Defect.** Zero `cache_control` in `provider/anthropic.rs`. On a 12-round tool loop the system prompt,
tool definitions, and the whole accumulated history are re-sent at full price **every round**.

**Fix.** Mark stable prefixes with `cache_control: ephemeral` — system prompt + tool definitions
first (they are byte-identical across rounds of a turn), then the message-history prefix. Respect
Anthropic's minimum-cacheable-tokens threshold and breakpoint limit.

**⚠ COMPLIANCE GATE — must not be missed.** Prompt caching means the provider retains the prompt
prefix server-side (minutes). That is retention. **It must be disabled when `req.zdr` is true**, the
same way `cache.rs` already bypasses the local cache on both read and write. Add a test asserting a
ZDR request sends no `cache_control`.

**Verify.** Provider usage fields report `cache_creation_input_tokens` / `cache_read_input_tokens` on
round 2+ of a real multi-round turn.

### 2.2 A real tokenizer (unblocks every budget decision)
**Defect.** Token accounting is `bytes/4` (`session-core/grpc.rs:2612,2623`); no `tiktoken`/`tokenizers`
anywhere. Systematically wrong for Norwegian `æ/ø/å`, and context assembly + compaction thresholds all
ride on it. **Do this before tuning any budget**, or you are tuning against a broken ruler.

### 2.3 Parallel tool dispatch
**Defect.** `tool_loop.rs` has no `join_all`/`FuturesUnordered` — N independent lookups serialise into
N× latency, all of it before first byte. Fix: execute the model's concurrent tool calls in parallel,
preserving per-call audit and the existing duplicate-suppression.

### 2.4 TOON on tool outputs (evaluate, don't assume)
`mp_toon` is real and depended on, but only reachable via a standalone `/toon/encode` endpoint and a
status-message formatter. Tool outputs — the biggest token consumers — go in as raw JSON. This was
deliberately deferred once for correctness risk; re-evaluate with a measured before/after on a real ERP
turn rather than shipping on faith.

---

## Phase 3 — Make it actually learn from behaviour (new work)

### 3.1 Capture implicit dissatisfaction
**Defect.** Nothing detects re-asking, regenerating, editing-and-resubmitting, or "that's wrong".
`regenerateLatest` bumps a local counter and sends nothing. Zero hits repo-wide for any such signal.

**Fix.** Emit a durable negative signal for: regenerate of an answer, edit-and-resubmit of the prior
question, a near-duplicate question within a short window, and explicit correction phrases
(NO/"det er feil"/"wrong" — Norwegian first). Keep the detector pure and unit-tested; treat it as
*evidence*, not truth (a regenerate can mean "give me another style").

### 3.2 Consume the signal
A signal nothing reads is Phase 1.2's mistake repeated. Minimum viable consumer: demote/quarantine
skills that correlate with negative turns, and surface a per-org quality trend in Ops/Quality.

### 3.3 Upgrade `dreaming` from prefix-matching to real extraction
**Defect.** `dreaming.rs` docstring claims "LLM-assisted consolidation"; the file contains **no LLM
call** — it is four hardcoded prefix families (`mitt navn er`, `jeg foretrekker`, `husk at`, artifact
summaries) with 4/16/24-word caps. It therefore cannot learn corrections, which supplier/entity matters,
approval conventions, tool preferences, or anything mid-sentence.

**Fix.** Replace with an LLM extraction pass over the turn (it already runs on a 300s cadence, so this
is batchable and cheap), writing typed candidates with confidence. Keep the prefix matcher as a
zero-cost fast path.

---

## Phase 4 — CAG, user-scoped (design first, then build)

**Today.** No semantic cache of any kind. The prompt cache is in-process `DashMap` (5-min TTL, 10k
entries) — **per-container, dies on every deploy, not shared across replicas**. Redis/Dragonfly is used
only for SSE stream buffering, not caching.

**Two different things get called "CAG" — decide explicitly:**
1. **Semantic response cache** — embed the question, reuse a previous answer above a similarity
   threshold. Cheap and effective for repeated questions; **dangerous** for anything time-sensitive
   (stock levels, invoices), because a stale answer to "hva er tomt på lager?" is worse than a slow one.
2. **Cache-augmented generation proper** — preload a stable corpus into the prompt/KV cache so
   retrieval is skipped. Composes with 2.1 and is safer, since the *answer* is still generated.

**Recommendation:** do (2) via prompt caching first, and scope (1) narrowly — cache **retrieval
results** rather than final answers, and exclude any turn that used a live tool. Retrieval results are
the expensive, reusable part; answers are the risky part.

**Non-negotiable design constraints (learned the hard way today):**
- **Tenant + user in the cache key.** Today's `f1e0a9a5` fixed exactly this class of bug: a key without
  identity turns a cache into a cross-tenant **existence oracle** — probe a guessed prompt, learn from
  hit latency whether someone else asked it. Identity must come from the **verified principal**, never a
  header. Length-prefix the fields so `("ab","c")` cannot collide with `("a","bc")`.
- **ZDR bypasses read and write**, like `cache.rs` already does.
- **Move it to Dragonfly** so it survives deploys and is shared across replicas; keep an in-process L1.
- **Similarity threshold must be conservative** and configurable, with the embedding cost counted — an
  embed-per-query is not free.

---

## Phase 5 — Letta: make it real or rename it

**Defect.** `letta-bridge` is a Postgres `ILIKE` substring store (`pgstore.go:69,101`), **not a Letta
agent runtime**. It is wired live (write `grpc.rs:385→428`, read `grpc.rs:1474→1387 search_detailed(…,8)`)
but scoped **`(org_id, thread_id)` — not per-user** — and receives only the four dreaming candidate
types. Its 8 rows compete for the same memory-row budget as `memory_index` (64) and `agent_memory` (24).

**Fix, in order:** (a) **per-user scoping** — org-wide memory in a private-until-shared product is a
leak risk as much as a quality problem; (b) **embedding retrieval** instead of substring `ILIKE`
(reuse the existing embedding deployment); (c) feed it real material once 3.3 lands; (d) then decide
whether a real Letta runtime earns its keep, or whether this should simply stop being called Letta.

---

## Phase 6 — Prove it (otherwise Phases 1–5 rot)

`python/eval-lab-py` is real (12 baseline cases, LLM judge) but **not in CI** — no eval workflow among
the 12, zero callers in scripts/Makefile, output is a markdown file for humans, and no drift monitoring
feeds routing or prompts. Wire it to run on change, expand the golden set to cover the ERP/tool path
that this session exercised, and alert on regression.

---

## Execution order & rationale

1. **1.1** then **1.2** — activate built machinery before building anything new.
2. **2.2** (tokenizer) before any budget tuning; **2.1** (prompt caching) for the cost win, with the
   ZDR gate.
3. **2.3**, then **3.1/3.2** — the first genuinely new capability.
4. **3.3** → **Phase 5** — Letta is only worth fixing once there is real material to store.
5. **Phase 4** — design review before code; it is the easiest place to reintroduce a tenant leak.
6. **Phase 6** — as early as possible in practice; it is what keeps the rest honest.

## Open decisions for the owner
- **1.1** eligibility/sampling rule for LLM review (cost per chat turn).
- **1.2** what a chat rating attaches to (recommend run + injected skill ids).
- **1.2** un-quarantine `PromoteSkill`, or demotion-only for now?
- **Phase 4** semantic *answer* cache — accept staleness risk, or retrieval-only as recommended?
