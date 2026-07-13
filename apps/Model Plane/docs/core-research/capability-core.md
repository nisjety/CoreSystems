# capability-core Research Dive

Generated: 2026-07-11 (supersedes 2026-06-09)

Scope: `apps/Model Plane/go/services/capability-core`

## 2026-07-13 secure-MVP correction

The running image was not rebuilt, so the live unauthenticated and misleading-readiness evidence below remains an **open production finding**. Source has since been hardened:

- startup eagerly validates Auth Core configuration and fails closed; only `/healthz` and `/readyz` are public;
- every product HTTP API and unary gRPC RPC requires an RS256 token with issuer and `aud=capability-core`;
- organization and actor identity come from verified claims, with tenant predicates across capability, skill, MCP, routing, safety, memory, tasks, cron, model, audit, and scope operations;
- policy evaluation is service-only; global skill promotion additionally requires `capability:global:write`;
- unsafe tenant-agnostic command execution is quarantined;
- additive capability availability exposes fail-closed state/reason,
  approval/execution/cost semantics, server-time freshness, version CAS, and
  atomic audit;
- MCP registry writes use bounded strict transport/config schemas, remote
  public HTTPS only, exact nonempty tool allowlists, managed secret references,
  tenant-scoped mutation, and redacted/quarantined reads;
- production builds use Go 1.25.12 and updated `x/net` dependencies.

Verification: full tests, vet, focused race, availability coverage, and the MCP
boundary suite pass. Authz measured **91.7%**; availability wire/handler/
derivation/store-CAS functions measured **80–100%**; the strict MCP boundary
measured **84.49%**. The final full-module profile measured **55.4%**; policy
measured **95.4%**, models **93.6%**, and server **85.2%**. Registry remains
**47.8%**, so aggregate coverage is still an explicit release gap even though
the changed authorization/business-critical functions measured **80–100%**.

Policy authority is deliberately narrow for the MVP: `EvaluatePolicy` accepts
only `global` and the verified tenant's `org` scope. `agent`, `run`, `thread`,
`workspace`, `user`, empty, and unknown scopes fail before registry or grant
lookup because the request has no trustworthy server-derived resource/agent
identity. Exact or wildcard legacy agent grants cannot reactivate those
unsupported scopes. Migration `0007` tenant-binds grants, quarantines invalid
risk/scope data, and normalizes revoked malformed rows before adding
constraints; its fast regression and integration-gated PostgreSQL fixture pass
or compile/skip, but it has not run against an isolated release-shaped
Postgres. Private memory is actor-pinned and ownerless/resource-ambiguous rows
fail closed.

**Deployment state:** PARTIAL IN SOURCE ONLY. Auth Core, Model Gateway, and
frontend source mint/forward a separate `aud=capability-core` token for migrated
proxy paths. Availability is enforced by Capability Core policy but is not yet
an unavoidable Model Gateway/model-offer/Execution Core dispatch authority; a
scoped global health reporter and complete release-database migration proof are
absent. Registry DNS validation must be repeated and pinned by the execution
client. JWKS refresh is startup-only. Do not deploy until authority consumers,
callers, migrations, and live matrices pass.

Audit constraint this pass: Docker containerd content store is corrupted — `docker exec`/`build`/`logs` fail fleet-wide. Findings are graded **[live-curl]** (host curl to published ports), **[source-only]** (read from disk), or **[inspect]** (docker ps/inspect config/state). No exec/build/logs were attempted.

---

## Snapshot

`capability-core` is a **real, durable** Go service — a hybrid **registry + policy authority + workplane CRUD** service. It is NOT the chat tool-dispatch path, and its policy/HITL engine is currently unconsumed. The 2026-06-09 doc's "hybrid, not fully converged" read is directionally correct but (a) undersells how durable the registry layer is and (b) misses the four material findings below.

