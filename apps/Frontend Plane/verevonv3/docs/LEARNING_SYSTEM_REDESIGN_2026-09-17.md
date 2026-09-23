# Hermes+Letta-Inspired Learning System Redesign — 2026-09-17

Research and design pass — **parity/improvement, not a clone**. No code was changed as part of
this document. Produced by a 5-researcher + adversarial-verify + synthesis workflow. External
claims about Hermes Agent and Letta were independently fact-checked; refuted claims are excluded
from the design and listed separately (§0.2) so they aren't silently repeated later. All internal
citations are file path + line number, re-read fresh from `C:/dev/CoresSystem` on 2026-09-17.

## §0.0 — Status

Design only. Nothing below is implemented. See §8 for the priority-ordered action list.

## §0.1 — Correction to earlier reporting in this session

Two claims I made earlier this session, based on a narrower grep of model-gateway/session-core
Rust only, are **wrong**:

1. **"No code anywhere extracts a skill from a solved problem automatically."** False. A complete,
   wired, end-to-end pipeline exists in `capability-core` (Go) — see §1b.
2. Implicitly, that Verevon would need to build Letta-style semantic memory from scratch. False —
   **Verevon already has a live integration with the real Letta stack**: `letta_adapter.rs` →
   `letta-bridge` service → `agent-memory-server` → Redis, generating real embeddings. It's not a
   proposal; it's running today, just shadowed by a bug (see §2–§3).

## §0.2 — Important correction to the premise: "Letta" is two different products now

Verified directly against `letta-ai/letta`'s current README and archived-branch metadata: the
self-hosted Python server most existing documentation, blog posts, and tutorials describe (Postgres
+ pgvector, `core_memory_append`/`core_memory_replace`/`archival_memory_insert`/
`archival_memory_search` tool names) is now **explicitly labeled legacy/retired** by the project
itself. Active development has moved to a different, TypeScript-based product
(`letta-ai/letta-code`) built around a git-backed "MemFS" memory model — memory lives in a git
repository the agent owns, edited by "dreaming" and "memory doctor" subagents using git worktrees,
with **conflict resolution handled the same way git handles any merge conflict**, not by a
database query. Several tool names commonly cited for Letta (the `ChatMemory`/`memory_insert`
family) turned out to live in the **same retired file** as the "legacy" `core_memory_append` names
— there is no legacy/current split between them as commonly described.

This matters for scoping: Verevon's existing `letta_adapter.rs` integration talks to
`agent-memory-server` (a Redis-backed semantic memory server, independently maintained, **not**
the retired Letta V1 Python server or the new git-based `letta-code`). Design decisions below are
therefore based on Letta's **verified, currently-documented concepts** (memory blocks / core vs
archival vs recall memory, sleep-time compute) rather than on any specific product's current API
surface — those concepts are real and well-documented regardless of which Letta product currently
implements them, and matching concepts, not a specific SDK, is what "parity, not a clone" means
here in practice.

**Also corrected:** Hermes Agent's self-evolution repo (`NousResearch/hermes-agent-self-evolution`)
**does** document a rollback mechanism (git-revert-based, with lineage tracking and a performance
monitor that auto-triages underperforming skill variants) — an earlier draft of this research
under-read the repo and reported no rollback mechanism existed. This is directly relevant to §6's
skill-retirement design and is folded in there.

---

## 1. Diagnosis: what Verevon has today, for each of the three capabilities

### 1a. "Project learning" — **partial: write path good, read path broken**

A real write path exists and runs continuously: every dreaming cycle runs a deterministic phrase
matcher plus an LLM extractor over pending messages and upserts rows into `agent_memory`
(`session-core/src/dreaming.rs:162-234`, `persist_candidates` → `upsert_agent_memory`). Live:
`dream_runs` has executed 330 times with 95 successful saves. The read path
(`search_agent_memory`) has no relevance filter in its `WHERE` clause at all — it always returns
up to `limit` rows regardless of match quality — and ranks by a `content LIKE '%<entire user
message>%'` check that is almost never true, so ranking degrades to confidence-then-recency. See
§3.

