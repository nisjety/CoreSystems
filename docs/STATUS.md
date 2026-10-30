# CoreSystem Status

> **2026-07-20 correction:** this file's Model Plane claim of **"All 30 verification
> gates closed (100%)"** and **"no architectural drift"** is stale and superseded.
> Live 2026-07-16 through 2026-07-20 evidence (`apps/Model Plane/MODEL_PLANE_STATUS.md`,
> `docs/core-research/plane-audit-2026-07-16.md`,
> `docs/core-research/grpc-safe-rebuild-decision-2026-07-16.md`) shows the Model Plane
> is a working local integration stack (all 21 containers healthy, gRPC listeners
> authenticated and reachable as of 2026-07-20) but is **not production-ready**:
> approval-continuation has no restartable dispatcher/receipt, capability health has
> no attested reporter, tenant delegation (`allowAnyOrg`) is incomplete, and no signed
> release artifact/rollback exists. Do not treat the "30/30 gates" figure below as
> current. Everything under "Quarry V2 — detailed" was not re-verified in this pass;
> treat it as historical unless cross-checked against `Ingestion Plane/INGESTION_PLANE_STATUS.md`.

**Last updated:** 2026-04-23

Single source of truth across both in-flight planes. Points back at per-app docs for detail.

---

## At a glance

| Plane | App | Phase status | Verification gates | Next step |
|---|---|---|---|---|
| Ingestion | `Ingestion Plane/Quarry-v2` | Phase 0 ✅ · Phase 1 🟡 (7/10) · Phase 2 🟡 · Phase 6 🟡 · Phases 3–5, 7–8 ⬜ | n/a — feature-based | Webhook dispatcher + `chromiumoxide` browser driver |
| Model | `Model Plane` | Phases 0 ✅ · PR-2..PR-8 all ✅ · Phase 1 (orchestration shell parity) 🟡 started | **30/30 (100%)** | Wire `mp-orchestration` into session-core + draft `orchestration.proto` |

---

## Model Plane — detailed

### What's done

**All 30 verification gates closed.**

| Gate domain | Done |
|---|---|
| Contract | 5/5 |
| Replay | 4/4 |
| Runtime | 4/4 |
| Workflow | 4/4 |
| Security | 6/6 |
| Performance | 4/4 |
| Migration | 4/4 |

**Crates / services shipped this round:**

- `rust/crates/mp-slo` — SLO harness (19 unit tests). Primitives: `first_token_latency`, `step_throughput`, `checkpoint_recovery_ms`, `context_assembly` + named SLO catalog + nearest-rank percentiles.
- `rust/crates/mp-orchestration` — Phase 1 durable record types (38 unit tests). Plan, Todo, Approval, SubagentLineage, OrchestrationEvent. Pure data crate, zero IO, state-machine invariants enforced.
- `go/services/orchestrator-core/internal/compat/` — self-loop guard (producer≡compat-adapter OR schema_version>0) + 4 E2E tests through real production wiring. Fixes dual-write infinite loop.
- `go/pkg/natsx/` — compat_matrix_test + mode_matrix_test + PR-6 mechanism tests.
- `rust/services/execution-core/tests/slo.rs` — step throughput + checkpoint recovery wired against real `runtime_loop::execute_step`.
- `rust/services/session-core/src/grpc.rs::tests::context_assembly_meets_slo` — context assembly latency + token-budget assertion wired.

**Docs reconciled:** `docs/gap-analysis.md`, `docs/VERIFICATION.md`.

### What's missing (Phase 1 — orchestration shell parity, in progress)

`mp-orchestration` crate lives but is not yet consumed. Remaining Phase 1 work:

| Item | Owner | Status |
|---|---|---|
| `orchestration.proto` draft (Plan/Todo/Approval/SubagentLineage messages + PlanService/TodoService/ApprovalService RPCs + StreamRunEvents) | proto | ⬜ |
| session-core: Postgres tables (`plans`, `plan_steps`, `todos`, `approvals`, `subagent_edges`) + migrations | session-core | ⬜ |
| session-core: gRPC impl of the above services (using `mp-orchestration` types) | session-core | ⬜ |
| execution-core: call `Approval.grant/deny` from runtime loop when hitting approval gates | execution-core | ⬜ |
| orchestrator-core: subscribe to `OrchestrationEvent` via NATS for recovery + coordination | orchestrator-core | ⬜ |
| SSE stream endpoint for run events (model-gateway) | model-gateway | ⬜ |
| Resume-run path beyond existing `ResumeRun` RPC (approval-paused recovery) | execution-core + session-core | ⬜ |

### What's next (ordered)

