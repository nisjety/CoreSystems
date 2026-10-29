# Model Plane Audit

Baseline date: 2026-07-02
Live verification dates: 2026-07-10 (first pass) and 2026-07-11 (full 11-service + tool-path-trace re-verification, this pass)

Scope: `apps/Model Plane`

See also `MODEL_PLANE_STATUS.md` and `MODEL_PLANE_ROADMAP.md` at the plane root, and the per-service docs in `docs/core-research/` (all re-verified 2026-07-11).

## 2026-07-11 re-verification — executive summary (answers the user's two questions)

Verified with host-curl + source/config reading (Docker `exec`/rebuild/`logs` broken by containerd corruption — "(unhealthy)" flags are the exec-based healthcheck failing, not the services; all live services returned 200 on their real health paths). Findings graded `[live-curl]` / `[source-only]` / `[inspect]`.

### The Model Plane tool loop is REAL and non-mocked — the user's failures have two *different*, non-loop causes

A dedicated end-to-end source trace (chat → model-gateway → execution-core → tool target) plus the 2026-07-10 live reproduction establish:

1. **"shipping time Oslo→Trondheim" failed because normal chat sends ZERO tools.** The gateway SSE handler branches on request `features[]`: business tools are only offered on the **agentic** path (Plan mode / the Agent Run Console) or when the user toggles web/actions. The verevonv3 SPA's `buildChatWireBody` sets `DEFAULT_FEATURES = ['usage','citations','reasoning','steps','artifacts']` — **no `tools`, no `agentic`** — so a plain "shipping time…" question is answered from the model's own weights and never calls `get_shipping_quotes`. This is a **frontend/UX wiring gap, not a backend defect**. And even on the agentic path, the answer was gated by the shipping-core Bring transit-day defect (Phase 3, now fixed in source). The user was also on **deprecated verevonv2** (Phase 1), a near-toolless wrapper, compounding the impression.
2. **"test the Visma MCP" failed because there is NO Visma wiring in the Model Plane at all.** `grep -rni visma` across all Model Plane `.rs/.go/.ts/.js/.json/.yaml/.env` = **zero code matches**. MCP bridging itself is genuinely implemented (a real generic JSON-RPC 2.0 client in model-gateway: stdio+http transports, `tools/list` discovery, `tools/call` proxy, `mcp__<server>__<tool>` namespacing, capability-core write-through; `bridges/mcp-bridge` is a working reference server that is **not deployed** — opt-in compose only). The single live `visma mcp` registry record is a **user-created, malformed entry** (transport `stdio` + an HTTPS URL + empty allowlist) that can never discover or execute a tool. The Visma Net MCP that does exist (`visma_net_mcp` + `visma-salgsordre-test` skill) is a **claude.ai/Claude-Code connector — assistant-side, entirely separate from Verevon's runtime**. So testing the Visma MCP is not a Model Plane capability today; the plumbing exists but no working Visma server is wired.

**HITL is NOT decorative** on the agentic chat path (this corrects a prior blanket finding): risky tools (`book_shipment`, `execute_provider_action` writes, `publish_social_post`, `browser_agent`, shell, all `mcp__*`) are gated **before** execution — `runtime_loop` returns `awaiting_approval`, mints a durable session-core approval, and pauses the run; the gateway hardcodes `mode:"ask"` for chat agentic runs and `posture_floor` only ratchets stricter. **Caveats**: under `auto` posture (plain interactive chat, if tools were enabled) risky tools run immediately by design (human present); and the standalone `ExecuteStep` gRPC defaults to `Auto` (ungated) for an empty mode, so an orchestration caller that omits the mode would run risky writes unapproved — the chat path is safe, the orchestration step path depends on the caller.

### Findings by service (2026-07-11)

