# CoreSystem — Production-Readiness Verification (2026-07-13)

**Date:** 13 July 2026
**Type:** Full cross-platform, **read-only** code-verification audit. No fixes applied, nothing deleted — verify, document, and record improvements only.
**Goal:** Establish an honest production-readiness baseline before going enterprise.
**Companion:** [SYSTEM_PRODUCTION_READINESS_ROADMAP_2026-07-13.md](SYSTEM_PRODUCTION_READINESS_ROADMAP_2026-07-13.md) — the complete, phased inventory of **all 151 findings** (every issue, warning, and improvement, no matter how light).

---

## ⚠️ Operator verification corrections (2026-07-13, evening) — read first

These two system docs were written ~09:18; authoritative **plane** docs were updated 13:44–19:14 and materially supersede parts of them. A full reconcile (classifying every finding as **source-tested / deployed / live-effective / blocked** and recomputing active totals) is still **pending**. Operator-verified corrections to apply on top of everything below:

- **"Everything else confirmed live" is obsolete.** Convex member-removal, information provenance, graph-preview authorization, ZDR propagation, and several auth boundaries are now **source-fixed but mostly UNDEPLOYED** — so their *live* behavior is unchanged until a deploy. Treat the "CONFIRMED-LIVE" list below as a morning snapshot, not current live state.
- **The ZDR "zero references in Data/Ingestion" claim is FALSE.** Current source has ZDR coverage across **dozens** of Data and Ingestion files (source-fixed; deploy pending). The roadmap's ZDR finding is corrected accordingly.
- **Data "all five release contracts green" no longer holds.** On re-run, **3 pass, 2 FAIL**: `compose-security-contract-test.sh` and `image-provenance-contract-test.sh`. Current compose permits **empty self-owned credentials** and **unverified/unknown image provenance**. Both must be fixed **before any image build**.
- **Compaction is healthy** (correct) — but the counter is **cumulative and resets on restart**: it read 163 successes / 0 errors after the latest restart. The 384/479 figures were valid historical snapshots, not permanent totals.
- **Docker currently works:** 97 containers / 93 running / 80 explicitly healthy / none unhealthy; `exec`, `logs`, `inspect`, `system df` all succeed. Disk remains risky at ~98% (~10–11 GiB free).
- **Still true live (undeployed fixes notwithstanding):** inference `:9092` closed; cost-core returns 200 with no auth; shipping uses the July-4 unlabelled image and exposes mock carriers without auth; Quarry runs `ENVIRONMENT=dev` with bearer-bypass on and Control HMAC disabled (both ports loopback-bound).
- A **partial internal-key fragment** that appeared in an earlier revision of these docs has been **redacted**. (It persists in local intermediate commit history; scrub before any first push.)

---

## Live re-verification addendum (2026-07-13, all six planes running)

After the first pass, the whole fleet was confirmed up (**93 containers, all healthy**) and every headline finding was re-tested by **hitting the running service directly** (`curl`/`docker exec`), not by reading source. Result: **15 claims CONFIRMED-LIVE, 3 retracted/stale, 2 corrected.** The load-bearing conclusion held; three findings did not survive live testing and are withdrawn below.

**The core point stands and is sharper now:** every container is green and healthy, yet **chat still cannot generate text.** "Running and healthy" ≠ "the feature works" — that gap is the whole story.

