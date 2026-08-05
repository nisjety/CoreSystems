# execution-core Research Dive

Generated: 2026-07-11 (supersedes the 2026-06-09 pass)

Scope: `apps/Model Plane/rust/services/execution-core`

## 2026-07-16 delta

At 2026-07-16 20:05 CEST, the local integration container is healthy with
restart count zero and additive gRPC `:9093` is loopback-reachable;
unauthenticated dispatch rejects. A valid execution identity reached Capability
Core, `cap.retrieval.query` denied as `health_not_attested`, and wrong-org reuse
was denied. Forged approval decisions did not change durable counts. This is
fail-closed live evidence, not a successful tool or approval continuation.
Capability Core policy is now enforced before every direct and
agentic dispatch: deny/outage fail closed and `ask` enters the approval gate.
Session stores one content-free delivery intent per grant, but no authenticated
dispatcher can restart and acknowledge the exited agent continuation. The outbox has
claim/lease, bounded retry/backoff, poison/terminal, and acknowledgement-state
primitives, but there is no restartable descriptor, authenticated dispatcher,
or success receipt. Approval resume returns `Unavailable` rather than merely
flipping process-local state and claiming success. Signed ZDR is preserved at
execution ingress and delegation; a caller cannot downgrade it before dispatch.
Knowledge search emits typed hybrid results, guides bounded
reformulation/backtracking, suppresses exact repeats, distinguishes degraded
from empty, and redacts durable tool content under ZDR. Structured/tabular,
graph, vector-only, and MCP retrieval are not configured. Final candidate test
evidence is now source-only: at 15:10 CEST the execution all-target suite
passed (225 library + 17 integration tests; one live Quarry test intentionally
ignored), but no positive live execution/HITL/retrieval proof exists. Browser results
are typed across loop/bridge/runtime: only explicit success becomes completed;
approval denial, timeout, and resource exhaustion fail; cancelled/aborted work
remains cancellation. This does not solve P0 managed-run terminalization:
Execution still lacks a durable Session Core outcome receipt/reconciliation
contract after a producer crash or ambiguous terminal RPC response.

## 2026-07-13 secure-MVP correction

The historical WIP description below is retained as audit history. Current
source restores the additive `:9093` gRPC service with exact execution ingress
identity plus separately verified Data Plane, session, and inference delegated
credentials. Run ownership is verified through Session Core, ZDR durable tool
runs fail closed, and approval resume is now an atomic state transition that
accepts only `AwaitingApproval` or `Paused`. `Completed`, `Failed`, `Cancelled`,
`Running`, and unknown runs return `resumed=false` without mutation, so an
identical approval replay cannot reactivate terminal work.

Earlier command counts below are historical and must not be reused for the
current uncommitted aggregate. This remains source evidence only. No rebuild or
deployment occurred, `ResumeRunRequest` still has no approval ID for durable
approval-to-resume binding, and continuation dispatch still requires a durable
descriptor, authenticated dispatcher, and success receipt. The do-not-rebuild
decision remains binding.

Audit environment caveat: Docker's containerd content store is corrupted this
pass — `docker exec`/`build`/`logs` fail fleet-wide. Every finding is graded
`[live-curl]` (host curl / gRPC probe to a published port), `[source-only]`
(read from disk), or `[inspect]` (`docker ps`/`docker inspect` config+state, no
exec). Live checks reflect the **running** container, which was built from
**committed** code days ago; there is a large **uncommitted WIP** in the working
tree that changes the picture materially (see "Uncommitted WIP" below).

## Bottom line

`execution-core` is a **real, non-mocked governed agent tool-execution loop** —
not branding, not a stub. The ReAct driver (`runtime_loop::agent::run_agent`)
calls inference-core over gRPC, dispatches model-requested tool calls through a
permission/hook-gated path (`runtime_loop::execute_step`), feeds outcomes back,
and finalizes durably through session-core. Fifteen built-in tools are really
wired to live backends (shipping, provider-actions, Norwegian info APIs, org RAG,
web, shell, browser), plus an MCP proxy. All `mock`/`stub`/`unimplemented`
strings in the tree are inside `#[cfg(test)]` blocks or doc comments — there is
no production stub in the tool path. `[source-only]`