- HTTP `:8085` (health + all product/workplane REST APIs); gRPC `:9097` (CapabilityCore service).
- Postgres-backed registry (static Go seed ∪ `capabilities` table) + durable stores (capabilities, scope grants, models) + NATS reconcile events + a G7 learning-review consumer.
- Migrations self-applied at container start (`docker-entrypoint.sh` runs `psql` over `migrations/*.up.sql`).
- `go build` + `go vet ./services/capability-core/...` both **clean** on host (go 1.26.2) **[source-only]**.
- **No uncommitted WIP** in the service dir (`git status --porcelain` empty) **[source-only]**.

---

## Live health & runtime state **[live-curl]**

| Probe | Result |
|---|---|
| `GET /healthz` | **200 ok** |
| `GET /readyz` | **200 ok** |
| `GET /api/v1/model-plane/implementation-status` | 200 (static roadmap JSON) |
| `GET /api/v1/commands` | 200 (static catalog: /compact,/resume,/memory,/tasks,…) |
| `POST /api/v1/commands/exec {/models}` | 200 `{"success":false,"output":"model registry unavailable (inference-core not wired)"}` |
| `GET /api/v1/mcp?org_id=global` | **500** DB dial error |
| `GET /api/v1/capabilities` | **500** DB dial error |
| `GET /api/v1/skills?org_id=global` | **500** DB dial error |

**Every durable (DB-backed) endpoint 500s right now** with:
`failed to connect to user=postgres database=session_core: 172.21.0.12:5432 no route to host`.

Two things this proves:
1. **The Postgres host is unreachable from capability-core this pass** — the durable registry/MCP/skills/workplane surfaces are non-functional until the DB is back. This is an environment/runtime condition, not a source defect.
2. **`/healthz` and `/readyz` do not probe the DB** (both are trivial `200 ok` writers in `cmd/main.go`). The service reports healthy/ready while its entire durable surface is down — a misleading readiness signal.

Note: `DATABASE_URL=postgresql://postgres:postgres@postgres:5432/session_core` (compose) — capability-core **shares session-core's `session_core` database**. This is deliberate: its `/api/v1/skills` handler reads/writes `agent_skills`, and the tasks/cron/memory handlers use `tasks`/`cron_schedules`/`agent_memory` — tables session-core owns and migrates. capability-core migrates only the registry tables (see below).

---

## Runtime shape **[source-only]**

`cmd/main.go` wires, all nil-safe/guarded:
- pgx pool → registry (`NewCapabilitiesSource` Postgres ∪ static seed; falls back to static `NewRegistry()` if load fails), models registry, capabilities store, scope-grant store.
- policy engine (`policy.New(reg)` + optional `WithScopeResolver(scopeStore)`).
- HTTP `:8085`: `/healthz`, `/readyz`, roadmap, capabilities, skills, mcp, routing, safety, memory, tasks, cron, commands.
- gRPC `:9097`: `CapabilityCore` (ListCapabilities, GetCapability, EvaluatePolicy, ValidateSkillBundle, CheckSkillPromotion, PromoteSkill) with durable store attached for score-ranked listing.
- NATS (when `NATS_URL` set): reconcile publisher (`mp.v1.capability.<kind>.<action>`) + G7 learning-review consumer.

Migrations (`docker-entrypoint.sh` → psql): `0001/0002` models + seed; `0003` full registry (capabilities, capability_versions, capability_scopes, capability_health, skill_packages, skill_resources, **mcp_servers**, mcp_oauth_tokens, plugin_packages, routing_policies, safety_policies, registry_audit_log); `0004` seeds 13 self-owned systems (Tika, Tesseract, Gotenberg, OpenSanctions, Nominatim, disposable-email, …) as `connector` capabilities; `0005` seeds `operating_map.generate`.

---

## Who actually consumes capability-core (verified) **[source-only]**

Real consumers:
- **orchestrator-core (Go)** — gRPC `ValidateSkillBundle` / `CheckSkillPromotion` / `PromoteSkill` in Temporal activities (`services/orchestrator-core/cmd/activities/activities.go:356-382`). Skill-promotion workflow is genuinely wired.
- **model-gateway (Rust)** — HTTP write-through `POST /api/v1/mcp` on MCP-server register (`services/model-gateway/src/grpc.rs:976-984`, guarded on `capability_core_base_url`), plus the NATS read-path dual `capability_consumer.rs` subscribing to `mp.v1.capability.mcp_server.*` to keep the gateway's MCP cache coherent. capability-core is the **durable system-of-record for MCP servers**.

