# embedding-engine-rs Research Dive

Generated: 2026-06-07
Re-verified: 2026-07-10 (live Docker + source re-read; supersedes the 2026-06-07 snapshot below where noted)

Scope: `apps/Data Plane v2/services/embedding-engine-rs`

## 2026-07-15 final isolated acceptance delta

The final rebuilt service reached healthy state, participated in the supported
signed broker delivery/redelivery matrix, and was included in the six-store ZDR
final-state comparison. No restrictive request produced a persisted embedding in
that comparison. Production broker ACLs, strict per-store mutation telemetry,
and shared deployment remain pending.

## 2026-07-15 isolated runtime delta

The current-source image `401d28370432` carries revision
`eeebd0bc98c66434936460020958891066eb05fd` and reached healthy state in the
disposable stack. No signed broker event, embedding egress, Qdrant write, or
cross-plane inference flow was invoked, so this is startup evidence only.

## Secure-MVP current state — 2026-07-10

- **Implemented/contained:** unsigned knowledge-unit, wiki, and page-image event
  consumption is disabled by default behind two explicit insecure-development
  gates. This fails closed against forged asynchronous tenant/content identity.
- **Tested:** 19/19 tests pass after consumer containment, including the two-gate
  regression; strict combined embedding/index all-target clippy passes.
- **Built/reachable in isolation:** the revised image built locally with
  verified revision/build labels and reached healthy state in the disposable
  stack, but its signed event/data flow was not exercised or shared-deployed.
  With consumers disabled the embedding pipeline is intentionally ineffective until
  signed, producer-scoped envelopes and NATS permissions are implemented.
- **Cross-plane blocker:** Inference Core gRPC is securely disabled by default
  until a verified scoped service principal replaces shared-key/body identity;
  signed ZDR propagation is not proven end to end.
- **Coverage/audit:** Rust coverage and Rust dependency-audit tools were unavailable;
  no coverage percentage or audit result exists for this pass.

The remainder is a superseded, sanitized pre-containment audit. Its 18-test result
does not prove the current event-disabled runtime or a rebuilt image.

## Historical pre-containment audit (superseded)

This pass re-read every source file (the tree has grown from ~12 files/single-file modules to 10 files across `api/`, `batch/`, `provider/` (+ `visual.rs`), `qdrant_writer/`, `stream/`, plus standalone `image_consumer.rs`, `wiki_consumer.rs`, `config.rs`, `main.rs` — 2,314 non-test lines), ran a live curl-equivalent health check against the running container, ran `cargo fmt`/`cargo clippy`/`cargo test` scoped to this crate only, and checked the live Postgres schema the crate queries against.

Headline: **the 2026-06-07 doc is stale and undersells the service.** The single biggest change since then is a real, tested, committed ZDR (Zero Data Retention) egress guard (commit `31176334` — "feat(dpv2,model-plane): ZDR embedding-egress guard + zdr hop field (PR-3 pt2)"), plus a full multimodal visual-RAG arm (Cohere Embed v4 page-image embeddings) that didn't exist in June. The "provider path is still transitional" framing from the old doc is largely resolved: compose now defaults `EMBEDDING_PROVIDER=model_plane` and the live container confirms it is actually running that way.

Verdict on the specific baseline claim under test — **"does embedding-engine-rs actually skip persisting embeddings when zdr=true?"** — no. It does not have an "ephemeral write" mode. What it actually has is stronger for two of its three egress paths and weaker for the third:

- **Direct-Azure text path** (`AzureOpenAiEmbeddingClient::embed_batch`, `provider/mod.rs:257-265`) and **Cohere Embed v4 visual path** (`VisualEmbeddingProvider::embed_images`, `provider/visual.rs:137-144`): both **fail closed before any network call** when `zdr=true` — `anyhow::bail!` with an explicit "must not egress" error, verified before the HTTP request is built. This is not "ephemeral," it's "refuse to embed at all." Four unit tests (`azure_openai_egress_guard_rejects_zdr`, `azure_openai_allows_non_zdr`, `embed_images_egress_guard_rejects_zdr`, `embed_images_allows_non_zdr`) pin both the reject-when-true and allow-when-false behavior, and all pass live (`cargo test -p embedding-engine-rs`, 18/18 green, 2026-07-10).
- **Page-image consumer** (`image_consumer.rs:267-276`): a `zdr=true` page image is logged and **ack'd without embedding** (dropped, not persisted, no retry) — this one genuinely is a skip-persist path, and it is deliberate and documented in the module doc comment.
- **Model Plane (gRPC) path** — the live default (`EMBEDDING_PROVIDER=model_plane`) — does **not** enforce ZDR itself. `ModelPlaneEmbeddingClient::build_request` (`provider/mod.rs:189-207`) faithfully forwards the `zdr` bool and the residency `region` onto the wire `CreateEmbeddingRequest` to inference-core, and a unit test (`model_plane_request_carries_zdr_and_region`) pins that both `true` and `false` round-trip correctly on the request — but embedding-engine-rs itself trusts inference-core to act on that flag. **This is a real trust-boundary handoff, not a bug in this service**, but it means embedding-engine-rs's ZDR compliance for its default/live provider is contingent on Model Plane behavior this audit did not independently verify from this side. Flag for cross-plane confirmation.
- The historical source read confirmed `documents.zdr_classification` was queried
  rather than trusted from an event. Production row counts and classification
  distribution are redacted. This was unit/code evidence, not an end-to-end
  restricted-document run.
- `qdrant_writer/mod.rs` (`upsert_vectors`) has **no ZDR awareness at all** — it is a dumb sink that persists whatever points it is handed. All ZDR enforcement lives upstream in the provider/consumer layer, before a vector ever reaches this module. That's a reasonable single-responsibility split, but it also means a future caller that skips the egress guard (e.g., a new consumer added without routing through `EmbeddingProvider::embed_batch`) would have nothing at the Qdrant-write layer to catch it.

Net: the baseline's plane-wide ZDR finding ("retrieval writes query embeddings in ephemeral mode") is about `retrieval-engine-rs`, not this service — it does not apply here. embedding-engine-rs's own ZDR story is the healthiest-looking piece of the plane's ZDR picture, with the caveat that its default live path (Model Plane) delegates enforcement downstream rather than proving it locally.

### Fmt/clippy drift — re-verified for this crate specifically

- `cargo fmt -p embedding-engine-rs -- --check`: **fails**, 5 real diff hunks — `image_consumer.rs:318`, `image_consumer.rs:327`, `main.rs:76`, `provider/visual.rs:178`, `provider/visual.rs:193`, plus one in the `visual.rs` test module (`from_config_requires_key_when_endpoint_set`). All are long-line wraps rustfmt wants reflowed; no logic differences. Confirms the plane baseline's "Rust formatting drift spans embedding-engine-rs" P2 finding is still current.
- `cargo clippy -p embedding-engine-rs --all-targets -- -D warnings`: **clean, zero warnings.** The plane baseline's clippy finding (`retrieval-engine-rs/src/pipeline/orchestrator.rs:58`, doc-comment lint) is confirmed to belong to a different crate — it does not reproduce here. This crate's clippy is not part of that P2 item.
- `cargo test -p embedding-engine-rs`: **18/18 pass** (16 unit + 2 integration in `tests/wiki_event_schema.rs`), ~1m33s cold build. No flakiness observed.

### Live health check (2026-07-10)

```
GET /health  → 200 (body omitted)
GET /readyz  → 200 (body omitted)
docker ps: dpv2-embedding-engine  Up 14 hours (healthy)  0.0.0.0:9202->9202/tcp
```