**CONFIRMED-LIVE (kept):**
- **P0 chat/inference DOWN — verified end-to-end.** inference-core logs *"gRPC is unavailable in the secure MVP"* (only `:8082` health); a TCP probe to `inference-core:9092` is **refused** from inside model-gateway, execution-core, **and** data-plane retrieval-engine. There is no provider fallback in the invoke handler — an authenticated `/v1/invoke` would `502`. This is the current running image (built 2026-07-12 23:21), not a stale binary. Data-Plane **semantic/vector retrieval** is caught in the same cascade (query-embedding routes through `:9092`); BM25/keyword retrieval is unaffected.
- **P0 shipping** Bring `transit_days:0` / `0001-01-01` reproduced live; running `/app/shipping-core` has zero `parseBringDeliveryDate`; fix still uncommitted. *(Correction: `/api/carriers` does not return `mode=null` — it returns `{code,name,segment,is_mock}` with no `mode` field; and the zero-date affects UPS/DHL Express too, not only Bring.)*
- **P1** cost-core zero-auth confirmed: `GET /api/v1/cost/aggregate` returned real ledger data and `POST /api/v1/budget/check` returned `{"allowed":true}` with **no** auth header.
- **P1** autocomplete-core NATS consumer dead ~8h (`missed idle heartbeat`, no restart) while `/health` returns `ok`.
- **P1** org→user membership divergence exact: `org_core.organization_members = 3` vs `user_service.user_org_memberships = 1`.
- **P1** convex `onOrganizationMemberRemoved` dispatched at `http.ts:276` but not exported in `nats.ts` (confirmed static).
- **P1** mock carriers win live quotes: cheapest returned quote was `mock-helthjem` (NOK 77) beating real Bring (NOK 92).
- **P2** HITL still only engages in Plan mode (the composer has no client-side pre-dispatch approval gate) — confirmed against the current controller.

**RETRACTED / corrected:**
- ❌ **session-core compaction "100% failing" — WITHDRAWN.** Live `/metrics`: `mp_session_compaction_runs_total{status="ok"} 384`, no error counter, no `checkpoints_run_id_fkey` in logs. The earlier reading does not reproduce; compaction is healthy. *(Was a P1 in the first pass.)*
- ❌ **verevonv3 chat "tool-surfacing gap" — WITHDRAWN.** `buildChatWireBody` (chat-client.ts:250-273) now calls `buildToolSpecs()` on every turn, adds the `tools` feature when any tool is selected and `agentic` in Plan mode. Tools are opt-in per turn (a bare turn with nothing selected correctly sends none) — that is by design, not a missing wire.
- ⚠️ **information-core fabricated metrics — half-corrected.** The FNV fabrication is still real in `internal/traffic/service.go:166`, but `/api/v1/traffic` now returns `401`, so the "exposed unauthenticated" framing is stale. It remains a data-integrity concern (fabricated numbers presented as `operational`) for any authenticated caller.
- ⚠️ **verevon-gateway `graph_preview` IDOR — nuance.** Real in current source (onboarding router has no `require_session`; `org_id` taken from the query with no ownership check). But the **running** binary returns `401` on all onboarding routes — the deployed gateway is *stricter* than HEAD (a stale-binary divergence). The authenticated cross-tenant vector still holds in source.

**Stack correction:** verevonv3 is **Vite + SolidJS**, but **not Tailwind** — no config, no `@tailwind`/`@apply`, not in `package.json` or `node_modules`. Real stack: SolidJS on Vite 8, `@kobalte/core` UI, `@tanstack/solid-query` data, `@tanstack/ai`(+ai-solid/ai-client, AG-UI) for chat, `three`/`3d-force-graph` for the graph, semantic CSS custom properties in a single `src/styles/global.css`. The chat wire is `streamChat` → gateway `/api/v1/chat/stream` → model-gateway `/v1/invoke/stream` (the AG-UI `/api/v1/ag-ui/stream` route exists but is not the default chat transport).

The corrected counts: of the 47 P1s, **1 is withdrawn** (compaction) and **1 partially downgraded** (information-core exposure); of the frontend items, the tool-surfacing gap is **withdrawn**. Everything else in the inventory stands as written.

---

## Method

Two background verification passes, 12 agents total, each producing structured findings with `file:line` / live-command evidence:

1. **Per-plane deep verification** (6 agents, one per plane) — ran the **real** build/vet/lint/test suites (`nest build` + jest, `go build/vet/test -cover`, `cargo test`, `pnpm typecheck/test`), probed live containers, and analysed wiring correctness, call-chains, duplication, dead code, and perf/reliability/durability. → **96 findings**.
2. **Cross-cutting analysis** (6 agents, read/grep/reason only — deliberately no compilation, to protect the 99%-full disk) — Docker/host reliability, cross-plane contract wiring, fleet-wide security, performance/benchmark readiness, cross-plane duplication, and reliability/durability patterns. → **55 findings**.

Scope: the six focused planes (`Control`, `Data Plane v2`, `Ingestion`, `Model`, `Application`, `Frontend/verevonv3`). Channel Plane remains docs-only and was not in scope.

---

## Verdict: **NOT production-ready.** Compiles and tests green everywhere; runtime, durability, security, and infra are not enterprise-grade yet.

| Dimension | Status | One-line reason |
|---|---|---|
| **Build / compile** | 🟢 Green | Every service in all six planes builds, vets, and lints clean. |
| **Unit/integration tests** | 🟢 Green (coverage uneven) | All suites pass (e.g. auth-core 116 tests; Go/Rust `test` green). Coverage is thin on the exact seams that are broken (NATS handlers, membership projection: 0–3%). |
| **Runtime wiring** | 🔴 Red | Rebuilt Model Plane images broke inference (`:9092` no-op); chat **and** Data-Plane retrieval embedding are down. Bring feature serves a stale binary. |
| **Durability / messaging** | 🔴 Red | Ephemeral core-NATS is the fleet-wide default; ack-on-error drops; no DLQs; no reconnect loops → silent data loss on any blip. |
| **Security / tenancy** | 🔴 Red | One shared `INTERNAL_API_KEY` across 3 planes; shared LLM/Azure keys; unauthenticated surfaces; live cross-tenant IDORs; ZDR never leaves Model Plane. |
| **Performance / scalability** | 🟠 Amber | No benchmark harness or SLOs anywhere; DB pool near-exhaustion; serialized retrieval fan-out; timeout-less gRPC channels. |
| **Infra / host** | 🔴 Red | One Docker-Desktop VM (7.75 GiB RAM, single disk) hosts all 93 containers; host disk 99% full (5.6 GiB); RAM oversubscribed; OOM/disk-full is a fleet-wide single point of failure. |
| **Observability** | 🟠 Amber | Health/readiness endpoints report green while background consumers are dead; no host disk/RAM alerting; audit-core is the only durable-JetStream consumer. |