| Service | State | Key findings |
|---|---|---|
| **model-gateway** :8080/:9090 | Real, hardened | Chat entry point genuinely wired (forwards to inference-core gRPC; real 3-round in-gateway function-calling loop; ZDR-first branch). Auth hardened (RS256 JWKS, alg-pinned, canonical service identity + `models:invoke` scope, monotonic ZDR, verified delegated Data Plane bearer — the Phase-2 ZDR-into-grounding gap is closed in source). ~30% fine-tune poller error rate (non-chat). **WIP landmine (see below).** |
| **inference-core** :9092/:18082 | Real, multi-provider | Real Anthropic/Azure-Foundry/OpenAI clients, FallbackChain, intent-based model selection, EU-embedding residency enforced deny-by-default. No canned paths. Effective providers: azure-openai + azure-anthropic (direct Anthropic bypassed, out of credit). **WIP landmine (see below).** |
| **execution-core** :9093/:18083 | Real tool loop | Genuine ReAct agent loop, 15 built-in tools purpose-locked + name-dispatched, wired to live backends (`shipping_tools`→shipping-core :3156, integration/info/social/browser/knowledge). HITL enforced under `ask`. Prior ExecuteStep user-isolation leak remediated (threads `user_id`). **WIP landmine (see below).** |
| **session-core (MP)** :9091/:18081 | Real, durable | Postgres-backed durable sessions/runs/approvals; hot-path RPCs clean; approvals genuinely persisted + signalled. **NEW: compaction background loop 100% failing (0 ok / 3265 err, never once succeeded)**; get_policy ~47%, finetune ~30%, dreaming ~30% error rates (root cause needs DB access — blocked). Run read-model (`get_run`/`list_runs`/`cancel_run`) has no tenant scoping and no gRPC auth interceptor (relies on gateway); ZDR flag not persisted on the event log. |
| **orchestrator-core** :8084/:9080 | Real proxy + dormant workflows | Live gRPC proxy to session-core (used on the chat-approvals path). Temporal workflows are real code but **dormant** (nothing starts them; live loop uses execution-core `RunAgent`). `AutoresearchWorkflow` defined+tested but **not registered**. `CAPABILITY_CORE_ADDR` code default `:9092` collides with inference-core (saved only by a compose override to `:9097`). |
| **capability-core** :8085/:9097 | Audited, doc written | Tool/capability registry. Doc rewritten 2026-07-11; the agent's final structured summary hit the retry cap so its findings aren't in this synthesis table — see `docs/core-research/capability-core.md`. |
| **cost-core** :8089 | Real ledger, unsafe + degraded | Durable Postgres ledger + authoritative pricing (14 rates live). **HIGH: zero inbound auth** — unauthenticated cross-tenant read (IDOR via query params) AND write (`POST /cost/record`); callers send no key either. **HIGH: ledger currently unreachable** (Postgres no-route → cost not being persisted right now; `/pricing` still 200 masks it). Budget check **fails open** on ledger error; 500 body **leaks the DB DSN**. |
| **bridge-core** :8091/:9100 | Live shell, not in chat path | In-memory session bookkeeping; **HIGH: HTTP API completely unauthenticated, tenancy is attacker-controlled** (org_id query param → cross-org enumeration). All channels use the noop ECHO adapter; gRPC :9100 registers zero services; voice/WebSocket are skeletons. Nothing in the system calls it. |
| **browser-broker** :8087/:9095 | Real lifecycle, island | Real grant lifecycle (crypto IDs, TTL, revocation) but **no callers, no Quarry-v2 integration** (opaque tokens with placeholder scope/endpoint), and the rate-limiter + metadata-scrubber interceptors are **defined but never wired** into the server. |
| **sandbox-manager** :8086/:9094 | Real bookkeeping, no provisioning | Lease/snapshot bookkeeping is real+tested, but provisions nothing (fabricated `sandbox://`/MinIO endpoints, in-memory only), has **zero callers**, and reads none of its promised Redis/MinIO durability env. Dormant. Real agent code-exec lives in execution-core's bubblewrap path. |
| **letta-bridge** :8088/:9096 + agent-memory :8100 | Real memory, search down | Real 3-tier memory bridge (Redis-vector via agent-memory-server + Postgres + in-memory), genuinely called by session-core on read+write. **HIGH: semantic search returns 500 on every query** (agent-memory-server embedding-creds mismatch — sidecar defaults to OpenAI while the plane runs Azure), so long-term recall is non-functional; fails safe (chat not broken). |

### CRITICAL cross-service WIP landmine — do NOT rebuild/deploy the Model Plane gRPC services as-is