Container env confirms the live provider selection: `EMBEDDING_PROVIDER=model_plane`, `MODEL_PLANE_AI_CORE_GRPC_URL=http://inference-core:9092`, `MODEL_PLANE_EMBEDDING_PROVIDER=azure_openai` (inference-core's own upstream, one hop further), `COHERE_EMBED_V4_ENDPOINT` set (visual arm active). Startup log line confirms: `"embedding backend selected" provider=model_plane model=text-embedding-3-large` and `"visual embedding (Embed v4) enabled" model=embed-v-4-0`.

Two things worth flagging from the last-24h log scan, both minor/non-blocking:

1. A single `Error: pool timed out while waiting for an open connection` at 2026-07-09T21:02:43Z, immediately followed by a clean restart 12 seconds later (`embedding-engine-rs starting` → `embedding backend selected`) that has run healthy since (14h uptime, no recurrence). Reads as a one-time Postgres-not-yet-ready race at stack bring-up, not an ongoing issue — `docker-compose.yml` already gates this service on `postgres: condition: service_healthy`, so this was likely a startup-ordering blip during a full-stack restart rather than a steady-state fault.
2. 3× `"wiki message recv error" error="missed idle heartbeat"` and 3× `"page-image message recv error" error="missed idle heartbeat"`, all clustered in the first ~70 minutes after startup (21:26–22:14 that same day), none since. JetStream's pull-consumer `.messages()` stream re-establishes on the next loop iteration (see `wiki_consumer.rs:84-125`, `image_consumer.rs:196-256`), so this is self-healing by design, not a stuck consumer — but it is worth someone confirming NATS/JetStream wasn't under memory/CPU pressure during that window if it recurs at higher frequency.

No `batch embedded` (i.e., no document knowledge-unit batches processed) and no `batch processing failed` lines in the last 24h — consistent with an idle ingest pipeline in this environment, not evidence of a stuck consumer (the wiki and page-image consumers to at least attempt connections in that window).

### Known, still-present, non-blocking bug: log target name mismatch

`main.rs:18` still defaults the `tracing_subscriber::EnvFilter` to `"embedding_engine_rs=info"`, but the actual crate/binary target name tracing uses is `embedding_engine` (from `[[bin]] name = "embedding-engine"` in `Cargo.toml`, confirmed live: log lines carry `"target":"embedding_engine"`). The default therefore matches nothing and would silently produce empty logs if `RUST_LOG` were ever unset. `docker-compose.yml` already carries an explicit override (`RUST_LOG: ${RUST_LOG:-warn,embedding_engine=info}`) with a comment flagging this exact mismatch and noting the code default needs fixing on next rebuild — so it is a known, already-mitigated issue in the current deployment, not a fresh finding. Low priority; worth folding into the next unrelated touch of `main.rs`.

### grep for TODO/FIXME/mock/stub/fake/placeholder

No `TODO`, `FIXME`, `stub`, `placeholder`, `unimplemented`, or "not implemented" hits anywhere in `src/`. The only `mock`/`fake` hits are three `"fake-key"` string literals, all inside `#[cfg(test)]` modules (`provider/mod.rs:390,412`, `provider/visual.rs:237`) used to construct a provider against an intentionally-unreachable test endpoint — legitimate test fixtures, not production placeholders.

### Cross-plane contract note (out of scope for this doc, flagged for visibility only)

The plane baseline's Quarry-v2 contract-drift finding (`DataPlaneIngestRequest` constructors missing `initiator_user_id`/`visibility`) is on the ingest-write side (index-engine's inbound contract), several hops upstream of embedding-engine-rs's JetStream consumption of already-created `knowledge_units` rows. It does not touch any file in this crate and is not re-verified here — see the Ingestion Plane audit pass for that.

---

## 2026-06-07 Snapshot (superseded where the section above overrides it)

### Snapshot

`embedding-engine-rs` turns indexed knowledge units and wiki events into dense vectors and writes them into Qdrant. It is the vector-write authority inside Data Plane v2. As of 2026-07-10 it also owns a third arm: multimodal page-image embeddings via Cohere Embed v4 for the visual-RAG retrieval path (not present in the original June snapshot).