**Bottom line:** the codebase is in good *engineering* shape (it builds, it's tested, the honest-failure paths mostly exist) but not in *operational* shape. Nothing here is a rewrite; the blockers are a deploy landmine, a stale artifact, and a set of systemic patterns (messaging durability, secret scoping, host capacity) that must be addressed before enterprise traffic.

---

## What changed since the 2026-07-11/12 audit

- **The Model Plane landmine has detonated.** The prior `SYSTEM_STATUS.md` said *"running containers predate the WIP, so chat works today."* That is now **false**. `model-plane-inference-core` was rebuilt **2026-07-13 01:21**, `model-gateway` **02:36**, `execution-core` **02:39** — all from the secure-MVP source where `inference-core/src/main.rs:29` replaces the gRPC server with `std::future::pending()`. General chat inference is **down** in the running fleet (honest SSE `error`, no fake success), and — newly discovered this pass — so is Data-Plane retrieval's embedding hop, which dials the same deleted `:9092` listener.
- **Docker/containerd recovered.** The blob-corruption incident that blocked `docker exec`/rebuild/logs in the prior pass is resolved; `application-postgres` is healthy again. But the **root cause is unaddressed** — the host disk is still 99% full and every rebuild eats into it, so the incident can recur at any time.
- **The Bring shipping fix is still not live and still uncommitted.** Correct in source (`parseBringDeliveryDate`), but the running container serves an 8-day-stale binary (BuildKit `--mount=type=cache` reuse), and the fix is working-tree only — a CI build from `HEAD` would not include it.

---

## The three P0 gates (nothing ships until these clear)

### P0-1 — Model Plane inference is DOWN in the rebuilt fleet (chat + retrieval cascade)
`apps/Model Plane/rust/services/inference-core/src/main.rs:29` — `let grpc_handle = tokio::spawn(async { std::future::pending::<anyhow::Result<()>>().await });`. Only `http_health` serves; the `:9092` gRPC inference API is gone (doc comment at `:3-4` confirms "not shipped in the secure MVP"). But:
- `model-gateway` dials it for **every** chat/AI call (`sse.rs`, ~30 `http_routes.rs` call sites).
- `execution-core` dials it for its agentic `Infer` round.
- **New this pass:** Data Plane v2 `embedding-engine-rs` (ingest) *and* `retrieval-engine-rs` (query embedding) dial the same `inference-core:9092`. So the outage is a **full retrieval-pipeline cascade**, not just chat — and because `embedding-engine`'s JetStream consumer has `max_deliver=5` and no DLQ, ingest events during the outage are retried 5× then **permanently dropped**.

**Remediation (documented, not applied):** split the security hardening from the server removal — restore inference-core's served transport (behind the intended signed-audience auth) *or* migrate the gateway + execution-core + DP embedding clients to a transport that is actually served. Do this before any Model Plane redeploy.

### P0-2 — Ingestion shipping-core serves an 8-day-stale binary (the headline feature is broken live)
Live `POST /api/quotes` returns every Bring product (and UPS/DHL Express) with `transit_days:0` / `estimated_delivery:"0001-01-01T00:00:00Z"`. The in-container `/app/shipping-core` (dated Jul-4) lacks the `parseBringDeliveryDate` symbol (live grep count 0); a fresh build of current source contains it. *(Correction from the first pass: `/api/carriers` does not carry a `mode` field at all — it returns `{code,name,segment,is_mock}`.)* Two-part cause: (1) the fix is **uncommitted** working-tree changes (`bring.go`, `wire.go`, `bring_test.go` all ` M`), and (2) a BuildKit `go-build` cache layer reused the Jul-4 compiled binary.
**Remediation:** commit the fix, then rebuild with `--no-cache` (the documented repo gotcha). Requires the disk headroom from P0-3 first.

### P0-3 — Host is a single point of failure for the entire fleet
All 93 containers run in **one** Docker-Desktop Linux VM with **7.75 GiB RAM** and a single disk image on a host volume that is **99% full (5.6 GiB free)**. Every plane's Postgres/JetStream/MinIO/Qdrant/logs is a named volume on that one disk.
- **Disk-full → fleet-wide write failure**: Postgres WAL `No space left on device` (crash/read-only), JetStream stops persisting, logs stop — simultaneously across all six planes. This is the same root cause as the prior containerd-corruption incident.
- **RAM oversubscribed → cross-plane OOM cascade**: the sum of declared memory limits already exceeds 7.75 GiB, and Model Plane (0/34 services capped), the zammad overlay (0/18), and the DP-v2 app tier run **uncapped**. A leak in any uncapped service triggers the VM-wide OOM killer, which can reap Control-Plane Postgres or a JetStream broker in an unrelated plane.

**Remediation (read-only recommendation):** reclaim ~16 GiB (`docker image/builder/volume prune` — ~9.97 + 4.03 + 2.19 GB reclaimable), relocate the Docker VM disk onto `/Volumes/Applikasjon` (241 GiB free) or `/Volumes/Lagring` (92 GiB free), add memory limits to the three uncapped stacks, and wire host disk/RAM alerting at 85%.

---

## Systemic themes (the enterprise-blocking finding *classes*)

These are patterns, not one-offs — each spans multiple services/planes and is the real work between "it runs on my machine" and "it survives enterprise traffic."

1. **Messaging durability is the biggest reliability gap.** Core-NATS (at-most-once, ephemeral) is the fleet default for *consumers*, even where a durable JetStream context is already in hand:
   - Control Plane: **4 of 4** event consumers ephemeral (~18 subscriptions) — this is the mechanism behind the broken **org→user membership projection** (`user_org_memberships` holds 1 of 3 orgs) and behind session-core's cache-invalidation gap (a stale membership/plan survives a revocation until TTL — security-adjacent).
   - GDPR **erasure** ownership-transfer subscriber (documents-api-go) is ephemeral with no ack/nak → an erasure requested during a restart orphans personal data (DSAR failure).
   - Where consumers *are* durable, several **ack (drop) on transient errors** with no DLQ: DP's 3-service indexing family (`index/embedding/graph-engine`) drops genuine events on a `ReplayCacheUnavailable` blip; autocomplete-core drops on a Sonic outage *and* dies on a stream error with no reconnect loop while `/ready` stays green.
   - Model Plane's `natsx` wrapper keeps an **unbounded** in-memory dedup map (memory leak + double-apply on restart).

2. **Secret scoping has fleet-wide blast radius.** One static `INTERNAL_API_KEY` (`<INTERNAL_API_KEY-redacted>…`) is shared across **Control + Data + Ingestion** planes (auth/user/org/billing/imports/integration/notification/support-worker); one Azure key backs ~10 Azure AI services across two planes; Anthropic/Google keys are shared 3–4×; two disjoint **symmetric HS256** `JWT_SECRET`s (user-core, convex) are token-*mint* capability at rest. A leak in the lowest-trust plane (Data Plane v2, historically no-cred readable) compromises paid inference and internal impersonation everywhere.

3. **Unauthenticated / IDOR surfaces remain live.** `cost-core` (no inbound auth; ledger read+write IDOR; budget check **fails open** on DB error → unlimited spend), `bridge-core` (attacker-controlled `org_id`), `quarry-edge` (dev-bypass shipped ON with no `ENVIRONMENT` guard → accepts any bearer), verevon-gateway `graph_preview` (client `?org_id=` cross-tenant read), `finspo-core` (trusts client `X-Org-ID` behind only the shared key), and HITL that only engages in Plan mode (composer writes un-gated on a normal turn).

4. **Duplication has already caused divergence (drift = bugs).** JWKS/JWT verification is reimplemented **7×** inside Data Plane alone (four Go copies, three Rust — different md5s, hardening landed in only one); the NATS client is copy-pasted **8+×** (Application-Plane copies have **no reconnect handling**); `shared_publisher.go` is triplicated and diverged; the Ingestion auth-middleware trio diverged (finspo the weak 61-line outlier). A fix in one copy silently doesn't reach the others.

5. **No performance floor.** There is **no benchmark or load-test harness anywhere** (no criterion, no k6/vegeta, no p50/p99 SLOs) — no enterprise latency claim is verifiable or regression-gated. Concretely: DP-v2's nine services sum to ~100 pool connections against a `max_connections=100` Postgres (zero headroom); retrieval fuses its 4 arms **sequentially** (~500–600 ms that could collapse to the slowest arm); a per-request **uncached** cross-plane visibility call couples DP retrieval latency to CP user-core health; all ~18 model-gateway gRPC channels use `connect_lazy()` with **no timeout/keepalive** (a dead `:9092` hangs the task).

6. **Data-integrity / trust leaks.** `information-core` emits **FNV-fabricated** `trafficVolume`/`averageSpeed` labelled `operational` (still true in code; endpoint is now `401`-gated so it's no longer *anonymously* exposed, but authenticated callers still get synthetic numbers); Ingestion **mock carriers** (postnord/dsv/helthjem/porterbuddy) are unconditionally registered and **win the cheapest-quote comparison** and feed the AI recommendation (live: `mock-helthjem` NOK 77 beat real Bring NOK 92) — a customer could book against fabricated prices.

7. **Tenancy isolation is table-name-deep.** All Application-Plane Go services share **one DB and one role** (`application_plane`/`appuser`); GDPR person-data (`provider_leads`) is co-resident with conversation/social content, no RLS, no schema separation (insight-core is the lone exception). One SQL-injection foothold reads every tenant's data.

8. **ZDR does not propagate.** Zero-Data-Retention is threaded through Model Plane protos but has **zero references** in Data Plane v2 or Ingestion — a ZDR-flagged run that persists documents/embeddings or captures evidence has no field to honor and retains content durably, violating the architecture rule.

---

## Per-plane snapshot

| Plane | Build/Test | Headline production gap |
|---|---|---|
| **Control Plane** | 🟢 all 6 services build/vet/test green; auth-core 116 tests | org→user membership projection broken (1/3 orgs); entire event bus ephemeral core-NATS; shared internal key. |
| **Data Plane v2** | 🟢 builds/tests green; Secure-MVP JWT hardening confirmed in `main.rs` | Retrieval embedding cascades on the MP P0; 3-service ack-on-error drop family; pool near-exhaustion; JWKS verify duplicated 7×. |
| **Ingestion Plane** | 🟢 builds/tests green (8 shipping quote tests pass) | P0 stale shipping binary; mock carriers win quotes; autocomplete-core dead consumer; quarry-edge dev-bypass ON. |
| **Model Plane** | 🟢 builds/tests green | **P0** inference `:9092` no-op (live-verified); cost-core IDOR + fail-open budget; bridge-core unauth. *(session-core compaction finding withdrawn — healthy live: 384 ok runs, 0 errors.)* |
| **Application Plane** | 🟢 builds/tests green; postgres healthy again | convex `onOrganizationMemberRemoved` undefined → 500 on every removal; information-core fabricated metrics; single DB/role, GDPR co-resident. |
| **Frontend verevonv3** | 🟢 typecheck/build/tests green; confirmed off mocks (SolidJS+Vite+Kobalte+TanStack, no Tailwind) | `graph_preview` `?org_id=` IDOR (source; running binary is stricter, 401); HITL only in Plan mode (composer writes un-gated). *(chat tool-surfacing gap withdrawn — `buildChatWireBody` now sends tools/agentic.)* |

Each plane's own `*_STATUS.md` / `*_ROADMAP.md` (under `apps/<Plane>/`) remain the per-plane source of truth; this document is the system-level roll-up.

---

## Positives worth preserving (don't regress these)

- **Control Plane compose is the gold standard** — per-service mem/cpu limits, log rotation, digest-pinned images, deep DB healthchecks, `127.0.0.1` bindings, `${X:?required}` env vars. Use it as the template for the other stacks.
- **Secret hygiene in compose is uniformly good** — no `CHANGE_ME`, every secret is `${X:?required}` or a `__FILE` secret; disciplined `127.0.0.1` host-port binding; no cross-stack port collisions.
- **Honest failure paths exist** — the gateway emits a real SSE `error` instead of faking success when inference is down; legacy unsigned-event consumers are fail-closed behind triple env-guards; audit-core runs a durable JetStream consumer with a DLQ (the model for the rest of the fleet).
- **auth-core uses a transactional outbox** with revision ordering and leases — the correct pattern that the ephemeral-NATS consumers should adopt on the read side.
- **verevonv3 is genuinely off mocks** and its gateway IDOR surface is mostly closed (`authorized_org_id` helper exists and is used — it just needs to be applied to `graph_preview`).

---

## Next step

See [the roadmap](SYSTEM_PRODUCTION_READINESS_ROADMAP_2026-07-13.md) for the full 151-item inventory, phased P0→P3 with per-item evidence and recommendations. Read-only audit: **no code was changed and nothing was deleted in producing this report.**
