# inference-core Research Dive

Generated: 2026-07-11 (supersedes the 2026-06-09 pass)
Scope: `apps/Model Plane/rust/services/inference-core`

## 2026-07-16 delta

At 2026-07-16 20:05 CEST, the local integration container is healthy with
restart count zero, HTTP `/readyz` is 200, and additive gRPC `:9092` is
loopback-reachable. Unauthenticated calls reject. A valid Data retrieval-engine
service token reached `Infer`; its signed ZDR claim overrode request
`zdr=false`, both configured providers were skipped, zero provider attempts
occurred, and the RPC failed closed with `FailedPrecondition`. No paid provider
call was made. Interactive callers remain all-ZDR and no provider deployment is
independently verified as ZDR-capable, so provider work correctly remains
unavailable. Signed ZDR posture is preserved from authenticated identity, and
each unattested provider modality—unary/streaming inference, embeddings,
speech, translation, vision, document, language, realtime, and video—fails
before provider I/O. The live proof establishes safe denial, not provider,
fallback, cost-attribution, or usable-chat readiness; the dirty-tree image is
not an immutable release candidate.

## 2026-07-13 compatibility/security correction

The running image now differs from both the historical deployment described below and the current source: live HTTP health is green but `:9092` refuses connections. Chat, multimodal inference, and Data Plane query embedding are therefore unavailable through this required contract.

Current source restores the complete additive 20-RPC gRPC service without removing HTTP. It eagerly loads bounded Auth Core JWKS, accepts RS256 only, validates issuer/audience/lifetime and canonical user/service identity, pins all tenant-bearing requests to the signed organization, requires exact `inference:invoke` for provider work, permits `inference:read` or invoke for catalogs, applies issuer-monotonic ZDR to infer/embed, and exposes unauthenticated standard gRPC health under `model_plane.v1.InferenceCore` plus the overall server name. The HTTP routing-policy surface now requires the exact `inference:policy:admin` scope. Missing/bad auth configuration prevents both listeners from starting.

Verification: 124 library tests plus 7 cache and 8 fallback integration tests passed; format and strict clippy passed. Authentication code measured 86.77% line coverage. The inherited gRPC implementation measured only 11.50% because most RPCs lack a provider-chain service harness; that gap is explicit and blocks a coverage-complete readiness claim.

**Deployment state:** SOURCE RESTORED, LIVE ABSENT, CUTOVER BLOCKED. Every
caller needs a valid `aud=inference-core` bearer. The inference budget client
separately needs `aud=cost-core`; the inference bearer must not be reused.
Provider fallback skips any route whose exact deployment has not been explicitly
confirmed for ZDR and returns failed-precondition before network I/O when no
compliant route exists. `AZURE_OPENAI_ZDR_CONFIRMED` defaults false; region
alone never promotes a deployment. The modality handlers now enforce the signed
posture even where legacy request protos do not carry a ZDR field. Do not
rebuild or deploy until caller, policy-store, contract, live-negative, and
rollback gates pass.

Auditor note: every finding is graded `[live-curl]` (host curl to a published port), `[source-only]` (read from disk), or `[inspect]` (`docker ps`/`docker inspect` — config/state, not exec). Docker's containerd content store is corrupted this pass: `docker exec`/`build`/`logs` fail fleet-wide, so no in-container verification was possible.

## Snapshot

`inference-core` is the real provider-routing engine of Model Plane. It is NOT a stub. It owns multi-provider LLM chat (unary + streaming), embeddings, and a broad multimodal surface (speech, translation, vision, document intelligence, language analytics, realtime sessions, video generation). Inference runs against live upstreams — Anthropic (direct + Azure AI Foundry Claude), OpenAI, and Azure OpenAI — with no mock or canned-response path in any production code path. `[source-only]`

Non-generated source: 22 files, ~11.6k lines under `src/`. `[source-only]`

Headline for this pass: **there is a large uncommitted WIP in the working tree that removes inference-core's entire gRPC inference server.** The *running* container still serves it (built before the WIP), so live chat is unaffected today — but building/deploying the working tree as-is would break the whole Model Plane inference loop because model-gateway hard-dials inference-core on `:9092`. See "Uncommitted WIP" below. This is the single most important finding.

## Runtime shape

Entrypoints and surfaces:

- `src/main.rs` — process bootstrap: telemetry, `InferenceConfig::from_env`, builds the `FallbackChain`, starts the HTTP health/policy server, (committed) starts the gRPC server. `[source-only]`
- `src/grpc.rs` (1074 lines) — the full `InferenceCore` gRPC service on `:9092`. `[source-only]`
- `src/provider/*` — provider clients and modality chains. `[source-only]`
- `src/http_health.rs` — HTTP on `:8082` (`/healthz`, `/readyz`, `/metrics`) + the internal routing-policy admin (`GET|PUT /internal/v1/router-policy`). `[source-only]`
- `src/cache.rs`, `src/streaming.rs` — in-memory prompt cache + gRPC stream bridge. `[source-only]`

Ports:

- gRPC `:9092` (container-internal `9092`; the deployed container also happens to host-publish `9092`, a legacy mapping — the base compose only publishes `18082:8082`). `[inspect]`
- HTTP health `:8082`, published to host `18082`. `[inspect]`

Live health: `curl http://localhost:18082/healthz` → **200 `ok`**. Note the path is `/healthz`, not `/health` (that 404s). `[live-curl]`
Live gRPC port: `nc -z localhost 9092` → **open/accepting**; an h2c curl connects (000, no HTTP body — expected for gRPC). `[live-curl]`
Container: `model-plane-inference-core-1`, `Up ... (unhealthy)`, started 2026-07-09. The `(unhealthy)` is the compose `CMD ["curl","-f",".../healthz"]` healthcheck failing under the corrupted containerd exec path — NOT the service being down (host curl to `/healthz` returns 200). `[inspect]`+`[live-curl]`

## Are the providers real? (Phase-4 question 1 + service focus)

Yes. All provider clients are real HTTP integrations; no mock/canned path exists outside `tests/`. `[source-only]`

- **Anthropic** (`provider/anthropic.rs`, 583 lines) — POSTs the native Anthropic Messages API. Two flavors sharing one codec: **Direct** (`https://api.anthropic.com/v1/messages`) and **Azure AI Foundry** (`https://<res>.services.ai.azure.com/anthropic/v1/messages`, same `x-api-key` auth). Real unary + SSE streaming, real tool-use (`tool_use` block parsing), real token usage. Documents the field-verified Azure-Foundry quirk (native body + `x-api-key`, not `api-key`/`api-version`). `[source-only]`
- **OpenAI / Azure OpenAI** (`provider/openai.rs`, 818 lines) — real chat-completions (unary + SSE with `stream_options.include_usage` for real streamed token counts), real embeddings, real tools, real `response_format: json_schema` structured output, correct `max_completion_tokens` handling for gpt-5/o-series. Azure flavor uses deployment-in-URL + `api-key` header. `[source-only]`
- The multimodal chains (`speech`, `translation`, `vision`, `doc_intel`, `language`, `realtime`, `video`) each carry real Azure/OpenAI operation clients plus config-gated activation. The only "placeholder" provider is `speech.rs::NoopSpeechProvider`, an honest fail-closed fallback that returns `Unavailable("speech provider not connected to upstream")` when no upstream is configured — a guard, not a fake response. `[source-only]`

Deployed provider posture (from `deploy/.env`, values redacted): `INFERENCE_PROVIDER_ORDER=azure,anthropic,openai`; `AZURE_OPENAI_ENDPOINT=https://core-ai-rg.cognitiveservices.azure.com` + key set (region `swedencentral`); `AZURE_ANTHROPIC_ENDPOINT=https://cloude-ai-resource.services.ai.azure.com` + key + deployments `claude-haiku-4-5,-sonnet-4-5,-sonnet-4-6,-opus-4-8`; `ANTHROPIC_API_KEY` set but **not used** (Foundry takes precedence in `FallbackChain::from_config`); `OPENAI_API_KEY` empty. Effective registered providers: **azure-openai + azure-anthropic** (both real, both EU/Foundry). `[source-only]`

## Smart model selection (intent layer) — real

`provider/intent.rs` (721 lines) implements the "complexity × budget → model" router the memory notes describe, and it is genuine, pure, and well unit-tested. `[source-only]`

- Three Verevon auto modes parsed from the model id: `verevon-budget|-balance|-genius` (plus `verevon`/`verevon-auto`/`auto` → Balance). Pinned ids and `""`/`default` bypass the layer.
- `classify()` — deterministic heuristic scoring (total chars, last-user-turn length, code fences, whole-word reasoning keywords with boundary checks, tool use, conversation depth) → Simple/Moderate/Complex. Char-counted (not byte) so Norwegian text isn't over-classified.
- Budget posture via a **real** cost-core call: `BudgetClient` POSTs `COST_CORE_URL + /api/v1/budget/check` with a 400ms timeout, best-effort → `Healthy|Constrained|Exhausted|Unknown`. Any failure/absent org → `Unknown` (never blocks inference).
- `choose()` maps `(mode, complexity, posture)` through a `RoutingPolicy` table; `Exhausted` forces the `model-router` cheap fallback, `Constrained` downgrades one tier. The policy is hot-swappable (see below).