1. **Draft `proto/model_plane/v1/orchestration.proto`** — messages + 3 services + `StreamRunEvents` RPC. Additive-only.
2. **session-core Postgres schema + migrations** for plans/todos/approvals/subagent_edges.
3. **session-core gRPC handlers** wired to `mp-orchestration` types (stateless; the crate enforces transitions).
4. **execution-core approval gate wiring** — when runtime loop hits `awaiting_approval`, create an `Approval` record and pause.
5. **orchestrator-core event subscription** — watch for `OrchestrationEvent` in NATS and surface in recovery.
6. **model-gateway SSE `/v1/runs/{id}/events`** streaming handler (reuse edge's SSE pattern from Quarry V2 for shape parity).

After Phase 1 closes, Phase 2 (capability platform parity) is the logical follow-up per `PLAN.md`.

---

## Quarry V2 — detailed

### What's done

- **Phase 0 ✅** — contracts frozen, Rust + Go mirrored.
- **Phase 1 Go control plane 🟡 7/10** — CRUD on mem + Postgres, event log, pgx pool, embedded migrations, cursor pagination, `/v1/jobs/{id}/history` merged view. New kinds added: `whk_`, `whkd_`, `block_`. Store tables for Webhook / WebhookDelivery / BlocklistEntry landed.
- **Phase 2 Rust runtime 🟡** — static driver, pipeline, security engine, fingerprint/diff, artifact store (in-mem), event sink, retry. Lease pool + action runtime + TLS driver + browser driver modules all scaffolded per updated `runtime/src/lib.rs`.
- **Phase 6 Output + change tracking 🟡** — envelope + blake3 + text-fingerprint + paragraph chunker done; diff stub pending.
- **Edge** — real `reqwest` handoff to control, SSE scaffold present, Redis config wired in state, `/v1/internal/run_page` live.

**Test count (approx):** ~51 across Rust + Go — all green.

### What's missing

**Phase 1 remaining (3 items):**
- Schedule cron parsing + enable/disable
- Webhook dispatcher loop (queue + HMAC + retry + DLQ) — store types exist; dispatcher code doesn't
- Blocklist persistence endpoint — store types exist; HTTP endpoint + security engine wiring missing

**Phase 2 remaining:**
- Real `chromiumoxide` browser driver impl (modules scaffolded but empty)
- TLS-profile driver (JA3 fingerprint) — module scaffolded
- Browserless remote driver
- Action runtime body (wait/click/scroll/screenshot/pdf/evaluate) — module scaffolded
- Lease pool TTL + eviction + affinity — module scaffolded
- S3 artifact backend behind `ArtifactStore` trait
- DNS resolution guard at fetch time
- Readability-first markdown converter
- Runtime → control event publisher (HTTP POST loop)
- Full heuristic port from donor `internal/security/heur/`

**Phases 3–5, 7–8:** all ⬜ (not started).

**Risk register unchanged** — see `Quarry-v2/docs/PROGRESS.md` §Known gaps.

### What's next (ordered)

1. **Webhook dispatcher** — store types shipped; need worker loop that polls `WebhookDelivery` where `status=pending AND next_attempt_at <= now()`, POSTs with HMAC, records outcome, exponential backoff, DLQ after 24h.
2. **Blocklist endpoint** — `/v1/security/blocklist` CRUD + wire reads into `quarry-security::preflight::DefaultEngine`.
3. **`chromiumoxide` BrowserDriver impl** — highest-leverage Phase 2 item. Unblocks JS-heavy sites + lease model (Phase 4).
4. **Runtime → control event publisher** — closes observability loop (runtime emits page events → control persists → edge streams via SSE to client).
5. **S3 artifact backend** — swap `InMemoryStore` at production wire-up point.

After Phase 2 closes, Phase 3 fast-path cutover becomes feasible.

---

## Cross-plane notes

- **No architectural drift.** Both planes hold the Rust-hot-path / Go-durable-control split. No Python crept into either runtime.
- **Contract style is consistent** across planes: prefixed ULIDs, additive-only evolution, state-machine invariants enforced at crate boundaries.
- **Shared eval direction:** mp-slo primitives could also serve Quarry V2 scoreboards in Phase 8. Not a prerequisite, but worth porting when the time comes.

---

## If you only do one thing next

- **Model Plane:** draft `orchestration.proto` — it defines the wire shape for all the Phase 1 service work and unblocks parallel implementation on session-core / execution-core / orchestrator-core / model-gateway at the same time.
- **Quarry V2:** ship the webhook dispatcher — store + contract types exist, dispatcher is the missing behavior, and it unlocks every downstream feature that emits webhooks.

Either is ~1 focused turn of work. Model Plane's proto draft has broader leverage; Quarry's webhook dispatcher has shorter distance to user-visible value.