A large uncommitted "secure-MVP" WIP (the same hardening theme as Data Plane's 2026-07-11 remediation) is present across **model-gateway, inference-core, and execution-core**. It demotes each service's gRPC server to `#[cfg(test)]` (replacing `grpc::serve` with `std::future::pending()`), i.e. **removes the gRPC surfaces the running containers still serve** (model-gateway :9090, inference-core :9092, execution-core :9093). It compiles clean (`cargo check` exit 0), so there is no compile guard. **If built and deployed as-is it breaks the entire chat/inference/tool loop** — model-gateway hard-dials inference-core :9092 and execution-core :9093 with no fallback, and in execution-core the security hardening in the same diff only compiles under `#[cfg(test)]`, so it would never actually run. The running containers predate the WIP, so chat works today. Recommendation: split the change (land the auth/ZDR hardening on a live surface; do not remove gRPC until every internal caller is migrated and coordinated), then rebuild `--no-cache`. This is the Model Plane analogue of the Data Plane WIP landmines and the single highest-risk item in the plane.

### Docs

All 11 per-service core-research docs rewritten (2026-07-11). Top-level docs edited in place (update): `MODEL_PLANE_DEEP_DIVE.md`, `README.md`, `docs/STUBS.md`, `docs/gap-model.md`, `docs/ARCHITECTURE.md`, `docs/chat-parity-audit.md`, `docs/MODEL_PLANE_V2_PARITY.md`, `docs/ROADMAP.md`, `docs/capability-ownership-matrix.md`, `feature.md` — all confirmed the register's note that the Model docs overstated stub status now that services are live; corrected in place with dated banners. The register's existing `review`/`update` verdicts for `STUBS.md`/`gap-model.md`/`ARCHITECTURE.md` are confirmed as `update` (done).

---

## Prior passes (preserved below)

## Live Docker verification addendum — 2026-07-10

Model Gateway, inference-core, execution-core, session-core, orchestration, capability, browser, sandbox, cost, bridge, and memory containers were healthy. Authenticated v3 model catalog and chat calls returned live Azure GPT-4o-mini telemetry (tokens, latency, and cost), proving inference is not a canned mock. The cross-plane smoke suite passed 6/6.

Normal chat reproduced the reported failure: zero tool calls and zero citations, followed by a model statement that it cannot access live systems. Browse mode called live web search/fetch and emitted five citations. Knowledge search called once but returned an empty result because the seeded organization has no indexed documents. Plan-mode shipping called `shipping_carriers` and `get_shipping_quotes` and returned live/demo carrier results without booking.

The live MCP registry contains one enabled organization record named `visma mcp`, configured as `stdio` with an HTTPS URL and no allowlist. Discovery fails because transport and URL disagree; no MCP tool was executed. Plan observability is inconsistent: session events contain tool outcomes, while the main SSE has no tool events, usage is zero, plans remain draft/running, and the run API can say completed with no final output. Session logs also show NATS heartbeat, checkpoint, memory, and local retrieval fallback errors despite container health.

This is a plane-local audit report. It separates Rust runtime status from Go control-service status because the gates currently differ.

## Current Shape

Model Plane owns reasoning, sessions/runs, inference, execution loops, capabilities, sandboxes, browser grants, bridge services, and cost. Rust services cover the hot reasoning/runtime path. Go services cover Temporal orchestration, registries, sandboxes, browser grants, memory bridge, cost, and bridge boundaries.

## Commands Run

| Command | Result | Notes |
|---|---|---|
| `cargo test --workspace` in `rust` | Pass | Full Rust workspace passed; live-DB/browser/doc examples are intentionally ignored. |
| `go test ./...` across Go core packages and `services/orchestrator-core` | Fail | Core packages passed; `orchestrator-core/internal/orchestration` test stubs miss `ListPendingApprovals`. |
| `go test ./...` across remaining Go services | Fail | capability/browser/sandbox/cost/bridge passed; `letta-bridge/internal/memstore` failed time-range filtering. |

## Static-Scan-Heavy Addendum

Additional scans run after the initial plane audit:

| Command | Result | Notes |
|---|---|---|
| `cargo fmt --all -- --check` in `rust` | Fail | Formatting drift in execution-core and model-gateway modules. |
| `cargo clippy --workspace --all-targets -- -D warnings` in `rust` | Fail | `execution-core/src/promote_on_use.rs:22` hits `clippy::doc_markdown` for `web_fetch`. |
| `gofmt -l .` in `go` | Fail | Formatting drift in `pkg/natsx`, `pkg/quarry`, bridge, capability, orchestrator, and test files. |
| Go vet over Model Go modules | Fail | `orchestrator-core/internal/orchestration` test stub does not implement `ListPendingApprovals`. |
| `go test ./...` over actual Go services/packages | Fail | `pkg/natsx`, `pkg/quarry`, bridge, browser, capability, cost, sandbox pass; `orchestrator-core` and `letta-bridge` fail. |
| `staticcheck` over Go services/packages | Blocked | Local staticcheck was built with Go 1.25 and cannot analyze Go 1.26 source. |

## Live Validation Addendum

Additional validation run against the local Model Plane scripts:

| Probe | Result | Notes |
|---|---|---|
| `bash scripts/verify-durable-layer.sh` | Pass | Script now waits for successful SQL readiness against `session_core` before migrations; durable-layer assertions pass against throwaway Postgres. |

## High-Confidence Findings

| Priority | Finding | Evidence | Recommended action |
|---|---|---|---|
| P1 | `orchestrator-core/internal/orchestration` tests no longer compile against the generated orchestration client interface. | `handlers_test.go` stubs do not implement `ListPendingApprovals`. | Update fake clients or generate compliant mocks, then rerun `go test ./...` in `services/orchestrator-core`. |
| P1 | `letta-bridge/internal/memstore` time-range filtering includes an old record. | `TestTimeRangeFiltering/cutoff_excludes_old_record` got 2 hits and wanted 1. | Clarify cutoff inclusivity/time source behavior and fix the filter or fixture. |
| P1 | Memory consolidation and skill promotion activities are still documented as placeholders. | `apps/Model Plane/README.md` lists orchestrator memory consolidation and skill promotion activities as placeholder implementations. | Replace placeholders or gate dependent product features. |
| P1 | Rust runtime is green, but Model Plane as a whole is not green because Go gates fail. | Rust workspace passed; Go failures above. | Treat Rust and Go status separately in release checks. |
| P2 | Rust formatting and clippy gates are not clean despite tests passing. | `cargo fmt --all -- --check` fails; clippy blocks on a doc markdown warning in `promote_on_use.rs`. | Apply mechanical formatting and patch the doc comment before using Rust test green as release evidence. |
| P2 | Model Go formatting drift spans packages and services. | `gofmt -l .` reports drift in `pkg/natsx`, `pkg/quarry`, bridge, capability, and orchestrator files. | Run `gofmt` as a mechanical cleanup. |
| P2 | Full live cross-service validation remains incomplete. | Model README still lists `model-gateway -> session-core -> inference-core -> execution-core` validation as remaining work. | Run the verification plan with live service dependencies. |
| P2 | Auth/security hardening remains a documented follow-up. | Model README lists issuer/audience/key rotation/claim tests and malformed token coverage. | Convert security hardening list into automated tests where not already covered. |
| P3 | Go static analysis is blocked by local analyzer/toolchain skew. | Staticcheck reports Go 1.26 source requiring a newer analyzer than the installed Go 1.25-built binary. | Upgrade/reinstall staticcheck with the active Go toolchain and rerun. |

## Needs Review

| Item | Why uncertain | How to verify |
|---|---|---|
| Live Temporal workflows | Unit tests passed/fail as above, but live Temporal stack was not started. | Run compose and workflow smoke tests. |
| Browser broker with Quarry | Rust browser-agent e2e test is intentionally ignored without live quarry-edge/inference-core stack. | Run the live e2e once Ingestion Plane compile gates pass. |
| Cost and capability service release readiness | Service-local tests passed, but integration contracts were not exercised here. | Run cross-service contract tests and NATS/Temporal smoke tests. |

## Quality Gate

- Rust workspace tests: pass.
- Durable-layer integration script: pass.
- Rust format/clippy: fail.
- Go package/service tests: fail in orchestrator-core and letta-bridge; other checked packages/services pass.
- Go vet: fail due the same orchestrator-core stub interface drift.
- Go format: fail.
- Staticcheck: blocked by analyzer/toolchain mismatch.
- Live stack verification: not run.

## Recommended Remediation Order

1. Update orchestrator-core test stubs for `ListPendingApprovals`.
2. Fix letta-bridge memstore cutoff filtering.
3. Decide whether placeholder memory consolidation/skill promotion paths should be implemented or feature-gated.
4. Run Model Plane Go tests end to end.
5. Run live cross-service verification from `docs/VERIFICATION.md`.
