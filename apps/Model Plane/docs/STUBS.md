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
| **Voice TTS/STT** | `model-gateway/runtime_registries.rs:956-973` | Returns `Unimplemented` until inference-core's speech provider is wired. inference-core has the RPCs (`SynthesizeSpeech`/`TranscribeSpeech`) + a placeholder provider (`provider/speech.rs`); needs a real provider impl + creds. |
| **Letta memory** | `go/services/letta-bridge` (`memstore`, `agentmemory`) | In-memory substring stub; upgradeable to a managed backend (Redis Agent Memory / Letta). Functional for dev, not production recall. |
| **capability-core `/commands`** | `internal/commands/handler.go:94` | `/models` is real; other slash-commands return an acknowledgement placeholder. Needs per-command handlers. |
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
