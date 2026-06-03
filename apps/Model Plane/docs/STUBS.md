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

## 2. Originally-flagged stubs — audit outcome

**Conclusion (2026-06-03):** after tracing each, **none were fake / missing-implementation stubs over a real backend.** They were error-masking, dead code, mislabeled-but-functional, or a design fork. Resolved or corrected: streaming, voice, `/commands`, audit-log, **tool executor (G1)**, **letta memory**. The only genuinely-unbuilt piece is **sandbox-manager *provisioning*** — and that is a deliberate isolation-model decision (per-call isolation already works in-process), not a gap to blindly fill. Net: the model plane is functionally built; "small but powerful" holds.

| Item | Location | Status & what it needs |
|---|---|---|
| ~~Tool executor~~ (G1) | `execution-core/executor.rs` + `runtime_loop/mod.rs` | **AUDIT CORRECTION (2026-06-03): NOT a stub — built, wired & tested.** `execute_sandboxed` (build argv → `wrap_command` → spawn → capture → secret-scrub) IS wired into the `shell` tool in `runtime_loop::execute_step`, under a ReadOnly+no-network policy and the same permission/hook gates as every tool. 58 execution-core tests pass incl. `shell_tool_runs_a_real_process` (real `echo`), nonzero-exit, deny-mode, secret-scrub — cross-platform via passthrough. bwrap **isolation** is Linux-verified separately (`scripts/verify-sandbox-isolation.sh`). The deterministic `tool_bridge` path only serves NON-shell tools (deterministic by design). Per-call policy field is a future extension. |
| **Sandbox manager** (partial) | `go/services/sandbox-manager` | Lease + snapshot **bookkeeping is real** (`AcquireLease`/`ReleaseLease` + lease/snapshot stores); only actual sandbox **provisioning** (exec/docker) is absent — not a fake-success stub. **Design fork, not a gap to blindly fill:** per-tool-call isolation is ALREADY covered by the in-process executor (bwrap via `wrap_command`); a separate *provisioned-sandbox* model (long-lived per-session workspaces) is an undecided architecture call. Building provisioning now risks a **duplicate** isolation system (which the directive forbids). Needs the per-call-vs-per-session isolation-model decision + Linux to verify. |
| ~~Voice TTS/STT~~ | — | **AUDIT CORRECTION (2026-06-03): NOT a stub.** Voice is real end-to-end — gRPC `text_to_speech`/`speech_to_text` (grpc.rs) + HTTP `/v1/ai/speech` (http_routes.rs) both proxy to inference-core's `SynthesizeSpeech`/`TranscribeSpeech`, which route through the real OpenAI + Azure `SpeechChain` (`provider/speech.rs`, real HTTP incl. multipart STT). Creds-gated at runtime like chat. The original flag was misled by **dead, uncalled** `handle_*_speech` stubs in `runtime_registries.rs` — now removed. |
| **Letta memory** (functional dev backend) | `go/services/letta-bridge` (`memstore`) | **NOT a fake stub — a working in-memory store** (`Store`/`Search` do real substring recall over stored memory blocks, behind a `Store` interface). Swapping in Redis Agent Memory / Letta for **durable + semantic** recall is an *enhancement* (needs Redis/Letta), not a stub fix. |
| ~~capability-core `/commands`~~ | `internal/commands/handler.go` | ✅ **RESOLVED (2026-06-03).** `/models` and `/compact` now **delegate** to their canonical owners (inference-core `ListModels`, session-core `CompactNow`) via narrow injected clients — no duplicate logic, nil-safe (honest "unavailable" when unwired). `/help` was already real; `/budget` points to cost-core; unknown commands honestly report "no server-side action (client-handled)" instead of fabricating success. Wired in `main.go` via a single shared `dialBackends()` (also de-duplicated the learning consumer's dial). 6 unit tests vs fake clients. |
| ~~capability-core audit-log read~~ | `internal/api/capabilities.go` | ✅ **RESOLVED (2026-06-03).** `GET /api/v1/capabilities/audit` was a hardcoded empty placeholder; now queries `registry_audit_log` via the new `CapabilitiesStore.QueryAuditLog` — newest-first, filterable by `?entity_kind=`/`?entity_id=`, `?limit=` clamped to [1,500], parameterized (no injection), `diff_json` embedded as JSON. Integration-tested round-trip (append → query → ordering/filter/limit) against real Postgres via testcontainers (`-tags=integration`). |

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
