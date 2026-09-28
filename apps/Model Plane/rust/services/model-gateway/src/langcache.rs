//! Record of the model-gateway's removed answer cache. **No cache lives here
//! any more, and the chat path consults none.**
//!
//! This module is kept as documentation rather than deleted because the same
//! idea — "cache the answer, key it on the prompt" — is an obvious one to
//! propose again, and both attempts at it failed for reasons that are not
//! obvious from the outside. `SEMANTIC_CACHE_URL`, `SEMANTIC_CACHE_TTL_SECS`,
//! `SEMANTIC_CACHE_DATAPLANE_ENABLED` and `LANGCACHE_*` are all inert: setting
//! any of them now does nothing at all.
//!
//! # 1. The hosted third-party backend (removed first)
//!
//! A managed Redis `LangCache` backend (`LANGCACHE_URL` / `LANGCACHE_CACHE_ID` /
//! `LANGCACHE_API_KEY`) used to take selection precedence. What it shipped was
//! the whole problem: the key was the FULLY ASSEMBLED prompt — conversation
//! history, injected memory, retrieved Data Plane context — and the value was
//! the model's full answer, both POSTed to a third-party host that embedded them
//! server-side. That hop had none of the residency machinery inference-core
//! applies per provider (`provider/mod.rs` EU deny-by-default), carried no
//! purpose/lawful basis/retention metadata, set no TTL, and exposed no delete
//! call — so an org-erasure or DSAR fan-out could not reach the copy at all.
//! Leaving the client in the tree behind an env var would mean one `LANGCACHE_URL`
//! in one env file silently reopening it, which is why the code went rather than
//! being disabled.
//!
//! # 2. The local Dragonfly exact-match tier (removed 2026-09-16)
//!
//! What replaced it was boundary-safe — our own Dragonfly, keyed on the exact
//! `(org_id, user_id, model, prompt)`, no embeddings, no cross-plane call — and
//! it still had to go, for a different reason: **it could not both hit and be
//! correct.**
//!
//! Measured on the live stack: the same question ("Hva er hovedstaden i
//! Frankrike?") asked twice, each as the first message of a fresh thread,
//! produced two cache entries and zero hits. The assembled prompt had grown from
//! 17042 to 17229 characters between the two turns. It drifts because it carries
//! two independent recency-ordered top-N windows over a store the system writes
//! to after most turns:
//!
//!   * the gateway's own recalled-memory block (`sse.rs`, via
//!     `memory_provenance::memory_context_block`) — a cross-thread similarity
//!     search that returns the top few entries about the person *whether or not
//!     anything matches*; and
//!   * session-core's context-assembly block, whose `episodic`/`user` segments
//!     union thread-scoped rows with cross-thread, user-scoped agent-memory rows
//!     and a Letta semantic search (`session-core/src/grpc.rs`
//!     `load_context_memory_rows`, `bucket_memory_segments`).
//!
//! Every remaining component of the prompt is stable, stable within a UTC day
//! (the temporal message is date-only), or deterministic. So the choice was:
//!
//!   * **keep the volatile blocks in the key** — correct, and it cannot hit; or
//!   * **leave them out of the key** while still sending them to the model —
//!     hits become possible, and an answer computed under one memory set can be
//!     served to a request under another with nothing to detect it. Those blocks
//!     are in the prompt *in order to change the answer*; a key blind to them is
//!     a correctness bug, not an optimisation; or
//!   * **exclude memory-bearing turns from caching entirely** — correct, and
//!     equivalent to having no cache, since recall is unconditional top-N and
//!     assembly runs on essentially every turn. This is the same shape as the
//!     `assembly_supplied_grounding` gate that already made the cache store
//!     nothing at all once, and it would leave a module logging "cache enabled"
//!     while doing nothing.
//!
//! Per-user scoping was not the thing to relax to escape that. It was load-
//! bearing: without `user_id` in the key, two colleagues in one org asking
//! near-identical questions match each other and one is served an answer built
//! from the other's memories — a per-user assistant cannot trade that for a hit
//! rate.
//!
//! # 3. What the cache was actually costing
//!
//! It rendered a ~17 KB prompt, hashed it, and awaited a Dragonfly GET on the
//! critical path *before* inference on every eligible turn, then wrote the
//! answer back. The read never hit. The write left a copy of answer text under a
//! key with no index and no delete path, so an org-erasure or DSAR fan-out could
//! not reach it either — the same fault that removed the hosted backend, in the
//! tier that survived it.
//!
//! # 4. What covers the cases a hit would have covered
//!
//! With any safe key, a hit needed the same user, model, question, history and
//! UTC day inside the TTL — in practice double-submit, regenerate, and
//! retry-after-error. All three are already handled upstream of inference, and
//! better:
//!
//!   * [`crate::idempotency_registry`] — claimed at the top of `invoke_stream`,
//!     keyed on the client's `idempotency_key`. A concurrent duplicate is
//!     rejected as in-flight (which a prompt cache cannot do: both requests race
//!     past the lookup before either stores) and a completed result replays
//!     verbatim for 10 minutes, with its real usage rather than a re-derived
//!     score.
//!   * [`crate::stream_buffer`] — resume of an interrupted stream, addressed by
//!     verified identity plus request id.
//!
//! One hop down, inference-core's `PromptCache` (`inference-core/src/cache.rs`)
//! keys a blake3 digest of org, user, subscription, provider hint, model, EVERY
//! message role+content, temperature, max_tokens, tools and schema, refuses ZDR
//! requests outright, and expires in 5 minutes. It is live on the UNARY path
//! (`provider/fallback.rs` `infer`) and deliberately not on `infer_stream`,
//! which is what plain chat uses. Its own comment reaches the same conclusion
//! reached here: with the whole message history pinned, a realistic hit is the
//! same user regenerating or double-submitting. So the gateway tier was a
//! second, weaker copy (`DefaultHasher`, whose output Rust does not guarantee
//! stable across releases, persisted to Dragonfly with no key index and no
//! delete path) of an idea that is already implemented correctly where it can
//! see the provider request.
//!
//! # 5. If a response cache is wanted again
//!
//! Re-deriving the argument above is the prerequisite, not re-adding the code.
//! Two things would genuinely pay and neither is this module:
//!
//!   * ordering the system messages so the volatile blocks come LAST, directly
//!     before history. They currently sit ahead of five stable system messages,
//!     which destroys provider-side prompt-prefix caching from that point on;
//!     moving them restores a long byte-stable prefix that hits on nearly every
//!     turn at zero correctness cost.
//!   * a cache keyed on something that identifies the REQUEST (an idempotency
//!     key, a client submit token) rather than a hash of a 17 KB prompt — which
//!     is what `idempotency_registry` already is.
//!
//! The property the removal rests on is pinned by
//! `sse::tests::the_assembled_prompt_is_not_a_function_of_the_question`: if
//! assembly ever becomes a pure function of the question, that test fails and
//! this decision is worth revisiting.