Unconsumed (implemented, no caller anywhere in Go or Rust, generated clients aside):
- **`EvaluatePolicy`** — grep across `rust/` (non-target) and `go/services` (non-gen) finds no `evaluate_policy`/`EvaluatePolicy` caller. capability-core's risk/HITL policy engine is **not in any enforcement path**.
- **`ListCapabilities` / `GetCapability`** — no runtime consumer resolves tools from the registry.

Confirming the tool-loop boundary: `services/execution-core/src/mcp_gateway.rs:3` states *"execution-core has no MCP registry of its own"*; execution-core dispatches its tools from **compiled-in Rust modules** (shipping_tools, info_tools, integration_tools, quarry, retrieval) using direct env URLs (`SHIPPING_CORE_URL`, `INFORMATION_CORE_URL`, `INTEGRATION_COREV2_URL`, `QUARRY_EDGE_URL`, `DATAPLANE_RETRIEVAL_URL` — all in the exec-core compose block). **The capability registry does not drive chat tool dispatch.**

---

## Phase-4 headline questions

**Q1 — Is the model-gateway → execution-core tool loop real/non-mocked?**
Out of scope for this service, but capability-core is confirmed **not** part of that path. The loop is real in the Rust services; capability-core neither registers nor resolves the executable tool set. **[source-only]**

**Q2 — Are tools registered & dispatched (shipping → :3156)?**
Not through capability-core. Its registry is a descriptive catalog (cap.browser.open, cap.sandbox.exec, cap.memory.*, cap.retrieval.query, cap.tool.http, cap.inference.*, cap.skill.summarize, cap.plugin.code-exec, cap.mcp.filesystem, cap.policy.round-robin, cap.safety.pii-filter, cap.command.shell + 13 self-owned + operating_map). Shipping-tool reachability (`SHIPPING_CORE_URL=host.docker.internal:3156`) is an execution-core concern, unrelated to this registry. **[source-only]**

**Q3 — THE MCP / Visma question.**
capability-core IS the durable MCP registry (`mcp_servers` table + `/api/v1/mcp` CRUD + gateway write-through + reconcile read-path). Per the existing plane audits (`docs/core-research/plane-audit-2026-07-02.md:14`, `README.md:14`), the live MCP registry contains **exactly one** org record named **`visma mcp`**, configured `transport=stdio` with an **HTTPS `endpoint_url`** and **no allowlist**. That is self-contradictory — a stdio MCP transport does not take an HTTP URL — so discovery fails and **no Visma MCP tool has ever executed**. Grep for `visma` across the entire Model Plane returns **only those two audit docs** — there is **no Visma-specific code, client, or connector anywhere**. So the user's "test the Visma MCP": it is **not a working Model Plane capability** — just a single broken/misconfigured registry row, never an integrated connector. I could not re-confirm the row live this pass because the DB is unreachable and `/api/v1/mcp` 500s. **[source-only + prior-doc]**

**Q4 — Is HITL enforced or decorative (at this layer)?**
capability-core's policy engine (`internal/policy/engine.go`) is a **real, correct heuristic**: high-risk → deny with reason *"requires human approval"*; medium → allow under a constrained token/cost budget; low → allow under a generous budget; plus real static-scope denial (`denyOnScope`), durable grant-table denial (`denyOnGrant` via `capability_scopes`), and RBAC `Enforce`. **But `EvaluatePolicy` has no runtime consumer**, so this authority is **not exercised** — the capability-core layer's HITL is effectively **decorative / dead in practice**. Any real risky-tool gating (e.g. `book_shipment`) happens in execution-core/model-gateway, not here. This corroborates the prior "HITL decorative on live writes" concern at the capability-core boundary. **[source-only]**

---

## Findings