## Fallback chain — real

`provider/fallback.rs` (1111 lines): config-driven provider registration; per-provider bounded retries; provider-hint matching (hyphen/underscore-normalized after a Phase-3 regression); **model-family gating** (claude ids only hit Anthropic-shaped providers, everything else the OpenAI-shaped ones — avoids 404-ing the wrong deployment); unspecified-model → per-provider default resolution ("Verevon Auto"); prompt cache on the unary path; intent layer invoked at the top of both `infer` and `infer_stream`. Extensive unit tests. `[source-only]`

Routing policy is durable + hot-swappable: seeded from `RoutingPolicy::default` (overridden by env knobs), then a background loop polls session-core's `RoutingPolicy` gRPC every `ROUTER_POLICY_REFRESH_SECS` (default 60) and hot-swaps the live copy via `arc-swap`; the HTTP `PUT /internal/v1/router-policy` write-through persists to session-core and swaps immediately (503 when `SESSION_CORE_URL` unset). `[source-only]`

2026-07-13 correction: both routing-policy HTTP methods authenticate an exact `aud=inference-core` bearer and require exact scope `inference:policy:admin`; ordinary `inference:invoke` and prefix-like scopes do not authorize policy reads or writes. The subsequent session-core write still needs a dedicated session audience/caller migration before deployment, so this surface is source-hardened but not live-verified. `[source-test]`

## ZDR / EU residency (cross-plane rule)

- Embedding path enforces **EU residency deny-by-default**, in two places: (1) startup fail-loud `assert!` that refuses to boot a non-EU Azure embedding deployment unless `MODEL_PLANE_ALLOW_NON_EU_EMBEDDING=1`; (2) request-time gate in `create_embedding` that rejects a non-EU region *before any network call* → `ProviderError::ResidencyViolation` → gRPC `FAILED_PRECONDITION`. Deployed region is `swedencentral` (EU) so the gate passes. Tested (reaches-provider vs rejected-before-network). `[source-only]`
- `zdr` is monotonic from the verified issuer. Unary, streaming, embeddings, and
  every other modality reject unattested deployments before provider I/O. Direct
  OpenAI and Anthropic remain false; Azure OpenAI is false unless
  `AZURE_OPENAI_ZDR_CONFIRMED=true` is deliberately set from independent
  contractual evidence. The EU-region gate is separate and does not imply ZDR.
  Cache tests prove ZDR requests neither read nor write the prompt cache.
  Downstream session/tool/trace persistence and missing provider proof still
  prevent an end-to-end ZDR claim. `[source-test]`

## The gRPC surface the running binary serves

`grpc.rs` implements a full multimodal `InferenceCore` service, all backed by the real chains (no mocks): `infer`, `infer_stream`, `create_embedding`, `list_models`, `synthesize_speech`, `transcribe_speech`, `list_speech_voices`, `translate_text`, `batch_translate_text`, `detect_text_language`, `list_translation_languages`, `generate_image`, `analyze_image`, `extract_image_text`, `analyze_document`, `analyze_language`, `create_realtime_session`, `create_video_generation_job`, `get_video_generation_job`, `stream_video_generation_content`. Tenant `org_id`/`user_id` are extracted from gRPC metadata and threaded into the request for the intent/budget layer. Per-RPC input-size caps are enforced. `[source-only]`

model-gateway (THE chat entry point) consumes this over gRPC: `state.rs` builds `InferenceCoreClient` against `INFERENCE_CORE_URL`/`INFERENCE_CORE_ADDR` (default `http://localhost:9092`; compose sets `http://inference-core:9092`), and `/v1/invoke`, `/v1/invoke/stream`, `/v1/models`, embeddings, speech, vision, translation all proxy to it. So inference-core's gRPC is **load-bearing for the entire chat/inference loop.** `[source-only]`

Live end-to-end: model-gateway `/healthz` → 200; `/v1/models` (proxies inference-core `ListModels`) → **401** (model-gateway auth gate; an unauthenticated model list could not be obtained this pass). Combined with `:9092` being open, the running loop is wired; a fully authenticated live chat round-trip was not exercised here. `[live-curl]`

## Uncommitted WIP (CRITICAL — assess-WIP step)

`git status`/`git diff` for the service dir show four modified, uncommitted files: `src/main.rs`, `src/lib.rs`, `src/grpc.rs`, `Dockerfile`. The change **disables the gRPC inference server**: `[source-only]`

