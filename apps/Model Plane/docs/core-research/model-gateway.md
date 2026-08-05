# model-gateway Research Dive

Generated: 2026-07-11 (supersedes the 2026-06-09 dive)

Scope: `apps/Model Plane/rust/services/model-gateway`

## 2026-07-16 delta

At 2026-07-16 20:05 CEST, the local integration container is running healthy
with restart count zero. HTTP `/readyz` is 200 and the additive gRPC listener is
loopback-reachable on `:9090`; a descriptor-specific unauthenticated call is
rejected before business handling. The image is a dirty-tree, unsigned
`working-tree` build, not an immutable production candidate. Approval cache misses read
through to the durable Session record and re-bind owner/org before use. A grant
can enter the durable claim/lease/retry/terminal state machine, but cannot
continue execution without a restartable descriptor, authenticated dispatcher,
and successful execution receipt, so the gateway must propagate non-success
rather than claim resume. Public free-form `SendMessage` is quarantined and
fails closed; the gateway NATS ACL no longer grants `agents.>`, `org.>`, or
`notify.>` and retains only normal `mp.v1` event authority. The old free-form
bridge is unavailable, not capability-dispatched. Browser status mapping now
allows completion only for explicit success; denial, timeout, and resource
exhaustion terminalize failed while cancellation/abort invoke `CancelRun`.
Unknown/approval-paused outcomes remain nonterminal. At 15:10 CEST, source
tests passed 409 library plus 41 integration cases. Incremental validation now
passes 420 gateway library tests, including managed-start-key, service-token
heartbeat, and initial-heartbeat fail-closed regressions. The gateway derives
an opaque keyed-MAC managed start key instead of persisting raw caller
idempotency material, and its idempotency cache has fixed key, entry, value,
and TTL bounds. The P0 Session Core managed-run terminalization
receipt/reconciler now exists in source and is used by Gateway, but it has no
release-database migration, live service-token, periodic-ticker/browser
background-dispatch, or rollback evidence. Browser/session/SSE coverage remains
below the target. Live no-auth, malformed-bearer, and forged-identity gateway
probes deny, but a full valid chat/tool/browser E2E and immutable artifact do
not exist.

Method note: audited under a corrupted Docker containerd content store — no
`docker exec` / `build` / `logs`. Live facts come from host curl to published
ports and `docker ps` / `docker inspect`; everything else is read from source,
compose, and env on disk. Every finding is graded **[live-curl]**,
**[source-only]**, or **[inspect]**.

## 2026-07-13 compatibility/security correction

The historical bottom line below is retained as audit history, but its gRPC-WIP
description is superseded. Current source restores the additive `:9090`
ModelGateway contract and public standard health. Business RPCs eagerly verify
Auth Core RS256 material, exact `aud=model-gateway`, lifetime, canonical
user/service identity, tenant/user ownership, and exact service scopes. Session
and inference credentials are separate delegated bearers with target-audience
verification and identity binding; issuer ZDR is monotonic. Legacy gRPC MCP
registration and remote trigger remain quarantined rather than reopening RCE,
ownership, or DNS-rebinding gaps.

Earlier source-test counts below are historical and must not be reused for the
current uncommitted aggregate. The HTTP JWKS loader is startup-warmed,
redirect-denying, status-checked, time-bounded, and body-bounded. No `:9090` live claim is made:
the running listener still refuses connections. Approval/browser paths now
require separate exact execution/session credentials and affirmative execution
resume; durable decision persistence precedes in-memory mutation. An already-
granted retry whose execution delivery is unknown returns explicit gRPC
`Unavailable` / HTTP 503 and requests no second resume instead of reporting
false success. Session Core emits approval/resume events only for the durable
CAS winner. Execution Core uses an atomic gated-state transition, so a terminal/
running/unknown run cannot be reactivated by replay. Transactional outboxes for
the Session CAS/event and gateway decision/resume boundaries, plus user-scoped
durable read-through after cache eviction, are still required for crash recovery.
Safe cutover also requires a verified ZDR policy,
capability execution authority, migrations, immutable images, and live evidence.