| # | Sev | Grade | Finding |
|---|---|---|---|
| 1 | HIGH | live-curl | DB (`session_core`) unreachable → **all durable endpoints 500** (mcp/capabilities/skills/tasks/cron/memory). Runtime/env condition. `/healthz` + `/readyz` don't probe the DB, so the service falsely reports healthy — no readiness signal for the outage. |
| 2 | MED | live-curl + source | capability-core's compose env omits `SESSION_CORE_ADDR` / `INFERENCE_CORE_ADDR` → `dialBackends()` returns nil → (a) **G7 learning-review consumer never starts**, (b) `/models` & `/compact` command delegation return "unavailable". Confirmed live: `/models` exec → *"model registry unavailable (inference-core not wired)"*. |
| 3 | MED | source-only | `orchestrator-core/internal/config/config.go:32` default `CAPABILITY_CORE_ADDR="capability-core:9092"` is the **wrong port** (9092 = inference-core; capability-core gRPC = **9097**). Compose overrides it to `:9097`, so it works in-stack — but the code default is a latent misconfiguration for any deploy relying on defaults. |
| 4 | LOW | source-only | `EvaluatePolicy` / `ListCapabilities` / `GetCapability` gRPC methods are implemented but have **no consumer** — capability-core's risk/HITL/policy authority is dead code in the current topology. |
| 5 | LOW | source-only | `internal/roadmap/data.go` implementation-status still reports model-gateway / session-core / inference-core as `partial` / "in progress" — **stale** vs the confirmed-live reality (these are live and serving chat). |
| 6 | LOW | source-only | `/api/v1/skills` (`agent_skills`) and tasks/cron/memory handlers (`tasks`/`cron_schedules`/`agent_memory`) read/write **session-core-owned tables** capability-core does not migrate — cross-authority table access inside the shared `session_core` DB (works, but couples the two services' schemas). |

---

## Stubs / placeholders assessment **[source-only]**

No genuine product stubs. Grep hits classify as:
- **Test fakes** — `llmreviewer`, `skillsink`, `sessionreview`, `commands` (honest unit-test doubles).
- **Honest not-wired guards** — `/models`,`/compact` return "unavailable" when the backend client is nil; `dispatch` default case returns *"has no server-side action (client-handled)"* rather than fabricating output.
- **Standard gRPC embed** — `mpv1.UnimplementedCapabilityCoreServer`.
- **Aspirational comment, real code** — `policy/engine.go` header says *"Replace with an OPA-backed … implementation once the control plane exposes policy bundles"*, but the heuristic + scope + grant + RBAC logic is fully implemented.

The registry is **real and populated**: static Go seed (24 capabilities) ∪ Postgres `capabilities` table (migration seeds 0004/0005) ∪ durable store with composite scoring, scope grants, and append-only audit log. The 2026-06-09 doc's "in-memory abstraction still exists" is accurate but the durable path is the primary one.

---

## Doc-cleanup read

- `docs/STUBS.md`, `docs/gap-model.md`, `docs/ARCHITECTURE.md` (flagged in `apps/STALE_DOC_DELETION_REGISTER.md`): **confirmed overstating stub status** for capability-core — its durable registry, MCP system-of-record role, and orchestrator-core skill-promotion consumer are all live. Update, don't delete.
- `internal/roadmap/data.go`: update the stale "partial/in progress" statuses (Finding #5).
- Keep `README.md`.

## Bottom line

capability-core is a **genuine durable service**, not a stub — but its real value in the running system is narrower than its surface: it is (1) the **durable MCP-server registry** (system-of-record for model-gateway, and where the single broken `visma mcp` row lives), and (2) the **skill validate/promote authority** for orchestrator-core. Its **capability catalog is not consulted for chat tool dispatch**, and its **policy/HITL engine has no consumer** (decorative at this layer). Two wiring gaps blunt it further in the current deployment: the shared DB is unreachable this pass (all durable endpoints 500, masked by no-op health checks), and the session/inference backend addrs are unset (learning consumer + /models·/compact dead). The Visma MCP the user tried to test is not an integrated capability — just one misconfigured stdio-vs-HTTPS registry record that has never executed.