- `main.rs` deletes construction of every provider chain (chat + speech/translation/vision/doc-intel/language/realtime/video) and replaces the gRPC task with `tokio::spawn(async { std::future::pending().await })` (a task that never resolves), logging "inference-core gRPC is unavailable in the secure MVP because no dedicated signed audience contract exists."
- `lib.rs` demotes `pub mod grpc` to `#[cfg(test)] mod grpc` — the service is not compiled into the shipped binary.
- Only the HTTP health + routing-policy-admin server remains live; the `FallbackChain` is built solely to serve the policy GET/PUT and the session-core refresh loop. **No inference is served.**

Why it matters: model-gateway (committed *and* in its own working tree) still hard-dials `:9092` via `InferenceCoreClient` with no fallback and no coordinated de-wiring. If this WIP is built and deployed, `infer`/`infer_stream`/`create_embedding`/`list_models` (and thus chat, embeddings, speech, vision, translation, the model picker) all fail. The stated security rationale (the gRPC boundary is unauthenticated — no signed audience / ZDR-claim pinning) is legitimate, but the change removes the capability without providing the authenticated replacement, so it must NOT be shipped until model-gateway↔inference-core get an authenticated contract.

Why chat still works today: the running container was built 2026-07-09 from committed `main` (which has `pub mod grpc` + the wired chains) and still serves `:9092` (port open). The WIP is not in the deployed binary and cannot be built into it this pass (corrupted containerd blocks rebuild). This is a latent regression landmine, not a current outage. `[inspect]`+`[source-only]`

The working-tree WIP does compile: `cargo check -p inference-core` → **clean, exit 0** ("Finished dev profile in 1m 59s"). So there is no compile guard preventing an accidental build/deploy of the gRPC-less artifact. `[source-only]`

## Stub / mock / TODO scan (judged)

Grep of `todo|fixme|mock|stub|fake|placeholder|not-implemented|unimplemented|hardcoded|canned|dummy` across `src`+`tests`: `[source-only]`

- `grpc.rs` `Status::unimplemented(msg)` for `UnsupportedModel` — honest error mapping, not a stub.
- `speech.rs` `NoopSpeechProvider` + `#![allow(dead_code)]` "placeholder … wired up incrementally" — fail-closed guard/fallback, real Azure/OpenAI speech ops exist alongside it.
- `vision.rs` comment about a hardcoded default deployment — a code comment, real provider present.
- `tests/fallback_test.rs` `MockProvider`, and in-file `RecordingProvider`/`RecordingEmbedProvider` — legitimate test doubles under `#[cfg(test)]`/`tests/`.

No genuine runtime stub found in any shipping code path.

## Build / toolchain

`cargo check -p inference-core` on the working tree (WIP applied): **clean, exit 0, ~2 min** (compiles the workspace deps then the crate; no errors, no warnings surfaced by `--message-format short`). Host toolchain note: GNU `timeout` is absent on this macOS host (a first attempt no-opped); the real check was re-run without it. `[source-only]`

## Not owned by inference-core (routing the Phase-4 questions correctly)

- **HITL approval enforcement** (Phase-4 q4) is not inference-core's responsibility. inference-core performs pure inference and returns `tool_calls` to the caller; it holds no approval/tool-execution logic. HITL lives in execution-core/session-core — audit there.
- **Tool dispatch / shipping-core / MCP / "the Visma MCP"** (Phase-4 q2/q3) are execution-core + `bridges/mcp-bridge` concerns, not inference-core. inference-core only translates tool *definitions* to/from the provider wire shapes and surfaces model-requested `tool_calls`. There is no Visma or MCP wiring in this service.

## Doc-register reconciliation

Confirms the STALE_DOC_DELETION_REGISTER entries for Model: `docs/STUBS.md`, `docs/gap-model.md`, `docs/ARCHITECTURE.md` overstate stub status. For inference-core specifically, the "stub" framing is wrong — the multi-provider router, intent layer, fallback chain, and multimodal gRPC surface are all real. Those docs should be updated per the register, not treated as ground truth for this service. `[source-only]`

## Bottom line

inference-core is a real, non-mocked, multi-provider multimodal inference router with a genuine intent-based model selector, a robust fallback chain, and EU-residency enforcement on embeddings. The deployed service is healthy and serving. The one urgent item is process/release hygiene, not missing functionality: an uncommitted WIP guts the gRPC inference server that model-gateway depends on, and must not be built/deployed until an authenticated gateway↔inference contract lands.