### 1b. "Auto-generated skills" — **the mechanism is real and complete; it has simply never fired**

A genuine, fully-wired pipeline exists, **in a different service than session-core or model-
gateway**: on a `RUN_COMPLETED` event, `capability-core` (Go) fetches the session transcript,
sends it to an LLM reviewer with a prompt that lists the org's already-registered skills (so it
can propose *improvements*, not just new candidates), filters proposals through content-hash
dedup and human-authored-skill protection, and persists survivors into session-core's
`agent_skills` table via `UpsertAgentSkill` — which `model-gateway`'s `SkillStore.replace_learned`
then syncs into the live matching cache under the `session-core:` provenance prefix.

- Wired at process startup: `capability-core/cmd/main.go:206` → `startLearningConsumer` → a NATS
  subscriber on the run-completion subject.
- Gated behind an explicit, fail-closed Zero-Data-Retention attestation: *"until the RUN_COMPLETED
  producer stamps `zdr` on the envelope, every review is skipped and no skills are learned"*
  (`sessionreview/retention.go:73-75`).
- Live: `agent_skills` has **0 rows**, against 330 completed memory-dream cycles — the mechanism
  has never once produced output in this deployment, most plausibly because no `RUN_COMPLETED`
  envelope has ever carried an attested `zdr: false`. This was not independently re-traced to the
  envelope producer in this pass (would require inspecting NATS JetStream history) — flagged as
  its own follow-up, not assumed.

### 1c. "Never forgets how it solved a problem" — **nothing dedicated; conflated with 1a/1b**

There is no episodic "trace of what happened last time" store distinct from the semantic-fact
memory (`agent_memory`) and the procedural-skill store (`agent_skills`). The memory extractor's
prompt only asks for durable facts about the person (employer, locale, preferences) — never "how
a problem was solved." The `agent_skills` pipeline (1b) is the closest fit in principle, but has
never fired in practice.

---

## 2. Letta-concept mapping onto `session_core.agent_memory` — no new store

| Letta concept (verified) | Verevon equivalent today | Evidence |
|---|---|---|
| Core memory — always in context, no retrieval | `load_agent_memory_context_rows` — unconditionally injected per turn, ordered by scope, no relevance step | `dreaming.rs:499-560` |
| Archival memory — searched on demand, semantic | `search_agent_memory`, backfilled by the **real** semantic layer (letta-bridge → agent-memory-server → Redis, real embeddings via an inference provider) | `memory_grpc.rs:259-297`; `letta_adapter.rs:28-33` |
| Recall memory — full history, searchable | The `messages`/`threads` tables exist and are read by dreaming, but **no dedicated search RPC over raw transcripts** exists (only the *consolidated* `agent_memory` is searchable) | `dreaming.rs:427-497` |

**Does this need pgvector added to `agent_memory`? No — and this is the single most important
"don't clone, adapt" call in this document.** Verevon already has a working, live, embedding-
backed vector store doing exactly this job. The defect is not "no vector search exists," it's
that the vector search that exists is **only invoked as a shortfall backfill**
(`memory_grpc.rs:267-269`: query pgstore first, fire the semantic layer only if `limit` wasn't
filled) and its results are **penalized 15%** relative to pgstore hits. Since pgstore's `WHERE`
clause always returns up to `limit` rows once a user has that many memories, the shortfall is
usually zero and the real semantic layer never runs. Adding a second embedding index inside
Postgres would be a redundant, parallel system solving a problem Verevon doesn't have — the
correct fix is to stop shadowing the one that already exists.