Current evidence highlights:

- Rust JetStream consumer with admin HTTP (`/health`, `/readyz`)
- provisions three Qdrant collections on boot: primary (`dataplane_knowledge`), wiki (`wiki_block_embeddings`), entity-summary (`entity_summary_embeddings`), plus a fourth visual collection (`dataplane_page_images`) when the visual arm is enabled
- handles document knowledge-unit embeddings, wiki/entity-summary embeddings, and (new) page-image embeddings
- compose now defaults `EMBEDDING_PROVIDER=model_plane` (confirmed live 2026-07-10) — the "still routes through direct provider HTTP" framing from June is outdated; direct Azure remains available as an explicit override/fallback, and is now also the provider that fails closed on ZDR content

Non-generated file count from the current tree: 10 files, 2,314 lines (`main.rs`, `config.rs`, `image_consumer.rs`, `wiki_consumer.rs`, `api/mod.rs`, `batch/mod.rs`, `provider/mod.rs`, `provider/visual.rs`, `qdrant_writer/mod.rs`, `stream/mod.rs`), up from the ~12-file estimate in June — the growth is the visual-RAG arm and the module split (single files → `provider/`, `qdrant_writer/`, `batch/`, `api/`, `stream/` submodules).

### Runtime Shape

Key runtime entrypoints (current module layout, corrects the June doc's flat single-file listing):

- `src/main.rs` — Postgres pool, Qdrant client, provider selection, JetStream knowledge-unit consumer, wiki subscriber, visual/page-image subscriber (conditional on `COHERE_EMBED_V4_ENDPOINT`), admin HTTP; races the admin server against the consumer loop via `tokio::select!`
- `src/provider/mod.rs` — `EmbeddingProvider` (`ModelPlane` gRPC backend vs. `AzureOpenAi` direct-HTTP backend), including the ZDR egress guard on the direct path
- `src/provider/visual.rs` — `VisualEmbeddingProvider` (Cohere Embed v4 / Azure AI Foundry), including its own ZDR egress guard
- `src/qdrant_writer/mod.rs` — collection provisioning (`ensure_collection`), point upsert (`upsert_vectors`), and delete-by-document/delete-by-ids (no ZDR logic; pure sink)
- `src/stream/mod.rs` — knowledge-unit JetStream consumer (`DATAPLANE_KNOWLEDGE` stream): batches, resolves ZDR from the live `documents` row, handles `documents.deleted`/`knowledge.units.deleted` for cleanup, DLQs after `max_delivery_attempts`
- `src/batch/mod.rs` — `process_batch`: groups by `(org_id, zdr)`, calls the provider, upserts to Qdrant, marks Postgres status, publishes `dataplane.documents.indexed` (freshness signal) and `dataplane.cost.ledger` events
- `src/wiki_consumer.rs` — durable JetStream subscriber for `dataplane.wiki.version.published` (upgraded from June's "wiki publish events" description of a best-effort core-NATS subscription to a durable stream with retry/DLQ semantics)
- `src/image_consumer.rs` — durable JetStream subscriber for `dataplane.page_images.created/deleted` (new since June): fetches rendered page bytes, downscales/recompresses to fit Embed v4's ~8000-token budget, embeds, upserts into the visual collection
- `src/api/mod.rs` — `/health`, `/readyz` only

Surface: admin/health HTTP only; three durable JetStream consumers (knowledge units, wiki, page images) plus core-NATS-derived document-deletion handling folded into the knowledge-unit stream.

### API And Relationship Map

Current relationships:

- `index-engine-rs` → `embedding-engine-rs`: knowledge-unit creation triggers dense embedding via `dataplane.knowledge.units.created`
- `wiki-store-go` → `embedding-engine-rs`: wiki version publish triggers wiki block embeddings via `dataplane.wiki.version.published` (now durable, was best-effort)
- Ingestion Plane (page renderer) → `embedding-engine-rs`: `dataplane.page_images.created/deleted` triggers/purges visual embeddings (new arm, not in June doc)
- `embedding-engine-rs` → Qdrant: provisions and writes primary, wiki-block, entity-summary, and page-image collections
- `embedding-engine-rs` → Model Plane (`inference-core`, gRPC `:9092`): live default embedding route; carries `zdr` and `region` on the wire
- `embedding-engine-rs` → Azure OpenAI (direct HTTP) / Cohere Embed v4 (direct HTTP, Azure AI Foundry): fallback/explicit-override providers; both fail closed on ZDR
- `retrieval-engine-rs` → `embedding-engine-rs`: depends on resulting Qdrant collections (including the visual one, fused via `w_visual`) for dense retrieval

### Duplicates, Redundancies, And Inactive Surfaces

The June doc's framing ("provider selection duplicated conceptually between direct provider access and the intended Model Plane route... a workaround for the deeper gRPC embedding-path issue") is now largely resolved: `EMBEDDING_PROVIDER=model_plane` is the compose default and is confirmed live. The direct-Azure and Cohere-direct backends remain as configured alternatives/fallbacks, and are now purpose-repurposed as the "fails closed on ZDR" backstop rather than a workaround — not dead code, but not the primary path either.

### Stubs, Placeholders, And Missing Connections

Re-confirmed 2026-07-10: no explicit source stubs, TODOs, or backup residue in this service tree (see grep results above). The one genuinely-unresolved partial connection is the trust handoff on the Model Plane path noted above: embedding-engine-rs forwards `zdr`/`region` faithfully but does not itself verify inference-core enforces them — that enforcement lives entirely on the Model Plane side of the boundary.

### API Design And Performance Notes

API design:

- keeping this service internal and admin-only is correct; unchanged from June
- collection provisioning at boot (now four collections, not one) is still the right place for Qdrant readiness
- grouping embed batches by `(org_id, zdr)` (`batch/mod.rs:125-134`) is a clean way to let the egress guard fire only for the restricted subset of a mixed batch, without embedding requests leaking across the ZDR boundary

Performance and operational notes:

- provider routing is no longer the top architectural concern it was in June — it is resolved to `model_plane` by default and live-verified as such
- the page-image budget-fitting logic (`image_consumer.rs::prepare_image_data_url`) is a reasonable defensive measure (progressive downscale + JPEG re-encode) against Embed v4's ~8000-token image budget, with unit-test coverage for the pass-through (small image) case
- `qdrant_writer::upsert_vectors` calls `.wait(true)` on every upsert (synchronous acknowledgement from Qdrant) — correct for correctness/ordering guarantees, at some latency cost per batch; not flagged as a problem, just worth knowing for capacity planning if batch sizes grow
- `ModelPlaneEmbeddingClient::embed_one` embeds texts one-at-a-time in a loop (`provider/mod.rs:179-183`) rather than a true batched gRPC call — fine at current volumes, a candidate for a batched RPC if per-document chunk counts grow large

### Current Doc Cleanup Read

Keep (unchanged from June):

- `DATA_PLANE_DEEP_DIVE.md`
- `docs/quickwit-read-model.md`

No delete-ready service-local docs found in either pass.

### Bottom Line

`embedding-engine-rs` is real, production-shaped, and has grown a full visual-RAG arm plus a genuine, tested ZDR egress guard since the June snapshot — the previous "transitional provider path" framing is stale and should not be repeated. The two live-verified gaps worth tracking are (1) `cargo fmt` drift specific to this crate (mechanical, 5 hunks, no logic risk) and (2) that this service's ZDR compliance on its default live path is a faithful-forward, not a locally-enforced guarantee — Model Plane/inference-core needs to be independently confirmed to honor the `zdr`/`region` fields it receives. Everything else checked (health, schema alignment, clippy, tests, TODO/stub scan) came back clean.