The headline nuance is a **deployment split**, not a fake:

- **Running container (committed code):** gRPC ExecutionCore is **live on
  :9093** and is an active in-fleet dependency (orchestrator-core dials
  `execution-core:9093`). `[live-curl]`
- **Working-tree WIP:** deliberately **removes the entire gRPC surface**
  ("secure MVP — no dedicated signed audience contract exists yet"). If built and
  deployed, execution-core would serve HTTP health only and `RunAgent`/
  `ExecuteStep` would go dark. The same WIP *also* hardens those handlers
  (verified user bearer required, ZDR fail-closed, per-user scoping) — but that
  hardening then only compiles under `#[cfg(test)]`. There is no single build
  that has both a live surface and the hardened handlers. `[source-only]`

## Live health

- `[live-curl]` HTTP health: `GET http://localhost:18083/healthz` → `200 "ok"`
  (also `/readyz`, `/metrics`). Container maps `18083:8083`.
- `[live-curl]` gRPC on `:9093`: TCP open; `curl` gets raw HTTP/0.9 (h2c), and
  `grpcurl -plaintext localhost:9093 list` returns a **gRPC-layer** error
  ("server does not support the reflection API"). Only a real gRPC server returns
  that — the ExecutionCore service is genuinely listening. (Reflection is off, so
  RPCs can't be enumerated without the protos.)
- `[inspect]` `docker ps` shows `Up 2 days (unhealthy)`. The "unhealthy" is a
  false signal from the corrupted store: the healthcheck is
  `curl -f http://localhost:8083/healthz` run via `docker exec`, and
  `docker inspect` shows every probe failing with
  `exec /usr/bin/curl: input/output error` (FailingStreak 5601) — an exec I/O
  error, not a service error. Host curl to the same endpoint succeeds.

## Runtime shape (source)

Entrypoint `src/main.rs` (working tree): spawns HTTP health (`http_health::serve`
on :8083) and a **`pending()` no-op in place of the gRPC server**. `src/lib.rs`
gates the `grpc` module to `#[cfg(test)]`. Committed `main.rs`/`lib.rs` instead
call `grpc::serve(state)` and export `pub mod grpc` — that is what the running
binary does.

Tool loop (all committed, present in the running binary):

- `runtime_loop/agent.rs` (1754 lines) — `run_agent`: purpose-locked, capped
  ReAct loop. Offers the merged tool defs, calls `InferenceCore.Infer`, dispatches
  each requested tool via `execute_step`, frames outcomes back, finalizes with one
  terminal `CompleteStep` + plan transitions. HITL pause + durable approval on a
  gated tool. Graceful-failure path guarantees a run is never left `queued`.
- `runtime_loop/mod.rs` (1036 lines) — `execute_step`: hook gate → permission
  gate → per-tool dispatch. `shell` runs a real sandboxed process; every other
  live tool has a dedicated async arm; unknown tools fall to the deterministic
  `tool_bridge`.
- Tool clients: `shipping_tools.rs`, `integration_tools.rs`, `info_tools.rs`,
  `knowledge_tools.rs`, `web_tools.rs`, `social_tools.rs`, `quarry_agent.rs` +
  `browser_agent.rs`, `mcp_gateway.rs`, `executor.rs` (sandbox), `scrub.rs`.
- `permission/mod.rs` — the gate. `grpc.rs` — the RPC surface (`ExecuteStep`,
  `RunAgent`, `Resume/Cancel/Pause`).

## Phase-4 answers

**1. Is the model-gateway → execution-core agent/tool loop real and non-mocked?
YES.** `[source-only]` `run_agent` drives a genuine multi-round loop against
`InferenceCore.Infer` (real inference-core), dispatching tools through the same
gated `execute_step` the `ExecuteStep` RPC uses, persisting via session-core.
The only mocks are in-process tonic test servers under `#[cfg(test)]`.
`tool_bridge::execute` is a deterministic echo, but it is only the **fallback**
for tool names with no dedicated arm — every real tool bypasses it.
Note: model-gateway also has its own in-process tool loop (agent.rs comments say
this "mirrors model-gateway's `run_tool_rounds`"), so the interactive chat path
may run tools inside the gateway rather than round-tripping to exec-core; the
exec-core `RunAgent` path is the durable/agent-run dispatcher. Confirming which
path a given chat uses requires reading model-gateway (out of this service's
scope). `[source-only]`

**2. Which tools are registered and how are they resolved?** `[source-only]`
`offered_tool_defs()` returns 15 built-ins: `yr_weather`, `traffic`, `news`,
`track_shipment`, `company_lookup`, `get_shipping_quotes`, `shipping_carriers`,
`book_shipment`, `list_social_accounts`, `publish_social_post`,
`knowledge_search`, `list_provider_actions`, `execute_provider_action`,
`web_search`, `web_fetch`. `merged_tool_defs()` appends the org's registered MCP
tools (namespaced `mcp__<server>__<tool>`) discovered best-effort via the gateway.
The offered set **is** the purpose-lock allowlist: a model call to any un-offered
name is rejected, never dispatched. Resolution in `execute_step` is a name-keyed
if/else chain; `shell`→sandbox, `browser_agent`→Quarry loop, `mcp__*`→gateway
proxy, else the matching client, else `tool_bridge`.

**3. Does it reach shipping-core :3156 via `SHIPPING_CORE_URL`? YES.**
`[source-only]+[live-curl]` `shipping_tools.rs` reads `SHIPPING_CORE_URL`
(compose default `http://host.docker.internal:3156`) and POSTs `/api/quotes`,
GETs `/api/carriers`, and does the two-step `/api/bookings[+/confirm]`. Live:
`GET http://localhost:3156/api/carriers` → `200` with a real fleet (bring/dhl
`is_mock:false`; postnord/dsv/helthjem/porterbuddy `is_mock:true`). Compose wires
the URL for the exec-core container. The `get_shipping_quotes` tool is therefore
capable end-to-end; the earlier "shipping time Oslo→Trondheim" failure was the
shipping-core Bring delivery-time defect (Phase 3, fixed in source), not an
exec-core loop defect.

**4. Is there ANY MCP client / Visma tool here? MCP proxy: yes. Visma: NO.**
`[source-only]` `mcp_gateway.rs` is a **proxy only** — execution-core holds no
MCP registry (per the capability-ownership matrix §G2 the gateway owns MCP
registration/discovery). It dials model-gateway gRPC `ListMcpTools` (to offer)
and `ProxyMcpTool` (to execute). So exec-core surfaces whatever MCP servers
model-gateway's registry returns for the org — and a repo-wide
`grep -rni visma` across the whole Model Plane (`.rs/.go/.ts/.yml`) returns
**0 code matches**. There is no Visma MCP server, bridge, token, or tool wired
anywhere. "Test the Visma MCP" is **not a Model Plane capability today** — it is
not an exec-core bug. (The `visma_net_mcp` connector and `visma-salgsordre-test`
skill that exist are assistant-side / claude.ai connectors, not Verevon runtime.)
Sibling docs `bridge-core.md`/`model-gateway.md` corroborate: `bridges/mcp-bridge`
ships only `fs`+`git` servers and the one `visma mcp` registry record is
malformed (`transport: stdio` + an HTTPS URL), so discovery fails before any call.

**5. Is HITL enforcement real or decorative?** `[source-only]` **Real at the
exec-core boundary under `ask`; intentionally open under `auto` (chat).**

- The permission gate (`permission::evaluate_call`) runs **before** dispatch. Under
  `ask` (the default posture in `run_agent` for deployed agents) a risky tool
  returns `AwaitApproval` and the tool function is **never called** — the run
  pauses and a durable `CreateApproval` is minted (idempotent on
  `{run_id}:{step_id}`). `is_risky_tool` covers destructive/outbound verbs,
  `book_shipment`, `publish_social_post`, `browser_agent`, and all `mcp__*`
  (external MCP is gated unconditionally). `execute_provider_action` is
  operation-aware: reads proceed, writes/unknown/malformed fail safe to gated.
- Provider writes are **fail-closed on a real durable approval**:
  `resolve_write_approval` forwards a genuine session-core approval id
  (`appr_…`), never a shared constant; under `ask` the record must already be
  human-`Granted` or the write is blocked. (Committed — present in the running
  binary.)
- Under `auto` (interactive chat) nothing is gated: risky tools run immediately,
  and provider writes auto-record the interactive user as the live approver.
  That is by design (a human is present), not decorativeness.
- Cross-plane containment update (Application/Ingestion source, 2026-07-13):
  integration-corev2 no longer accepts `approvalId` as authority. Effectful
  writes require a tenant-bound service bearer plus a short-lived Ed25519 proof
  bound to the exact provider effect and a durable authorization. The only
  trusted issuer currently implemented is conversation-core. Execution-core
  does not yet issue that contract, so Model-originated provider writes now fail
  closed against the changed Integration source—including `auto` posture—until
  session-core/execution-core persist the same binding and issue an attestation.
  The running old Integration workload still has the presence-only behavior;
  neither side of this source contract is deployed.

**6. ExecuteStep user-isolation leak (org- vs user-scoped): remediated.**
`[source-only]` The **committed/running** `ExecuteStep` already threads
`req.user_id` (proto field 8) into `execute_step`, so viewer-scoped tools
(e.g. `knowledge_search`) filter to the caller's visible set, not the whole org
(empty `user_id` = legacy org-scoped fallback). The **WIP** goes further:
`forwarded_user_bearer` makes a verified user credential **mandatory** and
`knowledge_search` fails closed without it, forwarding the bearer to Data Plane
retrieval so the post-filter runs as the real user. Caveat: in the **running**
binary the bearer is *not* required and `knowledge_search` grounds on the
`user_id`/`org_id` fields without a verified credential — so per-user isolation
currently leans on Data Plane trusting those fields (tie-in to the Data Plane
Phase 2 finding that `AUTHCTX_ENFORCE` is not yet flipped). The WIP closes this,
but only in a build that simultaneously removes the surface.

## Tool → backend wiring (from compose env, `[source-only]`)

> **2026-07-13 traffic-provenance update.** The information tool formatter now preserves measured/estimated/synthetic/unavailable provider provenance and labels legacy bare numeric traffic metrics `unverified_legacy`; four targeted Rust tests pass. The Model runtime is not deployed, and legacy input support is a compatibility warning rather than evidence that a number is observed.

| Tool(s) | Backend | Env (compose default) |
|---|---|---|
| get_shipping_quotes / shipping_carriers / book_shipment | shipping-core (Ingestion) | `SHIPPING_CORE_URL` → host.docker.internal:3156 |
| list_/execute_provider_action | integration-corev2 (Ingestion) | `INTEGRATION_COREV2_URL` → :3026 (+`INTEGRATION_COREV2_INTERNAL_KEY`) |
| yr_weather / traffic / news / track_shipment / company_lookup | information-core (Application) | `INFORMATION_CORE_URL` → :3190 (+`INFORMATION_CORE_INTERNAL_KEY`) |
| knowledge_search | Data Plane v2 retrieval | `DATAPLANE_RETRIEVAL_URL` → :50062 (+`DATAPLANE_INTERNAL_KEY`) |
| list_social_accounts / publish_social_post | social-core (Application) | `SOCIAL_CORE_URL` → :3162 |
| web_search / web_fetch / browser_agent | Quarry-v2 edge (Ingestion) | `QUARRY_EDGE_URL` → :8082, `QUARRY_BROWSER_AGENT_ENABLED=1` |
| mcp__* | model-gateway MCP registry (proxy) | `MODEL_GATEWAY_ADDR` → model-gateway:9090 |
| Infer round | inference-core | `INFERENCE_CORE_ADDR/URL` → :9092 |
| runs/approvals/checkpoints | session-core | `SESSION_CORE_ADDR` → :9091 |

Cross-plane targets use `host.docker.internal` published ports because exec-core
sits only on `model-plane-network` (the documented fix for the earlier
"dns error"/NXDOMAIN reaching integration-corev2). Live: shipping-core :3156
reachable (200); integration-corev2 :3026 `/healthz` → 404 (likely a different
health path; not probed further). `[live-curl]`

## Stubs / mocks / placeholders — verdicts

`grep -rniE "todo|fixme|unimplemented|mock|stub|fake|placeholder"` over `src`:

- **All `mock`/`unimplemented` hits are `#[cfg(test)]`** in-process tonic mock
  servers (`agent.rs`, `browser_events.rs`, `quarry_agent.rs` wiremock) — honest
  test doubles, not production stubs.
- `tool_bridge` "deterministic stub" (`executor.rs` doc + `mod.rs` const doc):
  **honest, by-design fallback**, not a hidden fake — real tools have dedicated
  arms; the bridge only echoes for unknown/reasoning-step tool names.
- `shipping_tools.rs` "mock vs credentialed real adapters": refers to
  shipping-core's own honest `is_mock` demo-vs-live carrier flag, surfaced to the
  model verbatim.
- `knowledge_tools.rs:195` "renders_placeholder": a **test name** (missing
  document id → `(document ?)`), not a stub.

No genuine production stub found in the tool-execution path. `[source-only]`

## Uncommitted WIP (git, `[source-only]`)

`git status` for the service shows 9 modified files (no untracked):
`Dockerfile`, `src/main.rs`, `src/lib.rs`, `src/grpc.rs`, `src/knowledge_tools.rs`,
`src/runtime_loop/mod.rs`, `src/runtime_loop/agent.rs`,
`tests/runtime_loop_test.rs`, `tests/slo.rs` (+251/−97).

The WIP is one coherent "secure-MVP" change:
1. **Removes the gRPC surface** — `main.rs` no longer starts `grpc::serve`
   (spawns `pending()`); `lib.rs` moves `grpc` to `#[cfg(test)] mod`; `grpc.rs`
   deletes `serve()` and drops the `ExecutionCoreServer` import. Adds a
   `grpc_containment_tests` asserting no env var can re-enable it.
2. **Hardens the (now test-only) handlers** — `forwarded_user_bearer` requires a
   verified user bearer on `ExecuteStep`/`RunAgent`; both fail closed with
   `failed_precondition` when `zdr=true` (ZDR tool exec / agent runs disabled
   "until the lifecycle is persistence-free"); `knowledge_search` requires and
   forwards the bearer to Data Plane with `zdr_mode`.

Assessment: **coherent and intentional, not abandoned/broken WIP.** It reflects a
real security decision (don't expose exec-core's gRPC until Control issues a
dedicated signed audience). The risk is the timing split — shipping the WIP
takes `RunAgent`/`ExecuteStep` offline for any caller that depends on them
(notably orchestrator-core, which dials `execution-core:9093`, and any durable
agent-run dispatch), while the hardening it introduces never runs in production
because it is `#[cfg(test)]`. Recommend either (a) keep the surface and land only
the handler hardening, or (b) confirm every real caller has migrated off
exec-core gRPC before removing it. Toolchain verified on host: cargo 1.94.1 /
rustc 1.94.1, `cargo check -p execution-core --all-targets` → clean in 1m48s.

## Doc-register reconciliation

Confirms the `apps/STALE_DOC_DELETION_REGISTER.md` direction that Model docs
(`ARCHITECTURE.md`, `gap-model.md`, `STUBS.md`) **overstate stub status**: for
execution-core the runtime loop, all 15 tools, MCP proxy, sandboxed shell,
browser-agent loop, durable approvals, and per-user scoping are real and (in the
running binary) live. The one caveat those docs should gain is the deployment
split above — "real but the surface is being removed in the secure-MVP WIP" — not
"stub."