**One concrete write-path gap, cheap to close:** `sync_persisted_candidates_to_letta`
(`dreaming.rs:295-319`) already indexes every **thread**-scoped memory into the semantic backend
immediately after a Postgres commit — but `correlated_thread_candidates` (`dreaming.rs:321-332`)
filters to `scope == "thread"` only. **User-scoped memories (14 of the 24 live rows — the richest,
most durable facts) are never mirrored into the semantic backend at all.** Widen the filter to
include `scope == "user"`.

---

## 3. Fix for the recall-ranking bug

**Root cause, exact:** `search_agent_memory`'s `ORDER BY` (`dreaming.rs:612-621`) ranks by
`lower(content) LIKE ('%' || lower($6) || '%')`, where `$6` is the entire user chat message (or a
transcript prefix) — not a keyword, on every call path (SSE chat, gRPC inference, compaction). A
3–8 word memory fact is essentially never a literal substring of a full sentence the user just
typed, so this branch is false almost every time and the `ORDER BY` collapses to its next term:
`confidence DESC, updated_at DESC` — recency, not relevance.

**Recommended fix: make the already-live semantic layer authoritative for ranking, instead of a
shortfall-only backfill — not a new keyword-extraction step.**

A keyword-extraction pre-step would mean either a second LLM call per turn (latency/cost on the
hot chat path, for a cost Verevon's own code already flags as the dominant driver of memory-search
latency) or a hand-rolled tokenizer — exactly the "reimplement what the vector store already does,
worse" the parity/improvement instruction warns against. Verevon already pays for one embedding
call when the semantic layer fires; the fix is to make that call run every non-trivial turn
(bounded by the existing timeout budget, `letta_adapter.rs:37-51`, which already exists specifically
so "a misconfiguration cannot stall a chat turn indefinitely") and merge-rank pgstore and semantic
results by score, rather than only firing semantic search for the shortfall. Keep the substring
check as a real signal (an exact restatement of a fact should still rank first) — just stop
treating its failure to match as equivalent to "no relevant memory exists."

Cost tradeoff: this means paying the existing, already-budgeted embedding-call cost more often
(every non-trivial turn instead of only on a pgstore shortfall), not adding new unbounded cost —
the SSE path already skips trivial prompts, and the call already degrades gracefully to
pgstore-only on timeout/failure.

---

## 4. Fix for duplicate memories under different keys