## Bottom line

`model-gateway` is a **real, non-mocked public boundary**, materially stronger
than the June dive described. It is THE Verevon chat entry point: `/v1/invoke`
(unary) and `/v1/invoke/stream` (SSE) forward to inference-core over gRPC for
completion and run an in-gateway function-calling loop for chat tools; durable
agent/tool runs are driven by execution-core. Auth is a hardened JWKS/RS256
model with canonical service identity and monotonic ZDR. The important debt is
now (1) an **uncommitted WIP that removes the shipped gRPC boundary** while the
running 2-day-old container still serves it, and (2) **config duplication /
provider-key operational noise**, not missing functionality.

## Phase-4 headline questions (answered)

1. **Is the gateway → execution-core agent/tool loop real and non-mocked?**
   YES [source-only]. Two distinct real loops exist:
   - *In-gateway chat loop* (`src/tool_loop.rs` + `src/sse.rs`): unary
     `infer(messages+tools)` against inference-core → execute requested tools →
     inject results as context → re-infer, capped at `MAX_TOOL_ROUNDS = 3`.
   - *Durable agent loop* in execution-core (`runtime_loop/{mod,agent}.rs`),
     reached for durable runs; the gateway resumes it after approvals.
   No stub/mocked inference on the shipped path (the only `Mock*` symbols are in
   `grpc.rs`'s `#[cfg(test)]` module).

2. **Are tools actually registered and dispatched (esp. shipping → :3156)?**
   YES, but note the split [source-only]:
   - The **gateway chat loop** advertises + dispatches `web_search`,
     `fetch_url`, `knowledge_search` (built-ins), plus `brreg_lookup_organization`
     (live Brønnøysund HTTP), `recall_memory`/`save_memory` (session-core memory),
     `browser_agent` (gated on `BROWSER_AGENT_URL`), and any registered MCP tool
     via the `mcp__<server>__<tool>` prefix. It has **no shipping tool.**
   - **Shipping tools live in execution-core**, not the gateway:
     `execution-core/src/shipping_tools.rs` calls `SHIPPING_CORE_URL`
     (default `http://host.docker.internal:3156`) `/api/quotes|/api/carriers|/api/bookings`.
     `get_shipping_quotes`/`shipping_carriers` are read-only; `book_shipment` is
     in `permission::is_risky_tool` and pauses for HITL. So a shipping-time
     question is answerable only through the **durable agent path** (execution-core),
     not the plain chat SSE loop — which is consistent with the Phase-3 finding
     that the real failure was shipping-core's Bring delivery-time defect, not
     the Model Plane loop.

3. **THE MCP question — is mcp-bridge real, and is there ANY Visma MCP wiring?**
   MCP is real; **Visma is not wired anywhere in the Model Plane.** [source-only]
   - `src/mcp_jsonrpc.rs` is a complete, unit-tested JSON-RPC 2.0 / MCP
     `2024-11-05` client (initialize → initialized → tools/list → tools/call),
     with `stdio://` command parsing. `src/runtime_registries.rs` implements the
     `McpRegistry` with **stdio** (spawns a child, speaks JSON-RPC over
     stdin/stdout, 30s bound) and **http** transports, tool discovery with
     caching (`mcp_tool_defs`), the `handle_proxy_mcp_tool` proxy, and a
     write-through of registered servers to capability-core. `POST /v1/mcp/servers`
     registers a server; its tools are then auto-advertised to the model.
   - **`grep -rni visma` across the entire Model Plane returns ZERO matches.**
     No Visma MCP server is registered, seeded, or defaulted. "Test the Visma
     MCP" is therefore **not a Model Plane / verevon-chat capability today** —
     the Visma Net MCP that exists is a claude.ai / Claude-Code connector
     (`visma_net_mcp`, and the `visma-salgsordre-test` skill), i.e. assistant-side
     tooling, entirely separate from the runtime the chat UI talks to. A user
     *could* register a Visma MCP server through the gateway's registry API, but
     nothing does so now.

4. **Is HITL actually enforced, or decorative?**
   ENFORCED pre-execution [source-only] — the prior "decorative" finding does
   not hold for the current execution-core agent path:
   - `runtime_loop/mod.rs`: `PermissionDecision::AwaitApproval => return
     StepOutcome::awaiting_approval()` fires **before** the tool match arm, so a
     risky tool (`book_shipment`, provider writes, social posts) never executes
     on the gated turn.
   - `runtime_loop/agent.rs`: on `awaiting_approval` the loop mints a **durable**
     approval via session-core `CreateApproval`, sets `RunStatus::AwaitingApproval`,
     and returns early; resume re-invokes `run_agent`.
   - `execution-core/src/browser_events.rs::require_approval` is documented and
     coded to **fail closed on every error path**.
   - The gateway is the **decision point**: `POST /v1/orchestration/approvals/:id/decide`
     persists the decision to session-core (org-scoped from verified claims, the
     Phase-6 IDOR fix) and, on grant, calls execution-core `resume_run`.

## Live health [live-curl] (host curl to :8080, 2026-07-11)

- `GET /healthz` → `200 ok`
- `GET /readyz`  → `200 ok`
- `GET /health`  → `401` (auth-gated; confirms JWT middleware is active and
  rejects a missing bearer — dev-bypass is off, see env below)
- `GET /metrics` → `200` Prometheus; notable series:
  `mp_gateway_finetune_poller_ticks_total{status="ok"} 2284`,
  `{status="error"} 983` — the fine-tuning poller (Azure configured) errors on
  ~30% of ticks. Low severity, but worth investigating (Azure job-list errors
  or transient auth); it does not affect chat.
- A **real invoke could not be exercised**: dev-bypass is `0` and no valid
  auth-core JWT was available, so `/v1/invoke` correctly refuses. The `401` on
  `/health` is the positive evidence that auth is enforced.

`docker ps` [inspect]: `model-plane-model-gateway-1` Up 2 days, publishes
`8080->8080` **and `9090->9090`**, shows `(unhealthy)` — but that is the
exec-based healthcheck failing under the corrupted content store, not the
service (host `/healthz` is 200).

## gRPC :9090 — shipped-vs-running discrepancy (important)

- A raw HTTP/2 preface to `127.0.0.1:9090` returns a valid SETTINGS frame
  [live-curl], so the **running** container **does** serve gRPC on 9090.
- But `src/main.rs` on disk (WIP) **disables** it: it replaces
  `tokio::spawn(grpc::serve(...))` with `tokio::spawn(std::future::pending())`
  and logs "Model Gateway gRPC is unavailable in the secure MVP". `src/lib.rs`
  now declares `grpc` as `#[cfg(test)] #[allow(dead_code)] mod grpc;`
  [source-only]. A new untracked test (`tests/grpc_secure_mvp_containment_test.rs`)
  and a `grpc_containment_tests` unit test assert no env combination can expose
  the unverified gRPC boundary.
- Interpretation: the running binary predates this uncommitted WIP (the classic
  "Docker rebuild serves the previous binary" gotcha). **On the next rebuild the
  gRPC :9090 boundary disappears.** Any internal caller still dialing the
  gateway's gRPC must migrate to HTTP, or this is a breaking change. The
  Dockerfile WIP adds OCI `revision`/`created` labels — a good mitigation for
  exactly this stale-binary confusion.

## Invoke path (verified real) [source-only]

`POST /v1/invoke` (`http_routes.rs::invoke`):
1. `normalize::normalize` + validate.
2. **ZDR branch first**: if `zdr`, `invoke_persistence_plan` →
   `SuppressAllForZdr`; the handler branches *before* the idempotency registry,
   budget/session clients, and event publisher, forwarding to inference-core
   with `zdr: true` and returning — so no request/response content becomes
   durable. Optional PII redaction (`moderation::redact_pii`) applies when the
   `pii_filter` feature is set.
3. Non-ZDR durable path: idempotency claim (dedup/regenerate, 409 on in-flight)
   → `budget::check_budget` (cost-core) → `session_flow::prepare_run`
   (session-core) → publish `INGRESS_ACCEPTED` → `inference_client.infer`
   (inference-core gRPC) → `append_assistant_message` (session-core) → publish
   `USAGE_ENVELOPE` → cache the result under the idempotency key.

`POST /v1/invoke/stream` (`sse.rs`) is the SSE tool-loop entry; `/v1/invoke/resume/:id`
replays a buffered stream (process-local `StreamBufferStore`, Redis-backed when
`REDIS_URL` is set); `/v1/invoke/:id/cancel` cooperatively stops an in-flight
stream via `CancelRegistry`.

Note: the **unary** `/v1/invoke` is single-shot inference (no tool loop). Chat
tool-calling runs on the **streaming** path.

## Auth model (hardened) [source-only]

`src/auth.rs`:
- JWKS fetched from `AUTH_CORE_JWKS_URL`, TTL-cached, two-pass `kid` lookup for
  rotation; **algorithm pinned to RS256** (blocks `none`/HS256 confusion).
- Enforces `exp`, `nbf` (configurable leeway), `iss` (`AUTH_CORE_ISSUER`), and
  `aud` (`AUTH_CORE_AUDIENCE`, default `model-gateway`). **Fail-closed**: missing
  issuer/audience env → `500`; the legacy `AUTH_CORE_AUDIENCE_OPTIONAL` opt-out
  was removed and no longer weakens validation (regression-tested).
- **Canonical service identity**: `principal_type=service` requires
  `service_id == sub` with a `service:` prefix, empty `user_id`, and a trimmed
  3–500 char `reason`; user principals require `sub == user_id` and no
  `service_id`. `authorize_principal_route` restricts service principals to
  `POST /v1/ai/chat` and `/v1/ai/embeddings` **and** requires the
  `models:invoke` scope. This is exactly the remediation Data Plane Phase 2
  called for.
- **Monotonic ZDR**: `Claims.zdr` is issuer-asserted; `effective_zdr(req_zdr) =
  self.zdr || req_zdr` — a request can only make ZDR *stricter*, never turn a
  signed `zdr:true` off.
- **Delegated Data Plane bearer**: a separate `aud=data-plane` user token in
  `x-data-plane-authorization` is independently verified against the same JWKS
  and must match the model token's `sub`/`user_id`/`org_id`; the gateway
  forwards this verified bearer to Data Plane instead of forging scoping headers.
  Service principals may not delegate one; dev-bypass may not supply one
  (both tested).
- **Dev bypass** requires BOTH `MODEL_GATEWAY_AUTH_DEV_BYPASS` and
  `ALLOW_INSECURE_DEV_DEFAULTS` (else `validate_startup` aborts boot). Both are
  `0`/unset in the audited env, so the running gateway enforces real auth.

## ZDR propagation into grounding/knowledge (closed gap) [source-only]

The Phase-2 gap ("gateway did not propagate the chat ZDR flag into
grounding/knowledge") is **closed**:
- `state.rs::DynPublisher::publish` drops any `zdr:true` envelope before any
  NATS/in-memory backend (unit-tested).
- `retrieval.rs::data_plane_zdr_mode(zdr)` maps `true → Some("ephemeral")`, and
  is threaded into `RetrieveRequest.zdr_mode` and graph/knowledge grounding
  requests, which are `authorize`d with the verified delegated bearer.
- `tool_loop.rs::knowledge_search` builds an org-scoped, ZDR-aware
  `RetrieveRequest` and refuses without a verified bearer.

## Downstream wiring [source-only + inspect]

`state.rs::from_env` lazily builds ~18 gRPC/HTTP clients (env-keyed, `connect_lazy`):
inference-core `:9092`, session-core/run/finetune/memory `:9091`,
orchestration `:9080`, execution-core `:9093`, sandbox `:9094`, browser-broker
`:9095`, capability-core gRPC `:9097` / HTTP `:8085`, Data Plane
retrieval+document+knowledge `:50052`, graph `:50053`, wiki `:50054`,
cost-core HTTP (`COST_CORE_URL`), Quarry edge (`QUARRY_EDGE_URL`), LSP bridge
(`LSP_BRIDGE_URL`). Absent config degrades honestly (`Unimplemented`, null cost,
disabled tool) — never a fake.

Compose [inspect] (`deploy/docker-compose.yml`) sets **explicit Model Plane
aliases** (`SESSION_CORE_ADDR: http://model-plane-session-core-1:9091`, etc.),
deliberately avoiding the ambiguous short names on the shared `inter-plane-bus`
(where `session-core` could resolve to a Control Plane service). The gateway
joins both `default` and `inter-plane-bus` (aliases `model-gateway`,
`model-plane-model-gateway-1`). Auth env: `MODEL_GATEWAY_AUTH_DEV_BYPASS=0`,
`AUTH_CORE_JWKS_URL=http://auth-core:3011/api/convex-auth/jwks`,
`AUTH_CORE_ISSUER=http://localhost:3011/api/convex-auth`, audience
`model-gateway`. Azure OpenAI + OpenAI keys present (explains the active
fine-tune poller); LangCache configured.

## Stubs / placeholders — judged [source-only]

The 2026-06 dive over-stated stub status; the current tree is largely honest:
- `tools.rs` / `grpc.rs` / `state.rs`: `Status::unimplemented("quarry edge not
  configured")` etc. are **honest not-implemented guards** keyed on missing
  config (`QUARRY_EDGE_URL`, `LSP_BRIDGE_URL`, `COST_CORE_URL`), not stubs.
- `auth.rs` `user_placeholder`/`org_placeholder`: only reachable on the
  double-gated dev bypass (off in prod).
- `grpc.rs` `Mock*` types: `#[cfg(test)]` only.
- `confidence.rs`/`pricing.rs`/`state.rs` comments emphasise "**never a fake**"
  (null cost / null confidence when the source is unreachable).
- Genuinely thin: `/v1/ai/realtime` is still session-scaffold level, and
  `metrics_placeholder` is only used when the Prometheus recorder failed to
  install. `browser_agent` is inert unless `BROWSER_AGENT_URL` is set.

Recommendation for `apps/STALE_DOC_DELETION_REGISTER.md`: **confirm the
"update, not delete" verdict** for `docs/STUBS.md`, `docs/gap-model.md`,
`docs/ARCHITECTURE.md` — they overstate stub status now that inference,
tool-calling, MCP, ZDR, and HITL are live; update rather than delete.

## Uncommitted WIP [inspect]

`git status` shows 15 modified files + 1 untracked test under the service,
`git diff --stat` ≈ **+2,243 / −448** lines. Largest: `auth.rs` (+657, the
service-identity + delegated-bearer + fail-closed hardening), `dataplane.rs`
(+590, retrieval/grounding + ZDR), `sse.rs` (+233), `grpc.rs` (±210, becoming
test-only), `http_routes.rs` (+199), `tool_loop.rs` (+81). Net: a coherent
security-MVP hardening pass. **Not yet rebuilt into the running container.**
This should be committed and rebuilt (with `--no-cache` per the known
stale-binary gotcha) so the running boundary matches the audited source — in
particular the gRPC removal and the auth hardening.

## Risks / follow-ups

- **[medium]** WIP removes the gRPC :9090 boundary the live container still
  serves; migrate/verify internal gRPC callers before rebuild, or the rebuild is
  a silent breaking change.
- **[low]** Fine-tune poller error rate ~30% of ticks (`/metrics`); investigate
  or quiet if Azure isn't the intended fine-tune backend for this profile.
- **[low]** `SESSION_CORE_URL` (short alias) appears in `.env` while the compose
  `environment:` uses the explicit `SESSION_CORE_ADDR` Model Plane alias; since
  `state.rs::read_endpoint` prefers `SESSION_CORE_URL`, forwarding that var into
  the container (e.g. via a future `env_file:`) would reintroduce the ambiguous
  short-name resolution the explicit alias was added to prevent. Keep them
  consistent.
- **[info]** "Test the Visma MCP" is not a Model Plane capability; if it is a
  product requirement, it needs a Visma MCP server registered via
  `POST /v1/mcp/servers` (stdio or http), which nothing currently does.
