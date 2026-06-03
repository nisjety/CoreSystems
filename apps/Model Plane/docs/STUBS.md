# Model Plane — Stub & Degraded-Path Inventory

Audited 2026-06-03 by sweeping `rust/services`, `rust/clients`, `go/services`
for `unimplemented!`/`Status::unimplemented`/`todo!`/`stub`/`placeholder`/
"not implemented". This separates **genuine stubs** (non-functional code that
returns nothing real) from **correct config-gated degradation** (a clear
`Unimplemented`/`503` when an optional dependency isn't wired — *not* a stub).

The governing rule (per `/goal`): the right tool for the right job, no duplicate
systems, and never a path that *masquerades as success while doing nothing*.

---

## 1. Streaming — FIXED (this change)

**`model-gateway` `/v1/invoke/stream` (Rust `sse.rs`)** — was emitting an
immediate empty `done` (`model_used:"default"`, 0 tokens) whenever the
inference-core `InferStream` RPC was unavailable. That read as a *successful
empty completion*, forcing clients (e.g. quarry-edge) to invent their own
fallback. The streaming pipeline itself is real end-to-end (gateway → inference
`InferStream` → fallback chain → OpenAI/Anthropic SSE); the bug was the
gateway's **error-masking**.

**Fix:** on `InferStream` failure, fall back to the (working) non-streaming
`Infer` and reveal its real content in chunks (`chunk_for_stream`, lossless,
unit-tested). If `Infer` *also* fails, emit an honest `error` SSE event — never
a fake `done`. One robust endpoint; the per-client fallback is removed
(harmonization). Real token-streaming flows automatically the moment
inference-core's streaming provider is reachable; otherwise content still
arrives (chunked) instead of an empty stub.

> Note: this also fixes **chat-v2** follow-ups, which shared the same endpoint.

---

## 2. Genuine stubs — gated on external resources / design (NOT buildable-verifiable here)

| Stub | Location | Status & what it needs |
|---|---|---|
| **Tool executor** (`tool_bridge`) | `execution-core/src/executor.rs`, `runtime_loop/mod.rs` | Deterministic stub (canned outputs, spawns nothing). G1: needs a real process executor on Linux + the team's executor-design decision. `sandbox.rs` argv builder is built+verified; this is the consumer. |
| **Sandbox manager** | `go/services/sandbox-manager` | gRPC server returns `Unimplemented`. G1: ephemeral sandbox provisioning; pairs with the executor above. |
| ~~Voice TTS/STT~~ | — | **AUDIT CORRECTION (2026-06-03): NOT a stub.** Voice is real end-to-end — gRPC `text_to_speech`/`speech_to_text` (grpc.rs) + HTTP `/v1/ai/speech` (http_routes.rs) both proxy to inference-core's `SynthesizeSpeech`/`TranscribeSpeech`, which route through the real OpenAI + Azure `SpeechChain` (`provider/speech.rs`, real HTTP incl. multipart STT). Creds-gated at runtime like chat. The original flag was misled by **dead, uncalled** `handle_*_speech` stubs in `runtime_registries.rs` — now removed. |
| **Letta memory** | `go/services/letta-bridge` (`memstore`, `agentmemory`) | In-memory substring stub; upgradeable to a managed backend (Redis Agent Memory / Letta). Functional for dev, not production recall. |
| ~~capability-core `/commands`~~ | `internal/commands/handler.go` | ✅ **RESOLVED (2026-06-03).** `/models` and `/compact` now **delegate** to their canonical owners (inference-core `ListModels`, session-core `CompactNow`) via narrow injected clients — no duplicate logic, nil-safe (honest "unavailable" when unwired). `/help` was already real; `/budget` points to cost-core; unknown commands honestly report "no server-side action (client-handled)" instead of fabricating success. Wired in `main.go` via a single shared `dialBackends()` (also de-duplicated the learning consumer's dial). 6 unit tests vs fake clients. |
| **capability-core capability detail** | `internal/api/capabilities.go:177` | A route placeholder ("so the route exists"). Needs the real detail payload. |

## 3. Correct config-gated degradation — NOT stubs (leave as-is)

These return a **clear** `Unimplemented`/`503` when an *optional* dependency is
absent. That is the right behavior (fail loud, not fake success):

| Path | Location | Trigger |
|---|---|---|
| Go `model-gateway` proxy | `go/.../model-gateway` (`buildProxy`) | `Invoke`/`InvokeStream` → `Unimplemented` when `SESSION_CORE_ADDR`/`INFERENCE_CORE_ADDR` unset (cutover safety). Set both to enable. |
| Quarry edge tools | `model-gateway/tools.rs`, `grpc.rs` | `Unimplemented("quarry edge not configured")` when `QUARRY_*` unset. |
| LSP bridge | `model-gateway/lsp.rs:162` | `Unimplemented` when `LSP_BRIDGE_URL` unset. |
| MCP transport | `runtime_registries.rs:402` | `Unimplemented` for transports other than `http`/`stdio` (both implemented). |
| Run-events SSE | `go/.../internal/sse/sse.go` | `503` when orchestrator-core client is nil. |

## 4. Duplicate-system — RESOLVED (Go `model-gateway` removed, 2026-06-03)

There were **two** `model-gateway` services. Resolution: the **Rust**
`rust/services/model-gateway` is the canonical front-door — it is the only one
built by **any** compose (root `mp-model-gateway`, `deploy/`, and consumed by
Application/Control Plane via DNS), and it covers the full surface
(`Invoke`/`InvokeStream` + Quarry `Fetch`/`ExtractStructured` + SSE + auth +
registries + voice + MCP + plan-mode). The **Go** `go/services/model-gateway`
was a v2-cutover proxy shim that only duplicated `Invoke`/`InvokeStream` (+ a
superseded Quarry `Fetch`/`ExtractStructured`).

Verified dead before removal: **no compose builds it** (all build the Rust one),
**no Go package imports it**, and its "v2 fallback" was vestigial (nothing
deployed it to *be* a fallback). Removed the directory + its `go.work` entry;
all remaining Go modules build + vet clean. Same safe-retirement pattern used
for the orphaned `task-core` (matrix §4.2). One canonical gateway — duplication
gone.