**This is a write-time grounding problem, not a missing consolidation pass.** The upsert already
has a correct, working conflict/supersede path (`ON CONFLICT (org_id, scope, owner, key) ... DO
UPDATE`, `dreaming.rs:769-791`) — it works perfectly *when the same key is minted twice*. The bug
is that the same fact almost never gets the same key twice, because the LLM extractor invents a
free-text `slot` name every call with **zero visibility into slots it or a prior call used before
for this user**. The extraction prompt *asks* for consistency ("the same fact learned again later
must reuse the same slot") but supplies no grounding to make that possible — nothing from
`agent_memory` is included in the extraction call's context.

This is corroborated by the live data: 24 rows, 24 distinct keys, zero collisions despite 330
dreaming cycles — the `ON CONFLICT` clause has essentially never fired.

**Recommendation: ground the extractor with the user's existing slot vocabulary before each call —
the same pattern Verevon's own skill-review loop already uses.** `capability-core`'s
`llmreviewer/reviewer.go:98-115` already lists "ALREADY-REGISTERED SKILLS" in its prompt so the
reviewer proposes improvements to existing skills rather than duplicates. Apply the identical
technique to memory extraction: before calling the extractor, fetch the user's existing slot names
(a cheap query reusing the existing `agent_memory_key_idx`) and inject them into the prompt as
slots to reuse. This fixes duplication and the contradiction problem (§7) with one change, and
mirrors a pattern already proven in this codebase rather than inventing a new one.

A background "sleep-time"-style consolidation pass (a real, verified Letta concept) is the right
*general* pattern for reconciling memories that already diverged — recommended as a secondary
safety net (periodically cluster user-scope memories by embedding similarity and flag near-
duplicates for an LLM merge decision), not the primary fix, since the divergence here is
preventable at the source for near-zero added cost.

---

## 5. Forgetting/decay design

**`expires_at` is a fully implemented, unused column — a missing invocation, not a missing
feature.** It already exists in the schema and is already load-bearing in every read path
(`search_agent_memory` and `load_agent_memory_context_rows` both filter `expires_at IS NULL OR
expires_at > now()`). Nothing ever sets it on write — confirmed by reading both INSERT statements
in full. The live finding (0 of 24 rows have it set) is the expected consequence, not a separate
bug.

**Recommendations:**

1. **Thread-scoped memories** (facts true only of one conversation, by the extractor's own scope
   definition) should get a default TTL tied to thread lifecycle — e.g. `expires_at = now() +
   90 days` at write time, since there's no principled reason a thread-scoped fact should outlive
   the thread indefinitely.
2. **User-scoped memories should NOT get an age-based default TTL.** These are meant to be
   durable, and — notably — **Letta's own verified docs describe no age/decay-based expiry
   mechanism either**; Letta relies on git-merge-style conflict resolution for concurrent edits,
   not time-based decay. Verevon should follow the same principle: age alone is not evidence of
   staleness.
3. **Set `expires_at` on explicit supersession, not primarily on age.** Today a same-key conflict
   does a hard `content = EXCLUDED.content` overwrite with no history kept. A stricter design
   (closer to "never forgets," while still surfacing only the current fact): on a same-key
   conflict where content differs meaningfully, insert a **new** row and set `expires_at = now()`
   on the **old** row — a soft-delete-by-expiry that keeps the audit trail while the existing
   `expires_at > now()` read-path filters transparently exclude it. No read-path change needed.
4. **Explicit "forget that" requests** should set `expires_at = now()` immediately via a dedicated
   tool call, symmetrical to whatever `save_memory` tool already exists. (Not independently
   verified whether a "forget" counterpart already exists — flagged as needing a quick check
   before building one.)

---

## 6. Auto-skill-generation pipeline — extend, don't replace

The existing pipeline (§1b) already does most of this correctly:

- **Trigger:** every eligible `RUN_COMPLETED` event (gated on ZDR attestation) — not a scoring
  heuristic, not an explicit user request. This is close to Hermes Agent's own verified trigger
  philosophy (judgment-based: "workflow judged worth repeating," an error-recovery path found, or
  a user correction — not a fixed numeric threshold), except Verevon currently delegates the whole
  judgment to the LLM reviewer call rather than gating the call itself on a pre-heuristic.
- **What a generated skill looks like:** `name`/`description`/`content`/`trigger_keywords`,
  persisted with a server-forced `origin = 'background_review'` — never trusted from the
  candidate.
- **Correction/retirement — already not add-only, but not automatic either:** duplicate
  suppression works (content-hash dedup); a revised proposal for the same name can overwrite the
  old content in place; human-authored skills can never be overwritten by a background review;
  and a manual `SetAgentSkillEnabled` off-switch exists. **What's missing:** nothing today
  automatically disables or downgrades a skill based on evidence it made things worse — the
  reviewer only ever proposes new/improved candidates from a successful-looking transcript, with
  no feedback signal from downstream outcomes.

**Extension proposal, built on existing primitives — and now informed by the corrected Hermes
research (§0.2):** Hermes's own self-evolution repo pairs exactly this kind of skill-quality
signal with **git-tracked lineage (trivial rollback) and a performance monitor that auto-triages
underperforming variants from real usage over time** — a stronger, more principled analog than a
single-session correction heuristic. Adapted:

1. **Trigger addition:** when a run's transcript shows the user rejecting or correcting a
   just-injected skill's guidance in the same session, tag the event with the skill id and a
   negative-outcome flag.
2. **Reviewer extension (same LLM call, extended prompt):** add an instruction so the reviewer can
   propose a `retire` or `revise` action for a specific skill id when it sees this pattern — the
   reviewer already reads existing skills and already has the transcript.
3. **Sink extension (one new branch, reusing the existing `SetAgentSkillEnabled` RPC):** when the
   reviewer proposes `retire`, call the existing enable/disable primitive rather than building a
   new one — subject to the same origin protection (never auto-disable a user-authored skill).
4. **Track lineage, per Hermes's verified pattern:** since `agent_skills` already versions by
   content-hash-gated upsert, a lightweight lineage log (which review produced which version, and
   why) gives the same "trivial rollback" property Hermes gets from git history, without adopting
   git as a storage layer.

This plugs into the existing `LEARNED_SOURCE_PREFIX` sync mechanism unchanged — a disabled skill
simply stops being returned by the org's skill list on the next sync.

---

## 7. Contradiction handling

**Supersession-by-key-match already exists and is mechanically correct — it isn't firing because
of the exact key-drift diagnosed in §4.** If "works in Norwegian" and "switched to English" had
been extracted under the identical slot, the existing `ON CONFLICT ... DO UPDATE` would already
make the newer statement overwrite the older one. Both being live simultaneously is direct
evidence they were minted under two different slot spellings. **The §4 fix (ground the extractor
against existing slots) resolves this specific contradiction class with no new logic** — same-slot
overwrite already handles it once naming is consistent.

A more general *semantic* contradiction detector (catching two genuinely different slots that
conflict, e.g. differently-named location fields) is a harder problem that **Letta itself does not
solve either** (verified: no semantic contradiction-detection mechanism beyond git-merge-style
conflict resolution for concurrent edits) — building one now would exceed what either reference
system has actually verified working, and shouldn't be attempted until §4 ships and its effect on
collision rate is measured.

---

## 8. Priority-ordered action list

**Honest framing, not maximal scope by default:** at 24 `agent_memory` rows and 0 `agent_skills`
rows after ~10 days of live use against 330 completed dreaming cycles, a full Letta-tier rebuild
(a second vector store, a git-backed memory filesystem, a new background-agent framework) is
disproportionate. Every gap found in this pass is a **wiring/invocation defect in code that
already exists** — a bad `ORDER BY`, an ungrounded prompt, an unset column, an unattested envelope
field — not a missing architectural capability. Nothing in this research supports rewriting the
memory system into a new store.

1. **Fix the recall-ranking bug (§3).** Smallest, highest-leverage change: stop letting a `LIKE`
   clause that almost never matches silently decide ranking; make the already-paid-for semantic
   backend authoritative instead of a shortfall-only fallback.
2. **Ground the memory-slot extractor against existing keys (§4/§7).** Fixes duplication and
   contradiction with one change, directly mirroring a pattern already proven in this codebase
   (the skill reviewer's "already-registered" hint).
3. **Widen the semantic-mirror filter to include user-scope memories; set `expires_at` on thread-
   scope writes and on supersession (§2/§5).** Meaningful correctness improvements; lower urgency
   than 1–2 because these are silent degradations, not active wrong-ranking.
4. **Investigate and fix `RUN_COMPLETED` ZDR attestation so the skill-learning loop can fire at
   all (§1b).** Prerequisite for #5 to have anything to build on — scope as its own investigation
   before estimating effort (the envelope producer side wasn't traced in this pass).
5. **Add the skill-retirement feedback loop, informed by Hermes's lineage/rollback pattern (§6).**
   Lowest priority — extends a pipeline that has never yet produced a single skill in this
   environment (item 4 must land first). Ship behind a flag/log-only mode first; a wrong
   auto-retirement heuristic could disable good skills.

**Not recommended at all:** a pgvector column on `agent_memory`, a git-backed MemFS-style rewrite,
or standing up a second embedding pipeline. Verevon already has a working, live, embedding-backed
semantic memory store that is simply being under-used by the read path. Building a parallel one
would be exactly the "install something that doesn't fit our architecture" outcome ruled out at
the start of this task.
